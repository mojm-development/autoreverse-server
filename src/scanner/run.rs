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
use super::store::{delete_items, known_files, store_items, vanished_items};
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

/// Below this many vanished items an automatic scan removes them without a second thought.
const CAUTIOUS_MIN_ITEMS: usize = 20;
/// An automatic scan leaves this share or more of a library alone: nobody is watching it.
const CAUTIOUS_MAX_SHARE: f64 = 0.25;

/// Why vanished items must stay, or `None`. Removing an item takes its progress, favorites and
/// listening history with it. A share that is not mounted still looks like a readable folder
/// inside a container, just an empty one, so an empty root never removes anything.
pub fn removal_problem(
    doomed: usize,
    total: usize,
    found_nothing: bool,
    cautious: bool,
) -> Option<String> {
    if doomed == 0 {
        return None;
    }
    if found_nothing {
        return Some(format!(
            "Ordner ist leer, {doomed} Einträge bleiben erhalten. Ist das Laufwerk eingebunden?"
        ));
    }
    let large = doomed >= CAUTIOUS_MIN_ITEMS && doomed as f64 >= total as f64 * CAUTIOUS_MAX_SHARE;
    (cautious && large).then(|| {
        format!(
            "{doomed} von {total} Einträgen fehlen und bleiben erhalten. Stimmt das, bitte den \
             Scan von Hand starten."
        )
    })
}

/// Runs a scan somebody asked for. The caller has already claimed the status with `try_start`.
pub async fn run_scan(app: AppState) {
    run_scan_with(app, false).await;
}

/// Runs a scan to the end; a `cautious` one refuses to remove large parts of a library.
pub async fn run_scan_with(app: AppState, cautious: bool) {
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
    if let Err(e) = run_passes(&app, &passes, &mut guard.failures, cautious).await {
        status.lock().last_error = Some(e.to_string());
    }
    // What the tags left open is measured now, in the background: decoding a library takes
    // far longer than a scan may.
    super::loudness::start(&app);
    // New and changed covers get their thumbnails now rather than on the first page visit.
    crate::library::thumbs::spawn_render_ahead(app.db.clone(), app.config.thumbs_dir.clone());
}

async fn run_passes(
    app: &AppState,
    passes: &[(String, ItemKind)],
    failures: &mut Vec<ScanFailure>,
    cautious: bool,
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
        let (doomed, total) = vanished_items(&app.db, root, &found, &skipped).await?;
        match removal_problem(doomed.len(), total, found.is_empty(), cautious) {
            Some(problem) => root_errors.push(format!("{root}: {problem}")),
            None => totals.removed += delete_items(&app.db, &doomed).await? as u32,
        }
        status.lock().last_report = Some(totals);
    }
    crate::library::artist_covers::unify_artist_spellings(&app.db).await?;
    // After both passes, over every track: a text file put beside an unchanged album is found
    // here, and so is everything from before `has_lyrics` existed. A cancelled scan skips it —
    // somebody wanted the scan to stop — and the next one catches up.
    if !status.cancel_requested() {
        let cancel = status.clone();
        super::lyrics::refresh_flags(&app.db, move || cancel.cancel_requested()).await?;
        // Loudness tags of new and changed files, and of everything from before the columns
        // existed: read here, so the values are there when the scan reports done.
        let cancel = status.clone();
        super::loudness::read_pending_tags(&app.db, move || cancel.cancel_requested()).await?;
    }
    if !root_errors.is_empty() {
        status.lock().last_error = Some(root_errors.join(" · "));
    }
    Ok(())
}

#[cfg(test)]
mod removal_tests {
    use super::removal_problem;

    #[test]
    fn nothing_vanished_is_no_problem() {
        assert_eq!(removal_problem(0, 100, true, true), None);
    }

    #[test]
    fn an_empty_root_never_removes_anything() {
        assert!(removal_problem(3, 3, true, false).is_some());
        assert!(removal_problem(3, 3, true, true).is_some());
    }

    #[test]
    fn a_large_disappearance_waits_for_a_person_only_when_automatic() {
        assert!(removal_problem(50, 100, false, true).is_some());
        assert_eq!(removal_problem(50, 100, false, false), None);
    }

    #[test]
    fn ordinary_clean_up_goes_through() {
        assert_eq!(removal_problem(5, 100, false, true), None);
        assert_eq!(removal_problem(19, 20, false, true), None);
        assert_eq!(removal_problem(30, 1000, false, true), None);
    }
}
