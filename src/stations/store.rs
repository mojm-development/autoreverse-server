//! The `radio_stations` table: each user's own list of stations.
//!
//! Positions are gapless `1..=n` per user, guarded by the unique index on `(user_id, position)`
//! — the same scheme as playlist entries (`library::playlists`): every change runs in a
//! transaction that first locks the user's list, and a block of positions is shifted through the
//! negative range so that no two rows ever share a position.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

/// More than anybody listens to; it bounds what one account can make the server store.
pub const MAX_STATIONS: i64 = 200;

/// One `radio_stations` row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Station {
    pub id: i32,
    pub user_id: i32,
    pub position: i32,
    pub name: String,
    pub source_url: String,
    pub stream_url: String,
    pub content_type: Option<String>,
    pub directory_id: Option<String>,
    pub homepage: Option<String>,
    pub country: Option<String>,
    pub tags: Vec<String>,
    pub codec: Option<String>,
    pub bitrate: Option<i32>,
    pub logo_file: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// What a new station is made of; position and id come from the table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewStation {
    pub name: String,
    pub source_url: String,
    pub stream_url: String,
    pub content_type: Option<String>,
    pub directory_id: Option<String>,
    pub homepage: Option<String>,
    pub country: Option<String>,
    pub tags: Vec<String>,
    pub codec: Option<String>,
    pub bitrate: Option<i32>,
}

/// The user's stations in their own order.
pub async fn list(db: &PgPool, user_id: i32) -> Result<Vec<Station>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM radio_stations WHERE user_id = $1 ORDER BY position")
        .bind(user_id)
        .fetch_all(db)
        .await
}

/// The user's station with this id; `None` for an unknown one and for someone else's alike.
pub async fn owned(db: &PgPool, user_id: i32, id: i32) -> Result<Option<Station>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM radio_stations WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await
}

/// The station the user already has for this directory id.
pub async fn by_directory_id(
    db: &PgPool,
    user_id: i32,
    directory_id: &str,
) -> Result<Option<Station>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM radio_stations WHERE user_id = $1 AND directory_id = $2")
        .bind(user_id)
        .bind(directory_id)
        .fetch_optional(db)
        .await
}

/// The station the user already has for this address: as it was entered, or as what it led to.
pub async fn by_url(db: &PgPool, user_id: i32, url: &str) -> Result<Option<Station>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM radio_stations WHERE user_id = $1 AND (source_url = $2 OR stream_url = $2)
         ORDER BY position LIMIT 1",
    )
    .bind(user_id)
    .bind(url)
    .fetch_optional(db)
    .await
}

/// The directory ids among the user's stations, for the `added` flag of a search.
pub async fn directory_ids(db: &PgPool, user_id: i32) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT directory_id FROM radio_stations
         WHERE user_id = $1 AND directory_id IS NOT NULL",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Serialises every change to one user's list. `FOR NO KEY UPDATE`: the row itself is not
/// changed, and logging in or playing (which reference the user) must not wait for this.
async fn lock_list(tx: &mut Transaction<'_, Postgres>, user_id: i32) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Moves the user's stations with a position in `[from, to]` by `delta`: out to
/// `-(position + delta)` first, then back.
async fn shift(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i32,
    from: i32,
    to: i32,
    delta: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE radio_stations SET position = -(position + $4)
         WHERE user_id = $1 AND position BETWEEN $2 AND $3",
    )
    .bind(user_id)
    .bind(from)
    .bind(to)
    .bind(delta)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE radio_stations SET position = -position WHERE user_id = $1 AND position < 0",
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// How adding a station ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Added {
    Created(Station),
    /// The user has it already (same directory id or same address); nothing was changed.
    Existing(Station),
    /// The list is full ([`MAX_STATIONS`]).
    Full,
}

/// Appends a station to the user's list. The check for a double runs under the list's lock, so
/// two requests for the same station at once end as one row and one "have it already".
pub async fn add(db: &PgPool, user_id: i32, new: &NewStation) -> Result<Added, sqlx::Error> {
    let mut tx = db.begin().await?;
    lock_list(&mut tx, user_id).await?;
    let existing: Option<Station> = sqlx::query_as(
        "SELECT * FROM radio_stations
         WHERE user_id = $1
           AND (stream_url IN ($2, $3) OR source_url IN ($2, $3)
                OR ($4::text IS NOT NULL AND directory_id = $4))
         ORDER BY position LIMIT 1",
    )
    .bind(user_id)
    .bind(&new.stream_url)
    .bind(&new.source_url)
    .bind(&new.directory_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(station) = existing {
        return Ok(Added::Existing(station));
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM radio_stations WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_STATIONS {
        return Ok(Added::Full);
    }
    let station = sqlx::query_as(
        "INSERT INTO radio_stations (user_id, position, name, source_url, stream_url,
             content_type, directory_id, homepage, country, tags, codec, bitrate)
         SELECT $1, coalesce(max(position), 0) + 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11
         FROM radio_stations WHERE user_id = $1
         RETURNING *",
    )
    .bind(user_id)
    .bind(&new.name)
    .bind(&new.source_url)
    .bind(&new.stream_url)
    .bind(&new.content_type)
    .bind(&new.directory_id)
    .bind(&new.homepage)
    .bind(&new.country)
    .bind(&new.tags)
    .bind(&new.codec)
    .bind(new.bitrate)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Added::Created(station))
}

/// Records the stored logo's file name and returns the row as it is now.
pub async fn set_logo(db: &PgPool, id: i32, file: &str) -> Result<Option<Station>, sqlx::Error> {
    sqlx::query_as("UPDATE radio_stations SET logo_file = $2 WHERE id = $1 RETURNING *")
        .bind(id)
        .bind(file)
        .fetch_optional(db)
        .await
}

pub async fn rename(db: &PgPool, id: i32, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE radio_stations SET name = $2 WHERE id = $1")
        .bind(id)
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

/// Moves a station to `new_position` (1-based) and shifts the ones in between by one. A
/// position past the end is clamped to it. False if the user has no such station (any more).
pub async fn move_to(
    db: &PgPool,
    user_id: i32,
    id: i32,
    new_position: i32,
) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    lock_list(&mut tx, user_id).await?;
    let old: Option<i32> =
        sqlx::query_scalar("SELECT position FROM radio_stations WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(old) = old else {
        return Ok(false);
    };
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM radio_stations WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    let new = new_position.clamp(1, count.max(1) as i32);
    if new == old {
        return Ok(true);
    }
    // Park the moving station at 0: positions are 1-based and the shifted block passes through
    // negatives only.
    sqlx::query("UPDATE radio_stations SET position = 0 WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if new < old {
        shift(&mut tx, user_id, new, old - 1, 1).await?;
    } else {
        shift(&mut tx, user_id, old + 1, new, -1).await?;
    }
    sqlx::query("UPDATE radio_stations SET position = $2 WHERE id = $1")
        .bind(id)
        .bind(new)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Removes a station and closes the gap behind it. The removed row, so the caller can delete
/// its logo; `None` if the user has no such station.
pub async fn remove(db: &PgPool, user_id: i32, id: i32) -> Result<Option<Station>, sqlx::Error> {
    let mut tx = db.begin().await?;
    lock_list(&mut tx, user_id).await?;
    let removed: Option<Station> =
        sqlx::query_as("DELETE FROM radio_stations WHERE id = $1 AND user_id = $2 RETURNING *")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(station) = removed else {
        return Ok(None);
    };
    shift(&mut tx, user_id, station.position + 1, i32::MAX - 1, -1).await?;
    tx.commit().await?;
    Ok(Some(station))
}
