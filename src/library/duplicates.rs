//! Albums that exist more than once: the same artist and title in different folders. One copy
//! stays, the others are hidden (`items.duplicate_of`), never deleted — the files are untouched
//! and a hidden copy can be shown again.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

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

/// What [`transfer_to_kept`] moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Transferred {
    pub progress: u64,
    pub favorites: u64,
}

/// Moves progress and favorites from every hidden copy to the album kept instead, so hiding a
/// copy loses nobody's place or heart. Idempotent: what was moved is gone from the copy.
///
/// - Progress: the newer one wins where a user has some on both.
/// - Favorite albums: moved; one already on the kept album stays as it is.
/// - Favorite tracks: moved to the kept album's track with the same title, or the same position
///   where either title is missing. A track without a match keeps its favorite on the copy,
///   which stays playable by id.
pub async fn transfer_to_kept(db: &mut PgConnection) -> Result<Transferred, sqlx::Error> {
    // DISTINCT ON: two hidden copies of one album must not both update the same row.
    sqlx::query(
        "INSERT INTO progress (user_id, item_id, position, finished, updated_at)
         SELECT DISTINCT ON (progress.user_id, copy.duplicate_of)
                progress.user_id, copy.duplicate_of, progress.position, progress.finished,
                progress.updated_at
         FROM progress JOIN items AS copy ON copy.id = progress.item_id
         WHERE copy.duplicate_of IS NOT NULL
         ORDER BY progress.user_id, copy.duplicate_of, progress.updated_at DESC
         ON CONFLICT (user_id, item_id) DO UPDATE
             SET position = excluded.position, finished = excluded.finished,
                 updated_at = excluded.updated_at
             WHERE excluded.updated_at > progress.updated_at",
    )
    .execute(&mut *db)
    .await?;
    let moved_progress = sqlx::query(
        "DELETE FROM progress USING items AS copy
         WHERE copy.id = progress.item_id AND copy.duplicate_of IS NOT NULL",
    )
    .execute(&mut *db)
    .await?;

    sqlx::query(
        "INSERT INTO favorites (user_id, item_id)
         SELECT DISTINCT favorites.user_id, copy.duplicate_of
         FROM favorites JOIN items AS copy ON copy.id = favorites.item_id
         WHERE copy.duplicate_of IS NOT NULL
         ON CONFLICT (user_id, item_id) WHERE item_id IS NOT NULL DO NOTHING",
    )
    .execute(&mut *db)
    .await?;
    let albums = sqlx::query(
        "DELETE FROM favorites USING items AS copy
         WHERE copy.id = favorites.item_id AND copy.duplicate_of IS NOT NULL",
    )
    .execute(&mut *db)
    .await?;

    let tracks = sqlx::query(
        "WITH moves AS (
             SELECT favorites.user_id, track.id AS old_id, target.id AS new_id
             FROM favorites
             JOIN tracks AS track ON track.id = favorites.track_id
             JOIN items AS copy ON copy.id = track.item_id
             CROSS JOIN LATERAL (
                 SELECT kept.id FROM tracks AS kept
                 WHERE kept.item_id = copy.duplicate_of
                   AND (lower(btrim(kept.title)) = lower(btrim(track.title))
                        OR ((kept.title IS NULL OR track.title IS NULL)
                            AND kept.position = track.position))
                 ORDER BY (lower(btrim(kept.title)) = lower(btrim(track.title))) IS TRUE DESC,
                          kept.position = track.position DESC
                 LIMIT 1
             ) AS target
             WHERE copy.duplicate_of IS NOT NULL
         ),
         added AS (
             INSERT INTO favorites (user_id, track_id)
             SELECT DISTINCT user_id, new_id FROM moves
             ON CONFLICT (user_id, track_id) WHERE track_id IS NOT NULL DO NOTHING
         )
         DELETE FROM favorites USING moves
         WHERE favorites.user_id = moves.user_id AND favorites.track_id = moves.old_id",
    )
    .execute(&mut *db)
    .await?;

    Ok(Transferred {
        progress: moved_progress.rows_affected(),
        favorites: albums.rows_affected() + tracks.rows_affected(),
    })
}

/// [`transfer_to_kept`] in its own transaction, for copies hidden before the transfer existed.
/// Run at every start; with nothing left on hidden copies it changes nothing.
pub async fn transfer_all(db: &PgPool) -> Result<Transferred, sqlx::Error> {
    let mut tx = db.begin().await?;
    let moved = transfer_to_kept(&mut tx).await?;
    tx.commit().await?;
    Ok(moved)
}

/// Keeps album `id` and hides every other copy of it, moving their progress and favorites over
/// ([`transfer_to_kept`]). `None` if `id` is no album, else how many copies are hidden now.
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
    transfer_to_kept(&mut tx).await?;
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
