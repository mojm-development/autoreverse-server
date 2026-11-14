//! A user's playlists and their entries (legacy `library/playlistEntries.ts`, the playlist
//! queries in `queries.ts` and the reads inside `routes/playlists/**`).
//!
//! New in Rust: a playlist can be `shared` with the other accounts of the server (they see and
//! play it) and `collaborative` on top (they change its entries too). Who may do what is decided
//! in one place, the three methods of [`Playlist`]; API and web both ask them.
//!
//! Entry positions are gapless `1..=n` per playlist, guarded by the unique index on
//! `(playlist_id, position)`. Every change runs in a transaction that first locks the playlist
//! row, so two concurrent appends cannot both take `max + 1`. The index is not deferrable, so a
//! block of positions is shifted in two statements: first into the negative range, then back —
//! the same guarantee legacy got from updating one row at a time in position order.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use super::artist_covers::{IMAGE_TYPES, MAX_IMAGE_BYTES, TOO_LARGE};
use super::queries::{Item, Track};
use crate::error::{ApiError, ApiResult};

/// One `playlists` row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Playlist {
    pub id: i32,
    pub user_id: i32,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub description: Option<String>,
    /// Playlists with the same folder name form a folder; there is nothing else to a folder.
    pub folder: Option<String>,
    /// The uploaded cover's file under `Config::playlists_dir`.
    pub cover_path: Option<String>,
    pub shared: bool,
    /// Stored as chosen; it only takes effect while `shared` is set ([`Playlist::editable_by`]).
    pub collaborative: bool,
}

impl Playlist {
    /// Name, description, folder, sharing, cover and deleting are the owner's alone.
    pub fn is_owner(&self, user_id: i32) -> bool {
        self.user_id == user_id
    }

    /// Whether this account may see and play it.
    pub fn visible_to(&self, user_id: i32) -> bool {
        self.is_owner(user_id) || self.shared
    }

    /// Whether this account may add, remove and move entries.
    pub fn editable_by(&self, user_id: i32) -> bool {
        self.is_owner(user_id) || (self.shared && self.collaborative)
    }
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

/// The playlist with this id if the user may see it: their own, or one shared on this server.
/// `None` for an unknown one and for someone else's private one alike, so that nobody learns
/// that the latter exists.
pub async fn visible_playlist(
    db: &PgPool,
    user_id: i32,
    id: i32,
) -> Result<Option<Playlist>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM playlists WHERE id = $1 AND (user_id = $2 OR shared)")
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await
}

/// Which playlists a list is about (`GET /playlists?scope=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The user's own — what the list was before sharing, and still is without a parameter.
    Mine,
    /// Shared ones of other accounts.
    Shared,
    /// Own first, then the shared ones of others.
    All,
}

impl Scope {
    pub fn parse(raw: &str) -> Option<Scope> {
        match raw {
            "mine" => Some(Scope::Mine),
            "shared" => Some(Scope::Shared),
            "all" => Some(Scope::All),
            _ => None,
        }
    }
}

/// A playlist as the clients see it: the row, its owner's account name, and what its entries add
/// up to.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PlaylistSummary {
    #[sqlx(flatten)]
    pub playlist: Playlist,
    pub owner: String,
    pub entry_count: i64,
    /// Seconds: a single-track entry counts its track, a whole-item entry all of the item's —
    /// or, for an episode not downloaded yet, what its feed claims.
    pub duration: f64,
}

/// Everything before the `WHERE` of a summary query. `$1` is the asking user in each of them.
const SUMMARY_SELECT: &str = "SELECT playlist.*, owner.name AS owner,
            count(entry.id) AS entry_count,
            coalesce(sum(coalesce(single.duration, whole.duration, 0)), 0)::float8 AS duration
     FROM playlists AS playlist
     JOIN users AS owner ON owner.id = playlist.user_id
     LEFT JOIN playlist_entries AS entry ON entry.playlist_id = playlist.id
     LEFT JOIN tracks AS single ON single.id = entry.track_id
     LEFT JOIN LATERAL (
         SELECT coalesce(
             (SELECT sum(tracks.duration) FROM tracks WHERE tracks.item_id = entry.item_id),
             (SELECT items.feed_duration FROM items WHERE items.id = entry.item_id)
         ) AS duration
     ) AS whole ON true";

