//! The background work at startup (legacy `hooks.server.ts` init): move old flat downloads into
//! their podcast folders, then refresh every feed every `AUTOREVERSE_PODCAST_REFRESH_HOURS` hours
//! and apply retention. Every path through this says something in the log: a loop that starts,
//! runs and finds nothing must not look like a loop that never started.

use std::time::Duration;

use super::relocate::relocate_legacy_downloads;
use super::retention::refresh_all_and_retain;
use crate::state::AppState;

/// Spawns the job; returns at once.
pub fn start(state: AppState) {
    tokio::spawn(async move {
        // Old downloads move before the refresh loop starts, never alongside it: a retention
        // sweep running at the same time would delete or re-download the files being renamed.
        match relocate_legacy_downloads(&state.db, &state.config.podcasts_dir).await {
            Ok(r) if r.moved > 0 || r.failed > 0 => tracing::info!(
                "[podcasts] {} alte Downloads in ihren Podcast-Ordner verschoben{}",
                r.moved,
                if r.failed > 0 {
                    format!(", {} ließen sich nicht verschieben", r.failed)
                } else {
                    String::new()
                }
            ),
            Ok(_) => {}
            Err(e) => tracing::error!("[podcasts] Verschieben alter Downloads fehlgeschlagen: {e}"),
        }
        refresh_loop(state).await;
    });
}

async fn refresh_loop(state: AppState) {
    let hours = state.config.podcast_refresh_hours;
    if hours <= 0.0 {
        tracing::info!(
            "[podcasts] automatische Aktualisierung ist aus (AUTOREVERSE_PODCAST_REFRESH_HOURS=0)"
        );
        return;
    }
    tracing::info!(
        "[podcasts] Feeds werden alle {hours} {} aktualisiert",
        if hours == 1.0 { "Stunde" } else { "Stunden" }
    );
    let mut interval = tokio::time::interval(Duration::from_secs_f64(hours * 3600.0));
    // A run that takes longer than the interval is not followed by a burst of catch-up runs.
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        run_once(&state).await;
    }
}

/// One refresh of every feed, logged.
pub async fn run_once(state: &AppState) {
    let config = &state.config;
    match refresh_all_and_retain(
        &state.db,
        &state.http,
        &config.cover_dir,
        &config.podcasts_dir,
    )
    .await
    {
        Ok(run) => {
            tracing::info!(
                "[podcasts] {} Feeds aktualisiert: {} neue Folgen, {} geladen, {} gelöscht, {} fehlgeschlagen",
                run.podcasts,
                run.new_episodes,
                run.downloaded,
                run.freed,
                run.failed
            );
            for error in run.errors {
                tracing::error!("[podcasts] {error}");
            }
        }
        Err(e) => tracing::error!("[podcasts] Aktualisierung fehlgeschlagen: {e}"),
    }
}
