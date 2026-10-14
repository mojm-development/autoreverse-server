//! `GET /items`, `GET /items/{id}`, `GET /items/{id}/children`, `GET /items/{id}/cover`.

use std::collections::HashMap;
use std::path::PathBuf;

use axum::extract::{Path as UrlPath, State};
use axum::response::Response;
use axum::{Json, Router, routing::get};
use serde::Serialize;

use super::images::{cover_roots, serve_image};
use super::validate::{ApiQuery, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::queries::{self, Item, ItemsFilter, SortKey};
use crate::serialize::{ChapterOut, ItemDetail, ItemSummary, TrackOut};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/items", get(list))
        .route("/items/{id}", get(detail))
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
