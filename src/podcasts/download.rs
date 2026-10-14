//! Downloading an episode into the podcasts directory (legacy `podcasts/download.ts`).

use std::path::{Path, PathBuf};

use sqlx::PgPool;
use tokio::io::AsyncWriteExt;

use super::chapters::{ChapterMark, fetch_chapters_json, save_episode_chapters};
use super::paths::{episode_destination, is_inside, suffix_for};
use super::store::describe_transport_error;
use crate::scanner::chapters::read_chapters;
use crate::scanner::tags::read_tags;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("not found")]
    NotFound,
    /// 422: the feed entry has no audio file.
    #[error("{0}")]
    NotDownloadable(String),
    /// 502: the media URL could not be fetched.
    #[error("{0}")]
    Fetch(String),
    /// 507: the file could not be written.
    #[error("{0}")]
    Storage(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Downloaded {
    pub track_id: i32,
    pub duration: f64,
}

/// Chapters, best source first: the file's own marks (ID3 CHAP or an MP4 chapter track), then the
/// Podcasting 2.0 JSON the feed pointed at. Podlove chapters from the feed were already stored at
/// sync time and stay if neither turns up — but the file's are re-timed against its real length.
/// A missing or broken chapter source never costs the listener the episode.
async fn store_chapters(
    db: &PgPool,
    http: &reqwest::Client,
    episode_id: i32,
    path: &Path,
    chapters_url: Option<&str>,
    duration: f64,
) {
    let file = path.to_path_buf();
    let from_file = tokio::task::spawn_blocking(move || read_chapters(&file))
        .await
        .unwrap_or_default();
    let marks: Vec<ChapterMark> = if !from_file.is_empty() {
        from_file
            .into_iter()
            .map(|c| ChapterMark {
                start: c.start,
                title: c.title,
            })
            .collect()
    } else if let Some(url) = chapters_url {
        fetch_chapters_json(http, url).await
    } else {
        Vec::new()
    };
    if !marks.is_empty()
        && let Err(e) = save_episode_chapters(db, episode_id, &marks, duration).await
    {
        tracing::warn!("Kapitel für Folge {episode_id} nicht gespeichert: {e}");
    }
}

/// Streams `response` into `destination` via a `.part` file, so a broken transfer never leaves a
/// half file under the real name.
async fn write_file(
    mut response: reqwest::Response,
    destination: &Path,
) -> Result<(), DownloadError> {
    let mut temp = destination.as_os_str().to_owned();
    temp.push(".part");
    let temp = PathBuf::from(temp);
    let storage =
        |e: std::io::Error| DownloadError::Storage(format!("Speichern fehlgeschlagen: {e}"));
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(storage)?;
    }
    let result = async {
        let mut file = tokio::fs::File::create(&temp).await.map_err(storage)?;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => file.write_all(&chunk).await.map_err(storage)?,
                Ok(None) => break,
                Err(e) => {
                    return Err(DownloadError::Fetch(format!(
                        "Download abgebrochen: {}",
                        describe_transport_error(&e)
                    )));
                }
            }
        }
        file.flush().await.map_err(storage)?;
        drop(file);
        tokio::fs::rename(&temp, destination).await.map_err(storage)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
    result
}

pub async fn download_episode(
    db: &PgPool,
    http: &reqwest::Client,
    episode_id: i32,
    podcasts_dir: &Path,
) -> Result<Downloaded, DownloadError> {
    let episode: (String, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT episode.title, episode.feed_url, episode.chapters_url, podcast.title
         FROM items AS episode LEFT JOIN items AS podcast ON podcast.id = episode.parent_id
         WHERE episode.id = $1 AND episode.kind = 'episode'",
    )
    .bind(episode_id)
    .fetch_optional(db)
    .await?
    .ok_or(DownloadError::NotFound)?;
    let (title, media_url, chapters_url, podcast_title) = episode;
    let media_url = media_url.ok_or_else(|| {
        DownloadError::NotDownloadable("Keine Audiodatei in diesem Feed-Eintrag".into())
    })?;

    let destination = episode_destination(
        db,
        podcasts_dir,
        episode_id,
        &title,
        podcast_title.as_deref().unwrap_or("Podcast"),
        &suffix_for(&media_url),
    )
    .await?;

    let response = http.get(&media_url).send().await.map_err(|e| {
        DownloadError::Fetch(format!(
            "Folge nicht erreichbar: {}",
            describe_transport_error(&e)
        ))
    })?;
    if !response.status().is_success() {
        return Err(DownloadError::Fetch(format!(
            "HTTP {}",
            response.status().as_u16()
        )));
    }
    write_file(response, &destination).await?;

    let file = destination.clone();
    let duration = tokio::task::spawn_blocking(move || read_tags(&file).duration)
        .await
        .unwrap_or(0.0);
    store_chapters(
        db,
        http,
        episode_id,
        &destination,
        chapters_url.as_deref(),
        duration,
    )
    .await;

    let path = destination.to_string_lossy().into_owned();
    let mut tx = db.begin().await?;
    // A download under an earlier name (the title changed since) would otherwise block position 1.
    let stale: Vec<String> =
        sqlx::query_scalar("DELETE FROM tracks WHERE item_id = $1 AND path <> $2 RETURNING path")
            .bind(episode_id)
            .bind(&path)
            .fetch_all(&mut *tx)
            .await?;
    let track_id: i32 = sqlx::query_scalar(
        "INSERT INTO tracks (item_id, position, path, duration, title) VALUES ($1, 1, $2, $3, $4)
         ON CONFLICT (path) DO UPDATE SET duration = EXCLUDED.duration
         RETURNING id",
    )
    .bind(episode_id)
    .bind(&path)
    .bind(duration)
    .bind(&title)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    for old in stale.iter().map(PathBuf::from) {
        if is_inside(&old, podcasts_dir) {
            let _ = tokio::fs::remove_file(&old).await;
        }
    }
    Ok(Downloaded { track_id, duration })
}
