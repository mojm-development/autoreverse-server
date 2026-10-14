//! Read queries over items and tracks (legacy `library/queries.ts`). Grows with each slice.

use std::collections::HashMap;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::like::like_pattern;
use super::sorting::{episode_number, natural_key};

/// One `items` row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Item {
    pub id: i32,
    pub parent_id: Option<i32>,
    pub kind: String,
    pub source_path: Option<String>,
    pub title: String,
    pub sort_title: String,
    pub cover_path: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i32>,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub description: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub feed_url: Option<String>,
    pub last_checked: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
    pub added_at: DateTime<Utc>,
    pub missing_since: Option<DateTime<Utc>>,
    pub guid: Option<String>,
    pub chapters_url: Option<String>,
    pub feed_duration: Option<f64>,
    pub keep_episodes: Option<i32>,
    pub locked_fields: Vec<String>,
}

/// One `tracks` row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Track {
    pub id: i32,
    pub item_id: i32,
    pub position: i32,
    pub path: String,
    pub duration: f64,
    pub title: Option<String>,
    pub disc: Option<i32>,
    pub mtime: Option<f64>,
    pub size: Option<i64>,
    pub locked_fields: Vec<String>,
}

/// One `chapters` row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Chapter {
    pub id: i32,
    pub item_id: i32,
    pub position: i32,
    pub title: String,
    pub start: f64,
    pub end: f64,
}

/// The orders a list of items can take. `Played` only means something per user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Title,
    Added,
    Series,
    Played,
}

impl SortKey {
    /// The order the books page offers them in.
    pub const BOOK_SORTS: [SortKey; 4] = [
        SortKey::Series,
        SortKey::Title,
        SortKey::Added,
        SortKey::Played,
    ];

    pub fn parse(raw: &str) -> Option<SortKey> {
        match raw {
            "title" => Some(SortKey::Title),
            "added" => Some(SortKey::Added),
            "series" => Some(SortKey::Series),
            "played" => Some(SortKey::Played),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SortKey::Title => "title",
            SortKey::Added => "added",
            SortKey::Series => "series",
            SortKey::Played => "played",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Title => "Titel A–Z",
            SortKey::Added => "Zuletzt dazu",
            SortKey::Series => "Serie & Folge",
            SortKey::Played => "Zuletzt gehört",
        }
    }

    fn order_by(self) -> String {
        let title = &*NATURAL_TITLE;
        match self {
            SortKey::Title => title.clone(),
            SortKey::Added => format!("items.added_at DESC, {title}"),
            // The scanner's own volume number wins; the number parsed out of a title stays as
            // the fallback for anything without one.
            SortKey::Series => format!(
                "lower(items.author) NULLS LAST, lower(items.series) NULLS LAST, \
                 items.series_index NULLS LAST, {} NULLS LAST, {title}",
                *EPISODE_NO
            ),
            // Only meaningful together with `played_by`; without the join every row has a NULL
            // timestamp and the order collapses to the title.
            SortKey::Played => format!("progress.updated_at DESC NULLS LAST, {title}"),
        }
    }
}

static NATURAL_TITLE: LazyLock<String> = LazyLock::new(|| natural_key("items.sort_title"));
static EPISODE_NO: LazyLock<String> = LazyLock::new(|| episode_number("items.title"));

/// Page size of the paged library lists.
pub const PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Default)]
pub struct ItemsFilter {
    pub kind: Option<String>,
    pub q: Option<String>,
    /// Exact album artist (`/items?artist=`).
    pub artist: Option<String>,
    pub missing: Option<bool>,
    pub sort: Option<SortKey>,
    pub limit: i64,
    pub offset: i64,
    /// Join this user's progress rows so `Played` has a timestamp to order by. A LEFT join on
    /// purpose: sorting by "last played" must not hide everything the user never started.
    pub played_by: Option<i32>,
}

