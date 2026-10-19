//! Subscriptions (legacy `podcasts/store.ts`): fetch a feed, store the podcast and its episodes,
//! refresh, unsubscribe.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::PgPool;

use super::chapters::ChapterMark;
use super::feed::{ParsedEpisode, ParsedFeed, parse_feed};
use super::paths::{is_inside, prune_empty_podcast_dir};
use crate::scanner::covers::write_cover_bytes;

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    /// The feed could not be fetched (502).
    #[error("{0}")]
    Fetch(String),
    /// What came back is no feed (422).
    #[error("{0}")]
    Invalid(String),
    #[error("not found")]
    NotFound,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

const FEED_TIMEOUT: Duration = Duration::from_secs(20);
/// Far above any real feed (long-running shows with full show notes reach a few MB).
const MAX_FEED_BYTES: usize = 64 * 1024 * 1024;
const MAX_COVER_BYTES: usize = 5_000_000;

/// A transport failure in a few German words, without the URL reqwest would add.
pub fn describe_transport_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "Zeitüberschreitung".into()
    } else if e.is_builder() {
        "Ungültige Adresse".into()
    } else if e.is_connect() {
        "Verbindung fehlgeschlagen".into()
    } else if e.is_redirect() {
        "Zu viele Weiterleitungen".into()
    } else {
        "Übertragung fehlgeschlagen".into()
    }
}

pub async fn fetch_feed_text(http: &reqwest::Client, feed_url: &str) -> Result<String, FeedError> {
    let response = http
        .get(feed_url)
        .timeout(FEED_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            FeedError::Fetch(format!(
                "Feed nicht erreichbar: {}",
                describe_transport_error(&e)
            ))
        })?;
    if !response.status().is_success() {
        return Err(FeedError::Fetch(format!(
            "HTTP {}",
            response.status().as_u16()
        )));
    }
    super::text_capped(response, MAX_FEED_BYTES)
        .await
        .map_err(|e| {
            FeedError::Fetch(format!(
                "Feed nicht erreichbar: {}",
                describe_transport_error(&e)
            ))
        })?
        .ok_or_else(|| FeedError::Fetch("Feed ist zu groß".into()))
}

pub async fn preview_feed(http: &reqwest::Client, feed_url: &str) -> Result<ParsedFeed, FeedError> {
    let raw = fetch_feed_text(http, feed_url).await?;
    parse_feed(&raw).map_err(|e| FeedError::Invalid(e.0))
}

/// What subscribing or refreshing did.
#[derive(Debug, Clone)]
pub struct Synced {
    pub id: i32,
    pub title: String,
    pub feed_url: Option<String>,
    pub cover_path: Option<String>,
    pub new_episodes: u32,
    pub updated_episodes: u32,
}

