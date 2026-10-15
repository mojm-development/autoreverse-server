//! Moving flat old downloads into `<Podcast>/<Folge>.<ext>` (legacy `podcasts/relocate.ts`).

use std::path::{Path, PathBuf};

use sqlx::PgPool;

use super::paths::{episode_destination, is_inside};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelocationResult {
    pub moved: u32,
    pub skipped: u32,
    pub failed: u32,
}

/// Episodes downloaded before the folder layout existed sit flat in the podcasts root, named after
/// their id. Move them where the downloader would put them today and carry `tracks.path` along, so
/// the old files stay playable instead of being re-downloaded.
///
/// Left alone: anything already in place, a row whose file is gone, and any path outside the
/// podcasts root. The extension comes from the file itself rather than the feed. Runs at startup;
/// once everything has moved it costs one query per downloaded episode and no file I/O.
pub async fn relocate_legacy_downloads(
    db: &PgPool,
    podcasts_dir: &Path,
) -> Result<RelocationResult, sqlx::Error> {
    let rows: Vec<(i32, String, i32, String, String)> = sqlx::query_as(
        "SELECT track.id, track.path, episode.id, episode.title, podcast.title
         FROM tracks AS track
         JOIN items AS episode ON episode.id = track.item_id
         JOIN items AS podcast ON podcast.id = episode.parent_id
         WHERE episode.kind = 'episode'",
    )
    .fetch_all(db)
    .await?;

    let mut result = RelocationResult::default();
    for (track_id, path, episode_id, episode_title, podcast_title) in rows {
        let current = PathBuf::from(&path);
        if !is_inside(&current, podcasts_dir)
            || !tokio::fs::try_exists(&current).await.unwrap_or(false)
        {
            result.skipped += 1;
            continue;
        }
        let suffix = current
            .extension()
            .and_then(|e| e.to_str())
            .map_or_else(|| ".mp3".to_owned(), |e| format!(".{e}"));
        let destination = episode_destination(
            db,
            podcasts_dir,
            episode_id,
            &episode_title,
            &podcast_title,
            &suffix,
        )
        .await?;
        if destination == current {
            result.skipped += 1;
            continue;
        }
        let moved = async {
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::rename(&current, &destination).await
        }
        .await;
        if moved.is_err() {
            // A failed move leaves the row pointing at the file that is still there.
            result.failed += 1;
            continue;
        }
        sqlx::query("UPDATE tracks SET path = $2 WHERE id = $1")
            .bind(track_id)
            .bind(destination.to_string_lossy().as_ref())
            .execute(db)
            .await?;
        result.moved += 1;
    }
    Ok(result)
}
