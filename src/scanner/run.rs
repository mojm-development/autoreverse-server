//! One full scan (legacy `scanner/run.ts`): every library root as its own pass.
//!
//! Each pass has its own `root`, and `root` scopes the removal of vanished items — merging passes
//! or swapping roots would delete the other library's items. A root that cannot be read is
//! skipped entirely rather than scanned as empty, for the same reason.

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::{SecondsFormat, Utc};

use super::books::{ItemKind, ScanFailure, library_root_problem, scan_tree};
use super::state::{Phase, ScanProgress, ScanReport, ScanStatus};
use super::store::{known_files, remove_vanished, store_items};
use crate::library::paths::get_library_paths;
use crate::state::AppState;

pub const PATHS_MISSING: &str =
    "Bibliothekspfade sind noch nicht konfiguriert (Einstellungen → Bibliotheken)";

/// Status timestamps keep their milliseconds, as legacy's `toISOString()` did: two scans that
/// finish within the same second must still be told apart by `finished_at`.
fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Marks the scan finished even if the task panics or is dropped.
struct FinishGuard {
    status: ScanStatus,
    failures: Vec<ScanFailure>,
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        let mut state = self.status.lock();
        state.running = false;
        state.finished_at = Some(now_iso());
        state.progress = None;
        state.last_skipped = self
            .failures
            .iter()
            .map(|f| format!("{}: {}", f.path, f.message))
            .collect();
    }
}

/// Runs a scan to the end. The caller has already claimed the status with `try_start`.
pub async fn run_scan(app: AppState) {
    let status = app.scan.clone();
    let paths = match get_library_paths(&app.db).await {
        Ok(paths) => paths,
        Err(e) => {
            let mut state = status.lock();
            state.last_error = Some(e.to_string());
            state.running = false;
            state.finished_at = Some(now_iso());
            return;
        }
    };
    let (Some(books_dir), Some(music_dir)) = (paths.books_dir, paths.music_dir) else {
        let mut state = status.lock();
        state.last_error = Some(PATHS_MISSING.into());
        state.running = false;
        state.finished_at = Some(now_iso());
        return;
    };
    {
        let mut state = status.lock();
        state.started_at = Some(now_iso());
        state.finished_at = None;
        state.cancelled = false;
        state.last_error = None;
        state.last_report = None;
    }
    let mut guard = FinishGuard {
        status: status.clone(),
        failures: Vec::new(),
    };

    // Two separate passes, each with its own root: the root scopes what a pass may remove.
    let passes = [(books_dir, ItemKind::Book), (music_dir, ItemKind::Album)];
    if let Err(e) = run_passes(&app, &passes, &mut guard.failures).await {
        status.lock().last_error = Some(e.to_string());
    }
    // New and changed covers get their thumbnails now rather than on the first page visit.
    crate::library::thumbs::spawn_render_ahead(app.db.clone(), app.config.thumbs_dir.clone());
}

async fn run_passes(
    app: &AppState,
    passes: &[(String, ItemKind)],
    failures: &mut Vec<ScanFailure>,
) -> Result<(), sqlx::Error> {
    let status = &app.scan;
    // Stored cover paths are absolute, so they do not depend on the working directory.
    let covers_dir =
        std::path::absolute(&app.config.cover_dir).unwrap_or_else(|_| app.config.cover_dir.clone());
    let mut root_errors = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut totals = ScanReport::default();

    for (root, kind) in passes {
        if status.cancel_requested() {
            status.lock().cancelled = true;
            break;
        }
        let root_path = PathBuf::from(root);
        let problem = {
            let root_path = root_path.clone();
            tokio::task::spawn_blocking(move || library_root_problem(&root_path))
                .await
                .unwrap_or_else(|e| Some(e.to_string()))
        };
        if let Some(problem) = problem {
            root_errors.push(format!("{root}: {problem}"));
            continue;
        }

        let known = known_files(&app.db).await?;
        status.lock().progress = Some(ScanProgress {
            phase: Phase::Scanning,
            root: Some(root.clone()),
            total: None,
            processed: 0,
            new: totals.new,
            updated: totals.updated,
            unchanged: totals.unchanged,
            skipped: totals.skipped,
        });

        let (scanned, mut root_failures) = {
            let status = status.clone();
            let kind = *kind;
            tokio::task::spawn_blocking(move || {
                let mut failures = Vec::new();
                let scanned = scan_tree(
                    &root_path,
                    kind,
                    &known,
                    &mut failures,
                    |processed, total| {
                        status.update_progress(|p| {
                            // Folders finish on several threads; a late report must not go back.
                            p.processed = p.processed.max(processed as u32);
                            p.total = Some(total as u32);
                        })
                    },
                    || status.cancel_requested(),
                );
                (scanned, failures)
            })
            .await
            .map_err(|e| sqlx::Error::Protocol(format!("scan task failed: {e}")))?
        };
        // Stopped halfway: what was not walked would count as vanished. Keep everything.
        if status.cancel_requested() {
            status.lock().cancelled = true;
            failures.append(&mut root_failures);
            break;
        }

        status.update_progress(|p| {
            p.phase = Phase::Storing;
            p.processed = 0;
            p.total = Some(scanned.len() as u32);
        });
        let report = store_items(
            &app.db,
            &scanned,
            &covers_dir,
            &mut root_failures,
            |processed, total, counts| {
                status.update_progress(|p| {
                    p.processed = processed as u32;
                    p.total = Some(total as u32);
                    p.new = totals.new + counts.new;
                    p.updated = totals.updated + counts.updated;
                    p.unchanged = totals.unchanged + counts.unchanged;
                    p.skipped = totals.skipped + counts.skipped;
                })
            },
            || status.cancel_requested(),
        )
        .await;
        totals.new += report.new;
        totals.updated += report.updated;
        totals.unchanged += report.unchanged;
        skipped.extend(root_failures.iter().map(|f| f.path.clone()));
        failures.append(&mut root_failures);
        totals.skipped = failures.len() as u32;
        if status.cancel_requested() {
            let mut state = status.lock();
            state.cancelled = true;
            state.last_report = Some(totals);
            break;
        }

        let found: HashSet<String> = scanned.iter().map(|s| s.source_path.clone()).collect();
        totals.removed += remove_vanished(&app.db, root, &found, &skipped).await? as u32;
        status.lock().last_report = Some(totals);
    }
    crate::library::artist_covers::unify_artist_spellings(&app.db).await?;
    if !root_errors.is_empty() {
        status.lock().last_error = Some(root_errors.join(" · "));
    }
    Ok(())
}
