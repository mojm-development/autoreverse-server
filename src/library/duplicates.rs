//! Albums that exist more than once: the same artist and title in different folders. One copy
//! stays, the others are hidden (`items.duplicate_of`), never deleted — the files are untouched
//! and a hidden copy can be shown again.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// What makes two albums the same: artist and title, case and runs of whitespace ignored.
/// Editions stay apart on purpose — "Achtung Baby (Deluxe Edition)" is not "Achtung Baby".
const KEY: &str = "lower(regexp_replace(btrim(artist), '\\s+', ' ', 'g')) || chr(31) || \
                   lower(regexp_replace(btrim(title), '\\s+', ' ', 'g'))";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Copy {
    pub id: i32,
    pub title: String,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub source_path: Option<String>,
    pub has_cover: bool,
    pub added_at: DateTime<Utc>,
    pub duplicate_of: Option<i32>,
    pub track_count: i32,
    pub duration: f64,
    key: String,
}

/// The copies of one album, the recommended one first: most tracks, then the longest, then the
/// one with a cover, then the oldest.
#[derive(Debug, Clone)]
pub struct Group {
    pub copies: Vec<Copy>,
}

impl Group {
    /// The copy the list suggests keeping.
    pub fn recommended(&self) -> &Copy {
        &self.copies[0]
    }

    /// Still to decide: more than one copy is visible.
    pub fn is_open(&self) -> bool {
        self.copies
            .iter()
            .filter(|c| c.duplicate_of.is_none())
            .count()
            > 1
    }
}

/// Every album that exists more than once, grouped. Missing albums do not count.
pub async fn groups(db: &PgPool) -> Result<Vec<Group>, sqlx::Error> {
    let sql = format!(
        "WITH albums AS (
             SELECT item.id, item.title, item.artist, item.year, item.source_path,
                    item.cover_path IS NOT NULL AS has_cover, item.added_at, item.duplicate_of,
                    {KEY} AS key,
                    (SELECT count(*) FROM tracks WHERE tracks.item_id = item.id)::int
                        AS track_count,
                    (SELECT coalesce(sum(duration), 0) FROM tracks
                     WHERE tracks.item_id = item.id) AS duration
             FROM items AS item
             WHERE item.kind = 'album' AND item.artist IS NOT NULL
               AND item.missing_since IS NULL
         )
         SELECT * FROM albums
         WHERE key IN (SELECT key FROM albums GROUP BY key HAVING count(*) > 1)
         ORDER BY lower(artist), lower(title), key,
                  track_count DESC, duration DESC, has_cover DESC, id"
    );
    let rows: Vec<Copy> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(db)
        .await?;
    let mut groups: Vec<Group> = Vec::new();
    for row in rows {
        match groups.last_mut() {
            Some(group) if group.copies[0].key == row.key => group.copies.push(row),
            _ => groups.push(Group { copies: vec![row] }),
        }
    }
    Ok(groups)
}

/// Keeps album `id` and hides every other copy of it. `None` if `id` is no album, else how
/// many copies are hidden now.
pub async fn keep(db: &PgPool, id: i32) -> Result<Option<u64>, sqlx::Error> {
    let mut tx = db.begin().await?;
    let found =
        sqlx::query("UPDATE items SET duplicate_of = NULL WHERE id = $1 AND kind = 'album'")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    if found.rows_affected() == 0 {
        return Ok(None);
    }
    let sql = format!(
        "UPDATE items SET duplicate_of = $1
         WHERE kind = 'album' AND artist IS NOT NULL AND id <> $1
           AND {KEY} = (SELECT {KEY} FROM items WHERE id = $1)"
    );
    let hidden = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(hidden.rows_affected()))
}

/// Keeps the recommended copy of every open group. Returns how many groups were decided.
pub async fn keep_all_recommended(db: &PgPool) -> Result<usize, sqlx::Error> {
    let open: Vec<i32> = groups(db)
        .await?
        .iter()
        .filter(|g| g.is_open())
        .map(|g| g.recommended().id)
        .collect();
    for &id in &open {
        keep(db, id).await?;
    }
    Ok(open.len())
}

/// Shows a hidden copy again. Whether it was hidden.
pub async fn unhide(db: &PgPool, id: i32) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE items SET duplicate_of = NULL WHERE id = $1 AND duplicate_of IS NOT NULL",
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}
