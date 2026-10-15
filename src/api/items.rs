//! `GET /items`, `GET|PATCH /items/{id}`, `GET /items/{id}/children`, `GET /items/{id}/cover`,
//! and the bulk side of metadata editing: `POST /items/bulk`, `POST /items/bulk/undo`,
//! `POST /items/series`, `DELETE /items/missing`.

use std::collections::HashMap;
use std::path::PathBuf;

use axum::extract::{Path as UrlPath, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use super::images::{cover_roots, serve_image};
use super::validate::{ApiJson, ApiQuery, QueryMap, int_param};
use crate::auth::{ApiAdmin, ApiUser};
use crate::error::{ApiError, ApiResult};
use crate::library::bulk_metadata::{BulkResult, UndoResult, bulk_edit, plan_series, undo_batch};
use crate::library::metadata::{ITEM_FIELDS, edit_item, parse_request};
use crate::library::queries::{self, Item, ItemsFilter, PAGE_SIZE, SortKey};
use crate::serialize::{ChapterOut, ItemDetail, ItemSummary, TrackOut};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/items", get(list))
        .route("/items/{id}", get(detail).patch(patch))
        // Static segments beat the `{id}` capture.
        .route("/items/bulk", post(bulk))
        .route("/items/bulk/undo", post(undo))
        .route("/items/series", post(series))
        .route("/items/missing", delete(remove_missing))
        .route("/items/{id}/children", get(children))
        .route("/items/{id}/cover", get(cover))
        .route("/library/more", get(library_more))
}

/// JS `Number.MAX_SAFE_INTEGER`, legacy's upper bound for `offset`.
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Serialize)]
struct ItemsOut {
    items: Vec<ItemSummary>,
}

/// `kind`, `q`, `artist` (exact; not in legacy), `missing=true|false`, `limit` (1–500, default 200), `offset`, and
/// `sort=title|added|series|played`. An unknown sort is refused rather than read as `title`: a
/// sort that silently does something else is a bug that takes long to notice.
async fn list(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<ItemsOut>> {
    let non_empty = |name: &str| query.get(name).filter(|v| !v.is_empty()).cloned();
    let limit = int_param(&query, "limit", 200, 1, 500)?;
    let offset = int_param(&query, "offset", 0, 0, MAX_SAFE_INTEGER)?;
    let sort = match query.get("sort") {
        None => SortKey::Title,
        Some(raw) => {
            SortKey::parse(raw).ok_or_else(|| ApiError::unprocessable("Unbekannte Sortierung"))?
        }
    };
    let rows = queries::items(
        &state.db,
        &ItemsFilter {
            kind: non_empty("kind"),
            q: non_empty("q"),
            artist: non_empty("artist"),
            missing: query.get("missing").map(|m| m == "true"),
            sort: Some(sort),
            limit,
            offset,
            // Only `played` needs the progress join.
            played_by: (sort == SortKey::Played).then_some(user.id),
        },
    )
    .await?;
    Ok(Json(ItemsOut {
        items: rows.iter().map(ItemSummary::from).collect(),
    }))
}

/// JS `Date.prototype.toJSON`: what legacy's raw Drizzle rows carried (milliseconds included).
fn js_date<S: serde::Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&d.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn js_date_opt<S: serde::Serializer>(d: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match d {
        Some(d) => js_date(d, s),
        None => s.serialize_none(),
    }
}

/// An `items` row as legacy's `/library/more` sent it: Drizzle's camelCase column names, every
/// column, `Date`s as JS serialises them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RawItem<'a> {
    id: i32,
    parent_id: Option<i32>,
    kind: &'a str,
    source_path: Option<&'a str>,
    title: &'a str,
    sort_title: &'a str,
    cover_path: Option<&'a str>,
    artist: Option<&'a str>,
    album_artist: Option<&'a str>,
    year: Option<i32>,
    author: Option<&'a str>,
    narrator: Option<&'a str>,
    description: Option<&'a str>,
    series: Option<&'a str>,
    series_index: Option<f64>,
    feed_url: Option<&'a str>,
    #[serde(serialize_with = "js_date_opt")]
    last_checked: Option<DateTime<Utc>>,
    #[serde(serialize_with = "js_date_opt")]
    published_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "js_date")]
    added_at: DateTime<Utc>,
    #[serde(serialize_with = "js_date_opt")]
    missing_since: Option<DateTime<Utc>>,
    guid: Option<&'a str>,
    chapters_url: Option<&'a str>,
    feed_duration: Option<f64>,
    keep_episodes: Option<i32>,
    locked_fields: &'a [String],
}