/// The playlists of a scope with their sums, in one statement: own ones first, each group by
/// lower-cased name.
pub async fn playlist_summaries(
    db: &PgPool,
    user_id: i32,
    scope: Scope,
) -> Result<Vec<PlaylistSummary>, sqlx::Error> {
    let filter = match scope {
        Scope::Mine => "playlist.user_id = $1",
        Scope::Shared => "playlist.shared AND playlist.user_id <> $1",
        Scope::All => "(playlist.user_id = $1 OR playlist.shared)",
    };
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SUMMARY_SELECT}
         WHERE {filter}
         GROUP BY playlist.id, owner.name
         ORDER BY (playlist.user_id <> $1), lower(playlist.name), playlist.id"
    )))
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// One playlist's summary, whoever asks: the caller has checked the access.
pub async fn playlist_summary(
    db: &PgPool,
    id: i32,
) -> Result<Option<PlaylistSummary>, sqlx::Error> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SUMMARY_SELECT}
         WHERE playlist.id = $1
         GROUP BY playlist.id, owner.name"
    )))
    .bind(id)
    .fetch_optional(db)
    .await
}

/// The folder names the user's playlists use, for the "Ordner" field's suggestions.
pub async fn folders(db: &PgPool, user_id: i32) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT folder FROM playlists
         WHERE user_id = $1 AND folder IS NOT NULL ORDER BY folder",
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

/// What the owner changes about a playlist; `None` leaves a field as it is. For the two texts
/// the inner `None` clears them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistChanges {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub folder: Option<Option<String>>,
    pub shared: Option<bool>,
    pub collaborative: Option<bool>,
}

impl PlaylistChanges {
    pub fn is_empty(&self) -> bool {
        *self == PlaylistChanges::default()
    }
}

