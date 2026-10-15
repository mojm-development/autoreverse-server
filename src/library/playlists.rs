//! A user's playlists and their entries (legacy `library/playlistEntries.ts`, the playlist
//! queries in `queries.ts` and the reads inside `routes/playlists/**`).
//!
//! Entry positions are gapless `1..=n` per playlist, guarded by the unique index on
//! `(playlist_id, position)`. Every change runs in a transaction that first locks the playlist
//! row, so two concurrent appends cannot both take `max + 1`. The index is not deferrable, so a
//! block of positions is shifted in two statements: first into the negative range, then back —
//! the same guarantee legacy got from updating one row at a time in position order.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use super::queries::{Item, Track};

/// One `playlists` row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Playlist {
    pub id: i32,
    pub user_id: i32,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// One `playlist_entries` row: either a whole item or a single track.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PlaylistEntry {
    pub id: i32,
    pub playlist_id: i32,
    pub item_id: Option<i32>,
    pub track_id: Option<i32>,
    pub position: i32,
}

/// What an entry points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryTarget {
    Item(i32),
    Track(i32),
}

/// The user's playlist with this id; `None` for an unknown one and for someone else's alike.
pub async fn owned_playlist(
    db: &PgPool,
    user_id: i32,
    id: i32,
) -> Result<Option<Playlist>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM playlists WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await
}

/// A playlist with its number of entries (`GET /playlists`).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PlaylistCount {
    pub id: i32,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub entry_count: i64,
}

/// The user's playlists by lower-cased name, with their entry counts.
pub async fn list_playlists(db: &PgPool, user_id: i32) -> Result<Vec<PlaylistCount>, sqlx::Error> {
    sqlx::query_as(
        "SELECT playlists.id, playlists.name, playlists.created_at,
                count(playlist_entries.id) AS entry_count
         FROM playlists
         LEFT JOIN playlist_entries ON playlist_entries.playlist_id = playlists.id
         WHERE playlists.user_id = $1
         GROUP BY playlists.id
         ORDER BY lower(playlists.name), playlists.id",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// A tile on the Playlists page: entry count and total length.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PlaylistOverview {
    pub id: i32,
    pub name: String,
    pub entry_count: i64,
    /// Seconds: a single-track entry counts its track, a whole-item entry all of the item's.
    pub duration: f64,
}

/// legacy `playlistOverview`.
pub async fn playlist_overview(
    db: &PgPool,
    user_id: i32,
) -> Result<Vec<PlaylistOverview>, sqlx::Error> {
    sqlx::query_as(
        "SELECT playlist.id, playlist.name,
                count(entry.id) AS entry_count,
                coalesce(sum(coalesce(single.duration, whole.duration, 0)), 0)::float8 AS duration
         FROM playlists AS playlist
         LEFT JOIN playlist_entries AS entry ON entry.playlist_id = playlist.id
         LEFT JOIN tracks AS single ON single.id = entry.track_id
         LEFT JOIN LATERAL (
             SELECT sum(tracks.duration) AS duration FROM tracks WHERE tracks.item_id = entry.item_id
         ) AS whole ON true
         WHERE playlist.user_id = $1
         GROUP BY playlist.id
         ORDER BY lower(playlist.name), playlist.id",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

pub async fn create_playlist(
    db: &PgPool,
    user_id: i32,
    name: &str,
) -> Result<Playlist, sqlx::Error> {
    sqlx::query_as("INSERT INTO playlists (user_id, name) VALUES ($1, $2) RETURNING *")
        .bind(user_id)
        .bind(name)
        .fetch_one(db)
        .await
}

pub async fn rename_playlist(db: &PgPool, id: i32, name: &str) -> Result<Playlist, sqlx::Error> {
    sqlx::query_as("UPDATE playlists SET name = $2 WHERE id = $1 RETURNING *")
        .bind(id)
        .bind(name)
        .fetch_one(db)
        .await
}

/// Deletes a playlist; its entries go with it (`ON DELETE CASCADE`).
pub async fn delete_playlist(db: &PgPool, id: i32) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM playlists WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn entry_count(db: &PgPool, playlist_id: i32) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM playlist_entries WHERE playlist_id = $1")
        .bind(playlist_id)
        .fetch_one(db)
        .await
}

/// One entry of a playlist; `None` if it belongs to another playlist.
pub async fn entry(
    db: &PgPool,
    playlist_id: i32,
    entry_id: i32,
) -> Result<Option<PlaylistEntry>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM playlist_entries WHERE id = $1 AND playlist_id = $2")
        .bind(entry_id)
        .bind(playlist_id)
        .fetch_optional(db)
        .await
}

pub async fn list_entries(
    db: &PgPool,
    playlist_id: i32,
) -> Result<Vec<PlaylistEntry>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM playlist_entries WHERE playlist_id = $1 ORDER BY position")
        .bind(playlist_id)
        .fetch_all(db)
        .await
}