impl<'a> From<&'a Item> for RawItem<'a> {
    fn from(i: &'a Item) -> Self {
        RawItem {
            id: i.id,
            parent_id: i.parent_id,
            kind: &i.kind,
            source_path: i.source_path.as_deref(),
            title: &i.title,
            sort_title: &i.sort_title,
            cover_path: i.cover_path.as_deref(),
            artist: i.artist.as_deref(),
            album_artist: i.album_artist.as_deref(),
            year: i.year,
            author: i.author.as_deref(),
            narrator: i.narrator.as_deref(),
            description: i.description.as_deref(),
            series: i.series.as_deref(),
            series_index: i.series_index,
            feed_url: i.feed_url.as_deref(),
            last_checked: i.last_checked,
            published_at: i.published_at,
            added_at: i.added_at,
            missing_since: i.missing_since,
            guid: i.guid.as_deref(),
            chapters_url: i.chapters_url.as_deref(),
            feed_duration: i.feed_duration,
            keep_episodes: i.keep_episodes,
            locked_fields: &i.locked_fields,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryMoreOut<'a> {
    items: Vec<RawItem<'a>>,
    #[serde(serialize_with = "crate::serialize::js_number_map")]
    durations: HashMap<i32, f64>,
    has_more: bool,
}

/// `GET /library/more` (legacy `library/more/+server.ts`): one page of albums or books for the
/// old web UI's infinite scroll — the Rust web UI pages through `/library/albums/more` instead.
/// Kept for any client that still calls it; native clients page through `/items`. Unlike
/// `/items`, an unknown sort quietly reads as `title`, as in legacy.
async fn library_more(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    let kind = query.get("kind").map(String::as_str).unwrap_or_default();
    if !matches!(kind, "album" | "book") {
        return Err(ApiError::unprocessable("Unbekannte Art"));
    }
    let sort = query
        .get("sort")
        .and_then(|raw| SortKey::parse(raw))
        .filter(|s| SortKey::BOOK_SORTS.contains(s))
        .unwrap_or(SortKey::Title);
    let offset = int_param(&query, "offset", 0, 0, MAX_SAFE_INTEGER)?;
    let limit = int_param(&query, "limit", PAGE_SIZE, 1, PAGE_SIZE)?;
    let rows = queries::items(
        &state.db,
        &ItemsFilter {
            kind: Some(kind.to_owned()),
            q: query.get("q").filter(|v| !v.is_empty()).cloned(),
            artist: None,
            missing: Some(query.get("missing").is_some_and(|m| m == "true")),
            sort: Some(sort),
            limit,
            offset,
            played_by: (sort == SortKey::Played).then_some(user.id),
        },
    )
    .await?;
    let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let durations = queries::item_durations(&state.db, &ids).await?;
    let out = LibraryMoreOut {
        has_more: rows.len() as i64 == limit,
        items: rows.iter().map(RawItem::from).collect(),
        durations,
    };
    Ok(Json(out).into_response())
}

/// The item behind a path segment; anything that is not a known id is 404 "Unbekanntes Item".
async fn find_item(state: &AppState, raw_id: &str) -> ApiResult<Item> {
    let not_found = || ApiError::not_found("Unbekanntes Item");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    queries::item(&state.db, id).await?.ok_or_else(not_found)
}

#[derive(Serialize)]
struct ItemOut {
    #[serde(flatten)]
    item: ItemDetail,
    tracks: Vec<TrackOut>,
    chapters: Vec<ChapterOut>,
}

async fn detail(
    State(state): State<AppState>,
    _user: ApiUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<ItemOut>> {
    let row = find_item(&state, &id).await?;
    let (tracks, chapters) = tokio::try_join!(
        queries::tracks(&state.db, row.id),
        queries::chapters(&state.db, row.id)
    )?;
    Ok(Json(ItemOut {
        item: ItemDetail::from(&row),
        tracks: tracks.iter().map(TrackOut::from).collect(),
        chapters: chapters.iter().map(ChapterOut::from).collect(),
    }))
}

#[derive(Serialize)]
struct ChildrenOut {
    items: Vec<ItemSummary>,
    /// Seconds per child id; a child without tracks falls back to its feed's claim, else is absent.
    #[serde(serialize_with = "crate::serialize::js_number_map")]
    durations: HashMap<i32, f64>,
}

/// The items below one (a podcast's episodes), newest first, with their durations.
async fn children(
    State(state): State<AppState>,
    _user: ApiUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<ChildrenOut>> {
    let parent = find_item(&state, &id).await?;
    let rows = queries::children(&state.db, parent.id).await?;
    let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let durations = queries::item_durations_with_feed_fallback(&state.db, &ids).await?;
    Ok(Json(ChildrenOut {
        items: rows.iter().map(ItemSummary::from).collect(),
        durations,
    }))
}

/// The cover image. Only files under a library root or the server's cover directory are served,
/// whatever `cover_path` says.
async fn cover(
    State(state): State<AppState>,
    _user: ApiUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Response> {
    let row = find_item(&state, &id).await?;
    let cover_path = queries::cover_path_for(&state.db, &row)
        .await?
        .ok_or_else(|| ApiError::not_found("Kein Cover"))?;
    let roots = cover_roots(&state.db, &state.config).await?;
    serve_image(
        &PathBuf::from(cover_path),
        &roots,
        "Kein Cover",
        "private, max-age=86400",
    )
    .await
}

#[derive(Serialize)]
struct EditedItem {
    #[serde(flatten)]
    item: ItemDetail,
    locked_fields: Vec<String>,
    changed: Vec<String>,
    batch_id: String,
}

/// Hand-editing an item's metadata. Every field set here is locked against the scanner, which
/// otherwise rewrites it from the files on the next scan; `reset` hands a field back. Verwalter
/// only — this changes what everyone sees.
async fn patch(
    State(state): State<AppState>,
    admin: ApiAdmin,
    UrlPath(raw_id): UrlPath<String>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<EditedItem>> {
    let request = parse_request(ITEM_FIELDS, &body)?;
    let id: i32 = raw_id
        .parse()
        .map_err(|_| ApiError::not_found("Unbekanntes Item"))?;
    let result = edit_item(&state.db, admin.id, id, request).await?;
    Ok(Json(EditedItem {
        item: ItemDetail::from(&result.row),
        locked_fields: result.row.locked_fields.clone(),
        changed: result.changed,
        batch_id: result.batch_id,
    }))
}

/// Editing many items in one go. Defaults to a dry run: the answer is the diff the apply would
/// write, so nothing changes before someone has seen it. `dry_run: false` writes, and the
/// `batch_id` it answers with is what undoes it.
async fn bulk(
    State(state): State<AppState>,
    admin: ApiAdmin,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<BulkResult>> {
    Ok(Json(bulk_edit(&state.db, admin.id, &body).await?))
}

async fn undo(
    State(state): State<AppState>,
    _admin: ApiAdmin,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<UndoResult>> {
    let batch_id = body
        .get("batch_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ApiError::unprocessable("batch_id fehlt"))?;
    Ok(Json(undo_batch(&state.db, batch_id).await?))
}

/// Series and volume numbers for a selection of books: `detect` applies the scanner's own rules
/// on demand, `assign` numbers a chosen series through. Dry run by default, undoable by batch id.
async fn series(
    State(state): State<AppState>,
    admin: ApiAdmin,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<BulkResult>> {
    Ok(Json(plan_series(&state.db, admin.id, &body).await?))
}

/// Removes every item marked missing — not while a scan runs, which may just be finding them.
async fn remove_missing(State(state): State<AppState>, _admin: ApiAdmin) -> ApiResult<Json<Value>> {
    if state.scan.snapshot().running {
        return Err(ApiError::conflict("Es läuft gerade ein Scan"));
    }
    let removed = queries::delete_missing(&state.db).await?;
    Ok(Json(json!({ "removed": removed })))
}
