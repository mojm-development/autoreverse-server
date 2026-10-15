//! A user's bookmarks (legacy `library/bookmarks.ts`).

use std::collections::HashMap;
use std::sync::LazyLock;

use sqlx::PgPool;

use super::queries::Item;
use super::sorting::natural_key;

/// One `bookmarks` row, without the owner.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Bookmark {
    pub id: i32,
    pub item_id: i32,
    pub position: f64,
    pub title: String,
}

/// Sets a bookmark. One per position and item: setting the same spot again renames it.
pub async fn add_bookmark(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
    position: f64,
    title: &str,
) -> Result<Bookmark, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO bookmarks (user_id, item_id, position, title) VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, item_id, position) DO UPDATE SET title = EXCLUDED.title
         RETURNING id, item_id, position, title",
    )
    .bind(user_id)
    .bind(item_id)
    .bind(position)
    .bind(title)
    .fetch_one(db)
    .await
}

/// A user's bookmarks in one item, in playback order.
pub async fn bookmarks_for_item(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
) -> Result<Vec<Bookmark>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, item_id, position, title FROM bookmarks
         WHERE user_id = $1 AND item_id = $2 ORDER BY position",
    )
    .bind(user_id)
    .bind(item_id)
    .fetch_all(db)
    .await
}

/// All of a user's bookmarks for `GET /bookmarks`: by item title (plain lower-case, as legacy's
/// route did), then position.
pub async fn bookmarks_by_title(db: &PgPool, user_id: i32) -> Result<Vec<Bookmark>, sqlx::Error> {
    sqlx::query_as(
        "SELECT bookmarks.id, bookmarks.item_id, bookmarks.position, bookmarks.title
         FROM bookmarks JOIN items ON items.id = bookmarks.item_id
         WHERE bookmarks.user_id = $1
         ORDER BY lower(items.sort_title), bookmarks.position",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Deletes one of the user's bookmarks; false when there is none with that id for them.
pub async fn remove_bookmark(
    db: &PgPool,
    user_id: i32,
    bookmark_id: i32,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM bookmarks WHERE id = $1 AND user_id = $2")
        .bind(bookmark_id)
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn count_bookmarks(db: &PgPool, user_id: i32) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM bookmarks WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await
}

/// A bookmark with the item it belongs to (the Lesezeichen page).
#[derive(Debug, Clone, PartialEq)]
pub struct BookmarkWithItem {
    pub bookmark: Bookmark,
    pub item: Item,
}

static NATURAL_TITLE: LazyLock<String> = LazyLock::new(|| natural_key("items.sort_title"));

/// Every bookmark of a user with its item, naturally by item title, then position.
pub async fn all_bookmarks(
    db: &PgPool,
    user_id: i32,
) -> Result<Vec<BookmarkWithItem>, sqlx::Error> {
    let sql = format!(
        "SELECT bookmarks.id, bookmarks.item_id, bookmarks.position, bookmarks.title
         FROM bookmarks JOIN items ON items.id = bookmarks.item_id
         WHERE bookmarks.user_id = $1
         ORDER BY {}, bookmarks.position",
        *NATURAL_TITLE
    );
    let marks: Vec<Bookmark> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(db)
        .await?;
    let ids: Vec<i32> = marks.iter().map(|b| b.item_id).collect();
    let items: HashMap<i32, Item> =
        sqlx::query_as::<_, Item>("SELECT * FROM items WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(db)
            .await?
            .into_iter()
            .map(|item| (item.id, item))
            .collect();
    Ok(marks
        .into_iter()
        .filter_map(|bookmark| {
            let item = items.get(&bookmark.item_id)?.clone();
            Some(BookmarkWithItem { bookmark, item })
        })
        .collect())
}