/// Inserts new episodes and updates known ones, matched by `(parent_id, guid)`.
async fn sync_episodes(
    db: &PgPool,
    podcast_id: i32,
    episodes: &[ParsedEpisode],
) -> Result<(u32, u32), sqlx::Error> {
    let (mut new_episodes, mut updated_episodes) = (0, 0);
    let mut tx = db.begin().await?;
    for episode in episodes {
        let (episode_id, inserted): (i32, bool) = sqlx::query_as(
            "INSERT INTO items (kind, parent_id, title, sort_title, description, feed_duration,
                 guid, feed_url, published_at, chapters_url)
             VALUES ('episode', $1, $2, lower($2), $3, $4, $5, $6, $7, $8)
             ON CONFLICT (parent_id, guid) WHERE guid IS NOT NULL DO UPDATE SET
                 title = EXCLUDED.title, sort_title = EXCLUDED.sort_title,
                 description = EXCLUDED.description, feed_url = EXCLUDED.feed_url,
                 published_at = EXCLUDED.published_at, chapters_url = EXCLUDED.chapters_url,
                 feed_duration = EXCLUDED.feed_duration
             RETURNING id, (xmax = 0)",
        )
        .bind(podcast_id)
        .bind(&episode.title)
        .bind(&episode.description)
        .bind(episode.duration_seconds)
        .bind(&episode.guid)
        .bind(&episode.media_url)
        .bind(episode.published_at)
        .bind(&episode.chapters_url)
        .fetch_one(&mut *tx)
        .await?;
        if inserted {
            new_episodes += 1;
        } else {
            updated_episodes += 1;
        }
        // Podlove chapters ride along in the feed, so they cost nothing to store here. The file's
        // own chapters, read at download time, take precedence over these.
        if !episode.chapters.is_empty() {
            let rows = super::chapters::to_chapter_rows(
                &episode
                    .chapters
                    .iter()
                    .map(|c| ChapterMark {
                        start: c.start,
                        title: c.title.clone(),
                    })
                    .collect::<Vec<_>>(),
                episode.duration_seconds.unwrap_or(0.0),
            );
            sqlx::query("DELETE FROM chapters WHERE item_id = $1")
                .bind(episode_id)
                .execute(&mut *tx)
                .await?;
            for row in rows {
                sqlx::query(
                    "INSERT INTO chapters (item_id, position, title, start, \"end\")
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(episode_id)
                .bind(row.position)
                .bind(row.title)
                .bind(row.start)
                .bind(row.end)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok((new_episodes, updated_episodes))
}

/// The feed's artwork into the cover directory. Any failure just means no cover.
async fn store_feed_cover(
    db: &PgPool,
    http: &reqwest::Client,
    podcast_id: i32,
    image_url: &str,
    covers_dir: &Path,
) -> Option<String> {
    let url = reqwest::Url::parse(image_url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let mut response = http.get(url).timeout(FEED_TIMEOUT).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_COVER_BYTES {
            return None;
        }
    }
    if bytes.is_empty() {
        return None;
    }
    let dir = covers_dir.to_path_buf();
    let path = tokio::task::spawn_blocking(move || write_cover_bytes(&dir, podcast_id, &bytes))
        .await
        .ok()?
        .ok()?;
    let path = path.to_string_lossy().into_owned();
    sqlx::query("UPDATE items SET cover_path = $2 WHERE id = $1")
        .bind(podcast_id)
        .bind(&path)
        .execute(db)
        .await
        .ok()?;
    Some(path)
}

async fn finish_sync(
    db: &PgPool,
    http: &reqwest::Client,
    podcast: (i32, String, Option<String>, Option<String>),
    parsed: &ParsedFeed,
    covers_dir: Option<&Path>,
) -> Result<Synced, FeedError> {
    let (id, title, feed_url, mut cover_path) = podcast;
    let (new_episodes, updated_episodes) = sync_episodes(db, id, &parsed.episodes).await?;
    sqlx::query("UPDATE items SET last_checked = now(), description = $2 WHERE id = $1")
        .bind(id)
        .bind(&parsed.description)
        .execute(db)
        .await?;
    if let (Some(dir), None, Some(image)) = (covers_dir, &cover_path, &parsed.image_url) {
        cover_path = store_feed_cover(db, http, id, image, dir).await;
    }
    Ok(Synced {
        id,
        title,
        feed_url,
        cover_path,
        new_episodes,
        updated_episodes,
    })
}

/// Subscribes to `feed_url`; subscribing to the same URL again reuses the podcast.
pub async fn subscribe(
    db: &PgPool,
    http: &reqwest::Client,
    feed_url: &str,
    covers_dir: Option<&Path>,
) -> Result<Synced, FeedError> {
    let parsed = preview_feed(http, feed_url).await?;
    if parsed.episodes.is_empty() && parsed.title.is_empty() {
        return Err(FeedError::Invalid(
            "Feed enthält weder Titel noch Folgen".into(),
        ));
    }
    let existing: Option<(i32, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, title, feed_url, cover_path FROM items WHERE kind = 'podcast' AND feed_url = $1",
    )
    .bind(feed_url)
    .fetch_optional(db)
    .await?;
    let podcast =
        match existing {
            Some(row) => row,
            None => sqlx::query_as(
                "INSERT INTO items (kind, title, sort_title, description, feed_url, last_checked)
                 VALUES ('podcast', $1, lower($1), $2, $3, now())
                 RETURNING id, title, feed_url, cover_path",
            )
            .bind(&parsed.title)
            .bind(&parsed.description)
            .bind(feed_url)
            .fetch_one(db)
            .await?,
        };
    finish_sync(db, http, podcast, &parsed, covers_dir).await
}

/// Fetches the podcast's feed again and syncs its episodes.
pub async fn refresh(
    db: &PgPool,
    http: &reqwest::Client,
    podcast_id: i32,
    covers_dir: Option<&Path>,
) -> Result<Synced, FeedError> {
    let podcast: (i32, String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT id, title, feed_url, cover_path FROM items WHERE id = $1 AND kind = 'podcast'",
    )
    .bind(podcast_id)
    .fetch_optional(db)
    .await?
    .ok_or(FeedError::NotFound)?;
    let feed_url = podcast.2.clone().unwrap_or_default();
    let parsed = preview_feed(http, &feed_url).await?;
    finish_sync(db, http, podcast, &parsed, covers_dir).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsubscribeReport {
    pub episodes: i64,
    pub files_deleted: u32,
    pub files_kept: u32,
}

/// Deletes the podcast with its episodes, and the downloaded files below the podcasts directory.
/// A file anywhere else is left alone and counted as kept.
pub async fn unsubscribe(
    db: &PgPool,
    podcast_id: i32,
    podcasts_dir: &Path,
) -> Result<UnsubscribeReport, FeedError> {
    let episodes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM items WHERE parent_id = $1 AND kind = 'episode'")
            .bind(podcast_id)
            .fetch_one(db)
            .await?;
    let paths: Vec<String> = sqlx::query_scalar(
        "SELECT tracks.path FROM tracks JOIN items ON items.id = tracks.item_id
         WHERE items.parent_id = $1",
    )
    .bind(podcast_id)
    .fetch_all(db)
    .await?;
    let deleted = sqlx::query("DELETE FROM items WHERE id = $1 AND kind = 'podcast'")
        .bind(podcast_id)
        .execute(db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(FeedError::NotFound);
    }
    let mut report = UnsubscribeReport {
        episodes,
        files_deleted: 0,
        files_kept: 0,
    };
    for path in paths.iter().map(PathBuf::from) {
        if !is_inside(&path, podcasts_dir) {
            report.files_kept += 1;
            continue;
        }
        match tokio::fs::remove_file(&path).await {
            Ok(()) => report.files_deleted += 1,
            Err(_) => report.files_kept += 1,
        }
        prune_empty_podcast_dir(&path, podcasts_dir).await;
    }
    Ok(report)
}
