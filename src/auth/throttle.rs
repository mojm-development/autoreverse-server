//! Sliding-window login throttle per lower-cased name (legacy `throttle.ts`).
//!
//! In memory and per process, like the original: a restart forgets the history, which is fine
//! for a self-hosted server and keeps the database out of the login hot path.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const LIMIT: usize = 10;
const WINDOW_SECONDS: f64 = 300.0;
const MAX_KEYS: usize = 10_000;

type Clock = Box<dyn Fn() -> f64 + Send + Sync>;

pub struct LoginThrottle {
    failures: Mutex<HashMap<String, Vec<f64>>>,
    limit: usize,
    window_seconds: f64,
    max_keys: usize,
    now: Clock,
}

impl Default for LoginThrottle {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginThrottle {
    pub fn new() -> Self {
        Self::with_clock(Box::new(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
        }))
    }

    /// A throttle reading time (in seconds) from `now` — for tests.
    pub fn with_clock(now: Clock) -> Self {
        LoginThrottle {
            failures: Mutex::new(HashMap::new()),
            limit: LIMIT,
            window_seconds: WINDOW_SECONDS,
            max_keys: MAX_KEYS,
            now,
        }
    }

    pub fn with_max_keys(mut self, max_keys: usize) -> Self {
        self.max_keys = max_keys;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<f64>>> {
        // A panic while holding the lock cannot leave the map inconsistent in a way that
        // matters for a throttle; keep serving.
        self.failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Drops timestamps outside the window; returns the seconds left if `key` is locked out.
    fn prune(&self, map: &mut HashMap<String, Vec<f64>>, key: &str) -> Option<f64> {
        let cutoff = (self.now)() - self.window_seconds;
        let timestamps = map.get_mut(key)?;
        timestamps.retain(|&t| t > cutoff);
        if timestamps.is_empty() {
            map.remove(key);
            return None;
        }
        if timestamps.len() >= self.limit {
            let oldest = timestamps[0];
            return Some((oldest + self.window_seconds - (self.now)()).max(0.0));
        }
        None
    }

    fn append_failure(&self, map: &mut HashMap<String, Vec<f64>>, key: &str) {
        let now = (self.now)();
        if !map.contains_key(key) && map.len() >= self.max_keys {
            let cutoff = now - self.window_seconds;
            map.retain(|_, timestamps| timestamps.iter().any(|&t| t > cutoff));
            // Still full of live keys: refuse to grow rather than evict someone's history.
            if map.len() >= self.max_keys {
                return;
            }
        }
        map.entry(key.to_owned()).or_default().push(now);
    }

    /// Counts this attempt up front. Returns the seconds until the next attempt is allowed if
    /// the key is locked out, otherwise records the attempt and returns `None`.
    pub fn check_and_record_attempt(&self, key: &str) -> Option<f64> {
        let mut map = self.lock();
        if let Some(remaining) = self.prune(&mut map, key) {
            return Some(remaining);
        }
        self.append_failure(&mut map, key);
        None
    }

    pub fn record_success(&self, key: &str) {
        self.lock().remove(key);
    }

    pub fn record_failure(&self, key: &str) {
        let mut map = self.lock();
        self.prune(&mut map, key);
        self.append_failure(&mut map, key);
    }

    /// Seconds remaining if `key` is locked out, without recording anything.
    pub fn check(&self, key: &str) -> Option<f64> {
        let mut map = self.lock();
        self.prune(&mut map, key)
    }

    pub fn size(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A clock the test can move; stores seconds as f64 bits.
    fn clock(start: f64) -> (Arc<AtomicU64>, LoginThrottle) {
        let now = Arc::new(AtomicU64::new(start.to_bits()));
        let reader = now.clone();
        let throttle = LoginThrottle::with_clock(Box::new(move || {
            f64::from_bits(reader.load(Ordering::SeqCst))
        }));
        (now, throttle)
    }

    #[test]
    fn allows_attempts_under_the_limit() {
        let (_, throttle) = clock(0.0);
        for _ in 0..10 {
            assert_eq!(throttle.check_and_record_attempt("oliver"), None);
        }
    }

    #[test]
    fn locks_out_after_the_eleventh_attempt_within_the_window() {
        let (_, throttle) = clock(0.0);
        for _ in 0..10 {
            throttle.check_and_record_attempt("oliver");
        }
        let remaining = throttle.check_and_record_attempt("oliver").unwrap();
        assert!(remaining > 0.0 && remaining <= 300.0);
    }

    #[test]
    fn slides_the_window() {
        let (now, throttle) = clock(0.0);
        for _ in 0..10 {
            throttle.check_and_record_attempt("oliver");
        }
        now.store(301f64.to_bits(), Ordering::SeqCst);
        assert_eq!(throttle.check_and_record_attempt("oliver"), None);
    }

    #[test]
    fn record_success_clears_history_for_the_key() {
        let (_, throttle) = clock(0.0);
        for _ in 0..10 {
            throttle.check_and_record_attempt("oliver");
        }
        throttle.record_success("oliver");
        assert_eq!(throttle.check_and_record_attempt("oliver"), None);
    }

    #[test]
    fn keys_are_independent() {
        let (_, throttle) = clock(0.0);
        for _ in 0..10 {
            throttle.check_and_record_attempt("oliver");
        }
        assert_eq!(throttle.check_and_record_attempt("mara"), None);
    }

    #[test]
    fn check_does_not_record() {
        let (_, throttle) = clock(0.0);
        for _ in 0..9 {
            throttle.check_and_record_attempt("oliver");
        }
        assert_eq!(throttle.check("oliver"), None);
        assert_eq!(throttle.check("oliver"), None);
        throttle.record_failure("oliver");
        assert!(throttle.check("oliver").is_some());
    }

    #[test]
    fn a_full_table_evicts_expired_keys_but_never_live_ones() {
        let (now, throttle) = clock(0.0);
        let throttle = throttle.with_max_keys(2);
        throttle.check_and_record_attempt("a");
        throttle.check_and_record_attempt("b");
        // Full of live keys: "c" is not tracked.
        throttle.check_and_record_attempt("c");
        assert_eq!(throttle.size(), 2);
        // Once a and b expire, c gets in.
        now.store(400f64.to_bits(), Ordering::SeqCst);
        throttle.check_and_record_attempt("c");
        assert_eq!(throttle.size(), 1);
    }
}