/// Top-level items only (no episodes), with the kind / search / artist / missing filters.
fn push_item_conditions(
    query: &mut QueryBuilder<Postgres>,
    kind: Option<&str>,
    q: Option<&str>,
    artist: Option<&str>,
    missing: Option<bool>,
) {
    query.push(" WHERE items.parent_id IS NULL");
    if let Some(kind) = kind {
        query.push(" AND items.kind = ").push_bind(kind.to_owned());
    }
    if let Some(artist) = artist {
        query
            .push(" AND items.artist = ")
            .push_bind(artist.to_owned());
    }
    if let Some(q) = q.filter(|q| !q.is_empty()) {
        let pattern = like_pattern(q);
        query
            .push(" AND (items.title ILIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR items.author ILIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR items.artist ILIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }
    match missing {
        Some(true) => {
            query.push(" AND items.missing_since IS NOT NULL");
        }
        Some(false) => {
            query.push(" AND items.missing_since IS NULL");
        }
        None => {}
    }
}

pub async fn items(db: &PgPool, filter: &ItemsFilter) -> Result<Vec<Item>, sqlx::Error> {
    let mut query = QueryBuilder::new("SELECT items.* FROM items");
    if let Some(user_id) = filter.played_by {
        query
            .push(" LEFT JOIN progress ON progress.item_id = items.id AND progress.user_id = ")
            .push_bind(user_id);
    }
    push_item_conditions(
        &mut query,
        filter.kind.as_deref(),
        filter.q.as_deref(),
        filter.artist.as_deref(),
        filter.missing,
    );
    let mut sort = filter.sort.unwrap_or(SortKey::Title);
    if sort == SortKey::Played && filter.played_by.is_none() {
        sort = SortKey::Title;
    }
    query.push(" ORDER BY ").push(sort.order_by());
    query.push(" LIMIT ").push_bind(filter.limit);
    query.push(" OFFSET ").push_bind(filter.offset);
    query.build_query_as().fetch_all(db).await
}

/// Present top-level items of `kind` matching `q`.
pub async fn count_items(db: &PgPool, kind: &str, q: Option<&str>) -> Result<i64, sqlx::Error> {
    let mut query = QueryBuilder::new("SELECT count(*) FROM items");
    push_item_conditions(&mut query, Some(kind), q, None, Some(false));
    query.build_query_scalar().fetch_one(db).await
}

pub async fn item(db: &PgPool, id: i32) -> Result<Option<Item>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM items WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The artwork to show for an item. An episode rarely carries its own, so it falls back to its
/// podcast's.
pub async fn cover_path_for(db: &PgPool, row: &Item) -> Result<Option<String>, sqlx::Error> {
    if row.cover_path.is_some() {
        return Ok(row.cover_path.clone());
    }
    let Some(parent_id) = row.parent_id else {
        return Ok(None);
    };
    let parent: Option<Option<String>> =
        sqlx::query_scalar("SELECT cover_path FROM items WHERE id = $1")
            .bind(parent_id)
            .fetch_optional(db)
            .await?;
    Ok(parent.flatten())
}

pub async fn track(db: &PgPool, id: i32) -> Result<Option<Track>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM tracks WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The items below `parent_id` (a podcast's episodes), newest first.
pub async fn children(db: &PgPool, parent_id: i32) -> Result<Vec<Item>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM items WHERE parent_id = $1 ORDER BY published_at DESC NULLS LAST, id",
    )
    .bind(parent_id)
    .fetch_all(db)
    .await
}

pub async fn tracks(db: &PgPool, item_id: i32) -> Result<Vec<Track>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM tracks WHERE item_id = $1 ORDER BY position")
        .bind(item_id)
        .fetch_all(db)
        .await
}

pub async fn chapters(db: &PgPool, item_id: i32) -> Result<Vec<Chapter>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM chapters WHERE item_id = $1 ORDER BY position")
        .bind(item_id)
        .fetch_all(db)
        .await
}

/// Items whose folder is gone, optionally of one kind.
pub async fn count_missing(db: &PgPool, kind: Option<&str>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM items
         WHERE missing_since IS NOT NULL AND ($1::text IS NULL OR kind = $1)",
    )
    .bind(kind)
    .fetch_one(db)
    .await
}

pub async fn delete_missing(db: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM items WHERE missing_since IS NOT NULL")
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

/// Total track duration per item; items without tracks are absent.
pub async fn item_durations(
    db: &PgPool,
    item_ids: &[i32],
) -> Result<HashMap<i32, f64>, sqlx::Error> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i32, f64)> = sqlx::query_as(
        "SELECT item_id, coalesce(sum(duration), 0) FROM tracks
         WHERE item_id = ANY($1) GROUP BY item_id",
    )
    .bind(item_ids)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Duration per item, falling back to what the feed claims. The tracks of an episode only exist
