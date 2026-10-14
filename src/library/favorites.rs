//! A user's favourite items and tracks (legacy `library/favorites.ts`). A `favorites` row names
//! either an item or a track, never both; the partial unique indexes make adding idempotent.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use sqlx::PgPool;

use super::queries::{Item, Track};
use super::sorting::natural_key;

pub async fn add_item_favorite(db: &PgPool, user_id: i32, item_id: i32) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO favorites (user_id, item_id) VALUES ($1, $2)
         ON CONFLICT (user_id, item_id) WHERE item_id IS NOT NULL DO NOTHING",
    )
    .bind(user_id)
    .bind(item_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn remove_item_favorite(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM favorites WHERE user_id = $1 AND item_id = $2")
        .bind(user_id)
        .bind(item_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn add_track_favorite(
    db: &PgPool,
    user_id: i32,
    track_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO favorites (user_id, track_id) VALUES ($1, $2)
         ON CONFLICT (user_id, track_id) WHERE track_id IS NOT NULL DO NOTHING",
    )
    .bind(user_id)
    .bind(track_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn remove_track_favorite(
    db: &PgPool,
    user_id: i32,
    track_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM favorites WHERE user_id = $1 AND track_id = $2")
        .bind(user_id)
        .bind(track_id)
        .execute(db)
        .await?;
    Ok(())
}

static ITEM_TITLE: LazyLock<String> = LazyLock::new(|| natural_key("items.sort_title"));

/// Favourite items that are still present, naturally by title.
pub async fn list_favorite_items(db: &PgPool, user_id: i32) -> Result<Vec<Item>, sqlx::Error> {
    let sql = format!(
        "SELECT items.* FROM favorites JOIN items ON items.id = favorites.item_id
         WHERE favorites.user_id = $1 AND favorites.item_id IS NOT NULL
           AND items.missing_since IS NULL
         ORDER BY {}",
        *ITEM_TITLE
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(db)
        .await
}

/// A favourite track with the item it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct FavoriteTrack {
    pub track: Track,
    pub item: Item,
}

/// Favourite tracks of present items, by item title, then track order.
pub async fn list_favorite_tracks(
    db: &PgPool,
    user_id: i32,
) -> Result<Vec<FavoriteTrack>, sqlx::Error> {
    let sql = format!(
        "SELECT tracks.* FROM favorites
         JOIN tracks ON tracks.id = favorites.track_id
         JOIN items ON items.id = tracks.item_id
         WHERE favorites.user_id = $1 AND favorites.track_id IS NOT NULL
           AND items.missing_since IS NULL
         ORDER BY {}, tracks.position",
        *ITEM_TITLE
    );
    let tracks: Vec<Track> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(db)
        .await?;
    let ids: Vec<i32> = tracks.iter().map(|t| t.item_id).collect();
    let items: HashMap<i32, Item> =
        sqlx::query_as::<_, Item>("SELECT * FROM items WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|item| (item.id, item))
            .collect();
    Ok(tracks
        .into_iter()
        .filter_map(|track| {
            let item = items.get(&track.item_id)?.clone();
            Some(FavoriteTrack { track, item })
        })
        .collect())
}

pub async fn is_item_favorite(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM favorites WHERE user_id = $1 AND item_id = $2)",
    )
    .bind(user_id)
    .bind(item_id)
    .fetch_one(db)
    .await
}

/// Which of an item's tracks the user marked (the hearts on the album page).
pub async fn favorite_track_ids(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
) -> Result<HashSet<i32>, sqlx::Error> {
    let ids: Vec<i32> = sqlx::query_scalar(
        "SELECT favorites.track_id FROM favorites JOIN tracks ON tracks.id = favorites.track_id
         WHERE favorites.user_id = $1 AND tracks.item_id = $2",
    )
    .bind(user_id)
    .bind(item_id)
    .fetch_all(db)
    .await?;
    Ok(ids.into_iter().collect())
}
