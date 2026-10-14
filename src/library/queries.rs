//! Read queries over items and tracks (legacy `library/queries.ts`). Grows with each slice.

use serde::Serialize;
use sqlx::PgPool;

/// Top-level, present items per kind, plus present tracks (`libraryCounts`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LibraryCounts {
    pub book_count: i64,
    pub album_count: i64,
    pub podcast_count: i64,
    pub track_count: i64,
}

pub async fn library_counts(db: &PgPool) -> Result<LibraryCounts, sqlx::Error> {
    let (book_count, album_count, podcast_count, track_count) = sqlx::query_as(
        "SELECT
             count(*) FILTER (WHERE kind = 'book'),
             count(*) FILTER (WHERE kind = 'album'),
             count(*) FILTER (WHERE kind = 'podcast'),
             (SELECT count(*) FROM tracks JOIN items AS owner ON owner.id = tracks.item_id
              WHERE owner.missing_since IS NULL)
         FROM items
         WHERE parent_id IS NULL AND missing_since IS NULL",
    )
    .fetch_one(db)
    .await?;
    Ok(LibraryCounts {
        book_count,
        album_count,
        podcast_count,
        track_count,
    })
}

/// Distinct album artists with at least one present album (`artists(db).length`).
pub async fn artist_count(db: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(DISTINCT artist) FROM items
         WHERE kind = 'album' AND artist IS NOT NULL AND missing_since IS NULL",
    )
    .fetch_one(db)
    .await
}

/// Episodes published since their podcast was subscribed that `user_id` has not finished — the
/// sum of `unheard_count` over `podcastOverview`. Older back-catalogue episodes do not count.
pub async fn unheard_episode_count(db: &PgPool, user_id: i32) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM items AS podcast
         JOIN items AS episode ON episode.parent_id = podcast.id
         LEFT JOIN progress ON progress.item_id = episode.id AND progress.user_id = $1
         WHERE podcast.kind = 'podcast'
           AND (progress.item_id IS NULL OR progress.finished = false)
           AND episode.published_at >= podcast.added_at",
    )
    .bind(user_id)
    .fetch_one(db)
    .await
}