/// once it was downloaded; until then its `<itunes:duration>` stands in.
pub async fn item_durations_with_feed_fallback(
    db: &PgPool,
    item_ids: &[i32],
) -> Result<HashMap<i32, f64>, sqlx::Error> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut durations = item_durations(db, item_ids).await?;
    let claimed: Vec<(i32, f64)> = sqlx::query_as(
        "SELECT id, feed_duration FROM items WHERE id = ANY($1) AND feed_duration IS NOT NULL",
    )
    .bind(item_ids)
    .fetch_all(db)
    .await?;
    for (id, duration) in claimed {
        durations.entry(id).or_insert(duration);
    }
    Ok(durations)
}

/// An album artist and how many present albums they have.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Artist {
    pub name: String,
    pub album_count: i32,
}

/// Every album artist with at least one present album, case-insensitively by name.
pub async fn artists(db: &PgPool) -> Result<Vec<Artist>, sqlx::Error> {
    sqlx::query_as(
        "SELECT artist AS name, count(*)::int AS album_count FROM items
         WHERE kind = 'album' AND artist IS NOT NULL AND missing_since IS NULL
         GROUP BY artist ORDER BY lower(artist), artist",
    )
    .fetch_all(db)
    .await
}

/// An artist's present albums, newest first, then by title.
pub async fn albums_of_artist(db: &PgPool, artist: &str) -> Result<Vec<Item>, sqlx::Error> {
    let sql = format!(
        "SELECT * FROM items
         WHERE kind = 'album' AND artist = $1 AND missing_since IS NULL
         ORDER BY year DESC NULLS LAST, {}",
        *NATURAL_TITLE
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(artist)
        .fetch_all(db)
        .await
}

/// A user's progress on one item.
#[derive(Debug, Clone, Copy, PartialEq, sqlx::FromRow)]
pub struct Progress {
    pub position: f64,
    pub finished: bool,
}

pub async fn progress(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
) -> Result<Option<Progress>, sqlx::Error> {
    sqlx::query_as("SELECT position, finished FROM progress WHERE user_id = $1 AND item_id = $2")
        .bind(user_id)
        .bind(item_id)
        .fetch_optional(db)
        .await
}

/// Progress plus the item's total duration, for many items at once.
#[derive(Debug, Clone, Copy, PartialEq, sqlx::FromRow)]
pub struct ProgressEntry {
    pub position: f64,
    pub finished: bool,
    pub duration: f64,
}

pub async fn progress_map(
    db: &PgPool,
    user_id: i32,
    item_ids: &[i32],
) -> Result<HashMap<i32, ProgressEntry>, sqlx::Error> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i32, f64, bool, f64)> = sqlx::query_as(
        "SELECT progress.item_id, progress.position, progress.finished,
                coalesce(sum(tracks.duration), 0)
         FROM progress LEFT JOIN tracks ON tracks.item_id = progress.item_id
         WHERE progress.user_id = $1 AND progress.item_id = ANY($2)
         GROUP BY progress.item_id, progress.position, progress.finished",
    )
    .bind(user_id)
    .bind(item_ids)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, position, finished, duration)| {
            (
                id,
                ProgressEntry {
                    position,
                    finished,
                    duration,
                },
            )
        })
        .collect())
}

/// The present books of a series in volume order. With an author the series is scoped to them:
/// two writers can use the same series name.
pub async fn series_siblings(
    db: &PgPool,
    series: &str,
    author: Option<&str>,
) -> Result<Vec<Item>, sqlx::Error> {
    let sql = format!(
        "SELECT * FROM items
         WHERE kind = 'book' AND series = $1 AND ($2::text IS NULL OR author = $2)
           AND missing_since IS NULL
         ORDER BY series_index NULLS LAST, {} NULLS LAST, {}",
        *EPISODE_NO, *NATURAL_TITLE
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(series)
        .bind(author)
        .fetch_all(db)
        .await
}

/// One shelf on the series page.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct BookSeries {
    pub series: String,
    pub author: Option<String>,
    pub count: i32,
    /// Ids of up to three volumes with artwork, in volume order.
    pub covers: Option<Vec<i32>>,
    pub finished_count: i32,
}

