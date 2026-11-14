//! Scans the library every `AUTOREVERSE_SCAN_HOURS` hours, so that new files appear by themselves.
//!
//! Nobody watches this scan, so it is the cautious kind: it removes nothing from an empty folder
//! and leaves large disappearances for a person to confirm (`removal_problem`).

use std::time::Duration;

use tokio::time::{Instant, MissedTickBehavior, interval_at};

use super::run::run_scan_with;
use crate::library::paths::get_library_paths;
use crate::state::AppState;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The library folders are not set yet, which is normal right after installing.
    NotConfigured,
    /// Somebody started a scan by hand; one at a time is enough.
    AlreadyRunning,
    Finished,
}

/// Spawns the job; returns at once.
pub fn start(state: AppState) {
    tokio::spawn(scan_loop(state));
}

async fn scan_loop(state: AppState) {
    let hours = state.config.scan_hours;
    if hours <= 0.0 {
        tracing::info!("[scan] automatischer Scan ist aus (AUTOREVERSE_SCAN_HOURS=0)");
        return;
    }
    tracing::info!(
        "[scan] Bibliothek wird alle {hours} {} gescannt",
        if hours == 1.0 { "Stunde" } else { "Stunden" }
    );
    // Not at once: restarting the server should not mean a scan every time.
    let period = Duration::from_secs_f64(hours * 3600.0);
    let mut interval = interval_at(Instant::now() + period, period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match run_once(&state).await {
            Ok(Outcome::Finished) => {
                let snapshot = state.scan.snapshot();
                match snapshot.last_error {
                    Some(error) => tracing::warn!("[scan] automatischer Scan: {error}"),
                    None => tracing::info!("[scan] automatischer Scan beendet"),
                }
            }
            Ok(Outcome::AlreadyRunning) => {
                tracing::info!("[scan] übersprungen, es läuft bereits ein Scan");
            }
            Ok(Outcome::NotConfigured) => {
                tracing::info!(
                    "[scan] übersprungen, die Bibliotheken sind noch nicht eingerichtet"
                );
            }
            Err(e) => tracing::error!("[scan] automatischer Scan fehlgeschlagen: {e}"),
        }
    }
}

/// One scan to its end, unless there is nothing to scan or one is already running.
pub async fn run_once(state: &AppState) -> Result<Outcome, sqlx::Error> {
    let paths = get_library_paths(&state.db).await?;
    if paths.books_dir.is_none() || paths.music_dir.is_none() {
        return Ok(Outcome::NotConfigured);
    }
    if !state.scan.try_start() {
        return Ok(Outcome::AlreadyRunning);
    }
    run_scan_with(state.clone(), true).await;
    Ok(Outcome::Finished)
}
