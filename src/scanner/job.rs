//! Scans the library every so many hours (Einstellungen → Server), so that new files appear by
//! themselves. A changed interval counts from the moment it is saved.
//!
//! Nobody watches this scan, so it is the cautious kind: it removes nothing from an empty folder
//! and leaves large disappearances for a person to confirm (`removal_problem`).

use std::time::Duration;

use super::run::run_scan_with;
use crate::library::paths::get_library_paths;
use crate::settings::{Ticker, period_hours};
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

fn log_period(period: Option<Duration>) {
    match period {
        None => tracing::info!("[scan] automatischer Scan ist aus"),
        Some(p) => {
            let hours = p.as_secs_f64() / 3600.0;
            tracing::info!(
                "[scan] Bibliothek wird alle {hours} {} gescannt",
                if hours == 1.0 { "Stunde" } else { "Stunden" }
            );
        }
    }
}

async fn scan_loop(state: AppState) {
    // Not at once: restarting the server should not mean a scan every time.
    let mut ticker = Ticker::new(&state.settings, |s| period_hours(s.scan_hours), log_period);
    while ticker.tick().await {
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