/// Applies the changes in one statement, so that two of them never meet half-way.
pub async fn update_playlist(
    db: &PgPool,
    id: i32,
    changes: &PlaylistChanges,
) -> Result<Playlist, sqlx::Error> {
    sqlx::query_as(
        "UPDATE playlists SET
             name = coalesce($2, name),
             description = CASE WHEN $3 THEN $4 ELSE description END,
             folder = CASE WHEN $5 THEN $6 ELSE folder END,
             shared = coalesce($7, shared),
             collaborative = coalesce($8, collaborative)
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(changes.name.as_deref())
    .bind(changes.description.is_some())
    .bind(changes.description.clone().flatten())
    .bind(changes.folder.is_some())
    .bind(changes.folder.clone().flatten())
    .bind(changes.shared)
    .bind(changes.collaborative)
    .fetch_one(db)
    .await
}

/// Deletes a playlist; its entries go with it (`ON DELETE CASCADE`), and so does its cover.
/// (The covers of the playlists that go with a deleted account: `auth::directory::delete_user`.)
pub async fn delete_playlist(db: &PgPool, id: i32) -> Result<(), sqlx::Error> {
    let cover: Option<Option<String>> =
        sqlx::query_scalar("DELETE FROM playlists WHERE id = $1 RETURNING cover_path")
            .bind(id)
            .fetch_optional(db)
            .await?;
    if let Some(path) = cover.flatten() {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

/// The media type of an upload: what the client declared, or — where it declared nothing useful,
/// as a bare request body often does — what the first bytes say.
fn image_extension(content_type: &str, bytes: &[u8]) -> Option<&'static str> {
    let declared = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let media = match declared.as_str() {
        "" | "application/octet-stream" => match image::guess_format(bytes).ok()? {
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::WebP => "image/webp",
            image::ImageFormat::Gif => "image/gif",
            _ => return None,
        },
        other => other,
    };
    IMAGE_TYPES
        .iter()
        .find(|(known, _)| *known == media)
        .map(|(_, extension)| *extension)
}

/// Stores an uploaded cover under `playlists_dir` and points the playlist at it; the same types
/// and the same limit as an artist's picture. `size` is the upload's size, which may be more
/// than `bytes` holds when reading stopped at the limit.
pub async fn store_cover(
    db: &PgPool,
    playlist: &Playlist,
    content_type: &str,
    size: usize,
    bytes: &[u8],
    playlists_dir: &Path,
) -> ApiResult<PathBuf> {
    if size > MAX_IMAGE_BYTES {
        return Err(ApiError::new(
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            TOO_LARGE,
        ));
    }
    if size == 0 {
        return Err(ApiError::unprocessable("Leere Datei"));
    }
    let extension = image_extension(content_type, bytes)
        .ok_or_else(|| ApiError::unprocessable("Nicht unterstütztes Bildformat"))?;
    tokio::fs::create_dir_all(playlists_dir)
        .await
        .map_err(ApiError::internal)?;
    // Named by the id: nothing a user typed ever becomes a file name.
    let destination = playlists_dir.join(format!("{}{extension}", playlist.id));
    tokio::fs::write(&destination, bytes)
        .await
        .map_err(ApiError::internal)?;
    let stored = destination.to_string_lossy().into_owned();
    sqlx::query("UPDATE playlists SET cover_path = $2 WHERE id = $1")
        .bind(playlist.id)
        .bind(&stored)
        .execute(db)
        .await?;
    // A cover of another type lived under another name.
    if let Some(old) = playlist.cover_path.as_deref()
        && old != stored
    {
        let _ = tokio::fs::remove_file(old).await;
    }
    Ok(destination)
}

/// Back to no cover of its own; the file is deleted.
pub async fn clear_cover(db: &PgPool, playlist: &Playlist) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE playlists SET cover_path = NULL WHERE id = $1")
        .bind(playlist.id)
        .execute(db)
        .await?;
    if let Some(path) = playlist.cover_path.as_deref() {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

/// A track of a playlist as an export names it. `path` is the stored, absolute one — whoever
/// sends it out makes it relative first ([`super::paths::relative_to_roots`]).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ExportTrack {
    pub title: String,
    pub artist: Option<String>,
    pub album: String,
    pub duration: f64,
    pub path: String,
}

/// The playlist's tracks in playing order, one statement: an entry that is a whole item (an
/// album, a book, an episode) stands for all of its tracks, in their order.
pub async fn export_tracks(db: &PgPool, playlist_id: i32) -> Result<Vec<ExportTrack>, sqlx::Error> {
    sqlx::query_as(
        "SELECT coalesce(track.title, item.title) AS title,
                coalesce(track.artist, item.artist, item.album_artist, item.author, show.title)
                    AS artist,
                CASE WHEN item.kind = 'episode' THEN coalesce(show.title, item.title)
                     ELSE item.title END AS album,
                track.duration, track.path
         FROM playlist_entries AS entry
         JOIN tracks AS track
           ON track.id = entry.track_id OR track.item_id = entry.item_id
         JOIN items AS item ON item.id = track.item_id
         LEFT JOIN items AS show ON show.id = item.parent_id
         WHERE entry.playlist_id = $1
         ORDER BY entry.position, track.position",
    )
    .bind(playlist_id)
    .fetch_all(db)
    .await
}

/// A new playlist made of these tracks, in this order, doubles included. One transaction and
/// two statements however long the list is.
pub async fn create_with_tracks(
    db: &PgPool,
    user_id: i32,
    name: &str,
    folder: Option<&str>,
    track_ids: &[i32],
) -> Result<Playlist, sqlx::Error> {
    let mut tx = db.begin().await?;
    let playlist: Playlist = sqlx::query_as(
        "INSERT INTO playlists (user_id, name, folder) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(user_id)
    .bind(name)
    .bind(folder)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO playlist_entries (playlist_id, track_id, position)
         SELECT $1, listed.track_id, listed.position
         FROM unnest($2::int4[]) WITH ORDINALITY AS listed (track_id, position)",
    )
    .bind(playlist.id)
    .bind(track_ids)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(playlist)
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
