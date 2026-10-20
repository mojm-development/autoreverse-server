//! Scan status (legacy `admin/scanState.ts`). One per server process, held in `AppState`:
//! `POST /scan` flips it to running and spawns the scan, `/scan/status` reads it while the scan
//! writes its progress into it.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ScanReport {
    pub new: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub removed: u32,
    pub skipped: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Scanning,
    Storing,
}

/// Which library a pass scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Library {
    Books,
    Music,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanProgress {
    pub phase: Phase,
    pub root: Option<String>,
    /// `None` while the folders are still being listed.
    pub total: Option<u32>,
    pub processed: u32,
    pub new: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub skipped: u32,
    // Beyond legacy, which the clients ignore: where in the run the scan is.
    pub library: Library,
    /// 1-based pass of `passes`.
    pub pass: u32,
    pub passes: u32,
    /// Folders found so far while listing the root, before `total` is known.
    pub found: u32,
    /// The folder last read or stored, relative to the root.
    pub current: Option<String>,
    /// When the phase began: rate and remaining time.
    #[serde(skip)]
    pub phase_started: Instant,
}

/// The wire shape of `/scan`, `/scan/status` and `/scan/cancel` as well.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScanState {
    pub running: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_report: Option<ScanReport>,
    pub last_error: Option<String>,
    pub last_skipped: Vec<String>,
    pub cancelled: bool,
    pub cancel_requested: bool,
    pub progress: Option<ScanProgress>,
}

/// Shared handle on the one [`ScanState`]. The lock is only ever held for a field update, never
/// across an `.await`.
#[derive(Debug, Clone, Default)]
pub struct ScanStatus(Arc<Mutex<ScanState>>);

impl ScanStatus {
    pub fn lock(&self) -> MutexGuard<'_, ScanState> {
        // A panic while holding the lock leaves plain data behind; keep serving it.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn snapshot(&self) -> ScanState {
        self.lock().clone()
    }

    /// Claims the scanner: false if a scan is already running.
    pub fn try_start(&self) -> bool {
        let mut state = self.lock();
        if state.running {
            return false;
        }
        state.running = true;
        state.cancel_requested = false;
        state.cancelled = false;
        state.progress = None;
        state.finished_at = None;
        true
    }

    /// Asks the running scan to stop: false if none is running.
    pub fn request_cancel(&self) -> bool {
        let mut state = self.lock();
        if !state.running {
            return false;
        }
        state.cancel_requested = true;
        true
    }

    pub fn cancel_requested(&self) -> bool {
        self.lock().cancel_requested
    }

    pub fn update_progress(&self, f: impl FnOnce(&mut ScanProgress)) {
        if let Some(progress) = self.lock().progress.as_mut() {
            f(progress);
        }
    }
}