async fn lock_playlist(
    tx: &mut Transaction<'_, Postgres>,
    playlist_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT id FROM playlists WHERE id = $1 FOR UPDATE")
        .bind(playlist_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Moves the entries of `playlist_id` with a position in `[from, to]` by `delta`, without ever
/// holding two rows at the same position: out to `-(position + delta)` first, then back.
async fn shift(
    tx: &mut Transaction<'_, Postgres>,
    playlist_id: i32,
    from: i32,
    to: i32,
    delta: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE playlist_entries SET position = -(position + $4)
         WHERE playlist_id = $1 AND position BETWEEN $2 AND $3",
    )
    .bind(playlist_id)
    .bind(from)
    .bind(to)
    .bind(delta)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE playlist_entries SET position = -position WHERE playlist_id = $1 AND position < 0",
    )
    .bind(playlist_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Appends an entry at the end (`max + 1`, 1 for an empty playlist).
pub async fn append_entry(
    db: &PgPool,
    playlist_id: i32,
    target: EntryTarget,
) -> Result<PlaylistEntry, sqlx::Error> {
    let (item_id, track_id) = match target {
        EntryTarget::Item(id) => (Some(id), None),
        EntryTarget::Track(id) => (None, Some(id)),
    };
    let mut tx = db.begin().await?;
    lock_playlist(&mut tx, playlist_id).await?;
    let entry = sqlx::query_as(
        "INSERT INTO playlist_entries (playlist_id, item_id, track_id, position)
         SELECT $1, $2, $3, coalesce(max(position), 0) + 1
         FROM playlist_entries WHERE playlist_id = $1
         RETURNING *",
    )
    .bind(playlist_id)
    .bind(item_id)
    .bind(track_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(entry)
}

/// Removes an entry and closes the gap behind it. False if it is not in this playlist (any
/// more). The position is read under the lock, not taken from an earlier read.
pub async fn remove_entry(
    db: &PgPool,
    playlist_id: i32,
    entry_id: i32,
) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    lock_playlist(&mut tx, playlist_id).await?;
    let removed: Option<i32> = sqlx::query_scalar(
        "DELETE FROM playlist_entries WHERE id = $1 AND playlist_id = $2 RETURNING position",
    )
    .bind(entry_id)
    .bind(playlist_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(position) = removed else {
        return Ok(false);
    };
    shift(&mut tx, playlist_id, position + 1, i32::MAX - 1, -1).await?;
    tx.commit().await?;
    Ok(true)
}

/// Moves an entry to `new_position` (1-based) and shifts the ones in between by one: moving
/// earlier pushes `[new, old)` down the list, moving later pulls `(old, new]` up. A position
/// past the end is clamped to it. False if the entry is not in this playlist.
pub async fn move_entry(
    db: &PgPool,
    playlist_id: i32,
    entry_id: i32,
    new_position: i32,
) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    lock_playlist(&mut tx, playlist_id).await?;
    let old: Option<i32> = sqlx::query_scalar(
        "SELECT position FROM playlist_entries WHERE id = $1 AND playlist_id = $2",
    )
    .bind(entry_id)
    .bind(playlist_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(old) = old else {
        return Ok(false);
    };
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM playlist_entries WHERE playlist_id = $1")
            .bind(playlist_id)
            .fetch_one(&mut *tx)
            .await?;
    let new = new_position.clamp(1, count.max(1) as i32);
    if new == old {
        return Ok(true);
    }
    // Park the moving entry at 0: positions are 1-based and the shifted block passes through
    // negatives only.
    sqlx::query("UPDATE playlist_entries SET position = 0 WHERE id = $1")
        .bind(entry_id)
        .execute(&mut *tx)
        .await?;
    if new < old {
        shift(&mut tx, playlist_id, new, old - 1, 1).await?;
    } else {
        shift(&mut tx, playlist_id, old + 1, new, -1).await?;
    }
    sqlx::query("UPDATE playlist_entries SET position = $2 WHERE id = $1")
        .bind(entry_id)
        .bind(new)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// An entry with what it points at: the item for a whole-item entry, the track and its item
/// for a single track. Both are `None` only if the row vanished in between.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEntry {
    pub entry: PlaylistEntry,
    pub item: Option<Item>,
    pub track: Option<(Track, Item)>,
}

/// Resolves entries with two queries instead of legacy's two per entry.
pub async fn resolve_entries(
    db: &PgPool,
    entries: Vec<PlaylistEntry>,
) -> Result<Vec<ResolvedEntry>, sqlx::Error> {
    let track_ids: Vec<i32> = entries.iter().filter_map(|e| e.track_id).collect();
    let tracks: HashMap<i32, Track> = if track_ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, Track>("SELECT * FROM tracks WHERE id = ANY($1)")
            .bind(&track_ids)
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|t| (t.id, t))
            .collect()
    };
    let mut item_ids: Vec<i32> = entries.iter().filter_map(|e| e.item_id).collect();
    item_ids.extend(tracks.values().map(|t| t.item_id));
    let items: HashMap<i32, Item> = if item_ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, Item>("SELECT * FROM items WHERE id = ANY($1)")
            .bind(&item_ids)
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|i| (i.id, i))
            .collect()
    };
    Ok(entries
        .into_iter()
        .map(|entry| {
            let item = entry.item_id.and_then(|id| items.get(&id).cloned());
            let track = entry.track_id.and_then(|id| {
                let track = tracks.get(&id)?.clone();
                let parent = items.get(&track.item_id)?.clone();
                Some((track, parent))
            });
            ResolvedEntry { entry, item, track }
        })
        .collect())
}
