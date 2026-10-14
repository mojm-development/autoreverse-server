//! `GET /items`, `GET|PATCH /items/{id}`, `GET /items/{id}/children`, `GET /items/{id}/cover`,
//! and the bulk side of metadata editing: `POST /items/bulk`, `POST /items/bulk/undo`,
//! `POST /items/series`, `DELETE /items/missing`.

use std::collections::HashMap;
use std::path::PathBuf;

use axum::extract::{Path as UrlPath, State};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{Value, json};

use super::images::{cover_roots, serve_image};
use super::validate::{ApiJson, ApiQuery, QueryMap, int_param};
use crate::auth::{ApiAdmin, ApiUser};
use crate::error::{ApiError, ApiResult};
use crate::library::bulk_metadata::{BulkResult, UndoResult, bulk_edit, plan_series, undo_batch};
use crate::library::metadata::{ITEM_FIELDS, edit_item, parse_request};
use crate::library::queries::{self, Item, ItemsFilter, SortKey};
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