/// Series grouped per author (a series belongs to whoever wrote it), with cover ids for the first
/// three volumes and how many the user finished.
pub async fn book_series(db: &PgPool, user_id: i32) -> Result<Vec<BookSeries>, sqlx::Error> {
    sqlx::query_as(
        "SELECT item.series,
                item.author,
                count(*)::int AS count,
                (array_agg(item.id ORDER BY item.series_index NULLS LAST, item.sort_title)
                    FILTER (WHERE item.cover_path IS NOT NULL))[1:3] AS covers,
                count(*) FILTER (WHERE progress.finished)::int AS finished_count
         FROM items AS item
         LEFT JOIN progress ON progress.item_id = item.id AND progress.user_id = $1
         WHERE item.kind = 'book' AND item.series IS NOT NULL AND item.missing_since IS NULL
         GROUP BY item.series, item.author
         ORDER BY lower(item.author) NULLS LAST, lower(item.series)",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

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

/// The newest present top-level items (the start page's "Neu im Bestand").
pub async fn recently_added(db: &PgPool, limit: i64) -> Result<Vec<Item>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM items WHERE parent_id IS NULL AND missing_since IS NULL
         ORDER BY added_at DESC, id DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Present items of the given kinds whose title, author or artist contains `q` — episodes too,
/// not only top-level items (legacy `searchItems`).
pub async fn search_items(
    db: &PgPool,
    q: &str,
    kinds: &[&str],
    limit: i64,
) -> Result<Vec<Item>, sqlx::Error> {
    let sql = format!(
        "SELECT * FROM items
         WHERE kind = ANY($1) AND missing_since IS NULL
           AND (title ILIKE $2 ESCAPE '\\' OR author ILIKE $2 ESCAPE '\\'
                OR artist ILIKE $2 ESCAPE '\\')
         ORDER BY {} LIMIT $3",
        natural_key("sort_title")
    );
    let kinds: Vec<String> = kinds.iter().map(|k| (*k).to_owned()).collect();
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(kinds)
        .bind(like_pattern(q))
        .bind(limit)
        .fetch_all(db)
        .await
}

/// A track hit with the item it belongs to.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct TrackHit {
    pub id: i32,
    pub title: Option<String>,
    pub duration: f64,
    pub item_id: i32,
    pub item_title: String,
    pub item_kind: String,
}

/// Tracks of present items whose title contains `q`.
pub async fn search_tracks(db: &PgPool, q: &str, limit: i64) -> Result<Vec<TrackHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT track.id, track.title, track.duration, track.item_id,
                item.title AS item_title, item.kind AS item_kind
         FROM tracks AS track JOIN items AS item ON item.id = track.item_id
         WHERE track.title ILIKE $1 ESCAPE '\\' AND item.missing_since IS NULL
         ORDER BY lower(track.title) LIMIT $2",
    )
    .bind(like_pattern(q))
    .bind(limit)
    .fetch_all(db)
    .await
}

/// An album artist or book author matching a search, with how many present works they have.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct NameHit {
    pub name: String,
    /// `artist` or `author`.
    pub role: String,
    pub work_count: i32,
}

pub async fn search_artists(db: &PgPool, q: &str, limit: i64) -> Result<Vec<NameHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT name, role, count(*)::int AS work_count FROM (
             SELECT artist AS name, 'artist' AS role FROM items
             WHERE kind = 'album' AND artist IS NOT NULL AND missing_since IS NULL
               AND artist ILIKE $1 ESCAPE '\\'
             UNION ALL
             SELECT author AS name, 'author' AS role FROM items
             WHERE kind = 'book' AND author IS NOT NULL AND missing_since IS NULL
               AND author ILIKE $1 ESCAPE '\\'
         ) AS named
         GROUP BY name, role ORDER BY lower(name) LIMIT $2",
    )
    .bind(like_pattern(q))
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Every present item, episodes included (the search page's "in N Items").
pub async fn total_items(db: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM items WHERE missing_since IS NULL")
        .fetch_one(db)
        .await
}
