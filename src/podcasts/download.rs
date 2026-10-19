//! Downloading an episode into the podcasts directory (legacy `podcasts/download.ts`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

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
    http: &super::net::FeedHttp<'_>,
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

/// One lock per episode for this process. Two downloads of the same episode (a click in the web
/// UI while the retention run or a second client fetches it) used to share one `.part` file:
/// `File::create` truncated the other's, and the slower one kept writing into the finished file.
/// Process-wide like the scan status, keyed by podcasts directory and episode: one server process
/// owns its directory (and tests, each with their own directory, do not wait on each other).
type LockKey = (PathBuf, i32);
static EPISODE_LOCKS: LazyLock<Mutex<HashMap<LockKey, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(Default::default);

fn episode_lock(podcasts_dir: &Path, episode_id: i32) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = EPISODE_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    // Drop the entries nobody holds or waits for, so the map stays as small as the work in flight.
    locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    locks
        .entry((podcasts_dir.to_path_buf(), episode_id))
        .or_default()
        .clone()
}

/// `<destination>.<random>.part`: unique per run, so even a second process writing into the same
/// directory cannot truncate this one's file.
fn temp_path(destination: &Path) -> PathBuf {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS random source failed");
    let tag: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let mut temp = destination.as_os_str().to_owned();
    temp.push(format!(".{tag}.part"));
    PathBuf::from(temp)
}

/// Streams `response` into `destination` via a `.part` file, so a broken transfer never leaves a
/// half file under the real name.
async fn write_file(
    mut response: reqwest::Response,
    destination: &Path,
) -> Result<(), DownloadError> {
    let temp = temp_path(destination);
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

/// The episode's current download, if its file is still on disk.
async fn existing_download(
    db: &PgPool,
    episode_id: i32,
) -> Result<Option<Downloaded>, sqlx::Error> {
    let row: Option<(i32, f64, String)> = sqlx::query_as(
        "SELECT id, duration, path FROM tracks WHERE item_id = $1 ORDER BY position, id LIMIT 1",
    )
    .bind(episode_id)
    .fetch_optional(db)
    .await?;
    Ok(match row {
        Some((track_id, duration, path)) if tokio::fs::try_exists(&path).await.unwrap_or(false) => {
            Some(Downloaded { track_id, duration })
        }
        _ => None,
    })
}

/// Downloads the episode, one run per episode at a time. A caller that had to wait for a running
/// download of the same episode gets that result instead of fetching the file a second time; an
/// uncontended call always fetches (that is how a listener re-downloads on purpose).
pub async fn download_episode(
    db: &PgPool,
    http: &super::HttpClients,
    episode_id: i32,
    podcasts_dir: &Path,
) -> Result<Downloaded, DownloadError> {
    let lock = episode_lock(podcasts_dir, episode_id);
    let _guard = match lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            let guard = lock.lock().await;
            if let Some(done) = existing_download(db, episode_id).await? {
                return Ok(done);
            }
            guard
        }
    };
    download_locked(db, http, episode_id, podcasts_dir).await
}

async fn download_locked(
    db: &PgPool,
    http: &super::HttpClients,
    episode_id: i32,
    podcasts_dir: &Path,
) -> Result<Downloaded, DownloadError> {
    let episode: (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT episode.title, episode.feed_url, episode.chapters_url, podcast.title,
                podcast.feed_url
         FROM items AS episode LEFT JOIN items AS podcast ON podcast.id = episode.parent_id
         WHERE episode.id = $1 AND episode.kind = 'episode'",
    )
    .bind(episode_id)
    .fetch_optional(db)
    .await?
    .ok_or(DownloadError::NotFound)?;
    let (title, media_url, chapters_url, podcast_title, podcast_feed) = episode;
    let http = http.for_feed(podcast_feed.as_deref()).await;
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

    let request = http.get(&media_url).ok_or_else(|| {
        DownloadError::Fetch("Folge nicht erreichbar: Adresse im lokalen Netz".into())
    })?;
    let response = request.send().await.map_err(|e| {
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
        &http,
        episode_id,
        &destination,
        chapters_url.as_deref(),
        duration,
    )
    .await;

    let path = destination.to_string_lossy().into_owned();
    let mut tx = db.begin().await?;
    // A download under an earlier name (the title changed since) moves the existing track row to
    // the new file instead of replacing it: favorites, playlist entries and metadata edits point
    // at the track id and would go with a deleted row (ON DELETE CASCADE).
    let current: Option<(i32, String)> = sqlx::query_as(
        "SELECT id, path FROM tracks WHERE item_id = $1 ORDER BY position, id LIMIT 1 FOR UPDATE",
    )
    .bind(episode_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (track_id, stale) = match current {
        Some((id, old_path)) => {
            sqlx::query("UPDATE tracks SET path = $2, duration = $3 WHERE id = $1")
                .bind(id)
                .bind(&path)
                .bind(duration)
                .execute(&mut *tx)
                .await?;
            (id, (old_path != path).then_some(old_path))
        }
        None => {
            let id: i32 = sqlx::query_scalar(
                "INSERT INTO tracks (item_id, position, path, duration, title)
                 VALUES ($1, 1, $2, $3, $4)
                 ON CONFLICT (path) DO UPDATE SET duration = EXCLUDED.duration
                 RETURNING id",
            )
            .bind(episode_id)
            .bind(&path)
            .bind(duration)
            .bind(&title)
            .fetch_one(&mut *tx)
            .await?;
            (id, None)
        }
    };
    tx.commit().await?;
    if let Some(old) = stale.map(PathBuf::from)
        && is_inside(&old, podcasts_dir)
    {
        let _ = tokio::fs::remove_file(&old).await;
    }
    Ok(Downloaded { track_id, duration })
}
