//! `GET|POST /bookmarks`, `DELETE /bookmarks/{id}` (legacy `routes/bookmarks/**`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing::delete, routing::get};
use serde::Serialize;
use serde_json::{Map, Value};

use super::validate::{ApiJson, ApiQuery, QueryMap};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::bookmarks::{
    Bookmark, add_bookmark, bookmarks_by_title, bookmarks_for_item, remove_bookmark,
};
use crate::library::queries;
use crate::serialize::js_number;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/bookmarks", get(list).post(create))
        .route("/bookmarks/{id}", delete(remove))
}

#[derive(Serialize)]
struct BookmarkOut {
    id: i32,
    item_id: i32,
    #[serde(serialize_with = "js_number")]
    position: f64,
    title: String,
}

impl From<Bookmark> for BookmarkOut {
    fn from(b: Bookmark) -> Self {
        BookmarkOut {
            id: b.id,
            item_id: b.item_id,
            position: b.position,
            title: b.title,
        }
    }
}

#[derive(Serialize)]
struct BookmarksOut {
    bookmarks: Vec<BookmarkOut>,
}

const ITEM_ID_NOT_A_NUMBER: &str = "item_id muss eine Zahl sein";

/// All of the user's bookmarks, or with `item_id` those in one item.
async fn list(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<BookmarksOut>> {
    let rows = match query.get("item_id").filter(|v| !v.is_empty()) {
        Some(raw) => {
            // Legacy ran `Number()` on it and failed with a 500 on anything else.
            let item_id: i32 = raw
                .trim()
                .parse()
                .map_err(|_| ApiError::unprocessable(ITEM_ID_NOT_A_NUMBER))?;
            bookmarks_for_item(&state.db, user.id, item_id).await?
        }
        None => bookmarks_by_title(&state.db, user.id).await?,
    };
    Ok(Json(BookmarksOut {
        bookmarks: rows.into_iter().map(BookmarkOut::from).collect(),
    }))
}

/// `{item_id, position, title}`. Setting a second bookmark at the same position renames it.
async fn create(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<(StatusCode, Json<BookmarkOut>)> {
    let item_id = body
        .get("item_id")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| ApiError::unprocessable(ITEM_ID_NOT_A_NUMBER))?;
    let position = body
        .get("position")
        .and_then(Value::as_f64)
        .filter(|p| *p >= 0.0)
        .ok_or_else(|| ApiError::unprocessable("position muss ≥ 0 sein"))?;
    // JS `length` counts UTF-16 code units; so does this, to accept exactly what legacy did.
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .filter(|t| (1..=200).contains(&t.encode_utf16().count()))
        .ok_or_else(|| ApiError::unprocessable("title muss 1–200 Zeichen haben"))?;
    let not_found = || ApiError::not_found("Unbekanntes Item");
    // A fractional or out-of-range id names no item.
    if item_id.fract() != 0.0 || item_id < i32::MIN as f64 || item_id > i32::MAX as f64 {
        return Err(not_found());
    }
    let row = queries::item(&state.db, item_id as i32)
        .await?
        .ok_or_else(not_found)?;
    let bookmark = add_bookmark(&state.db, user.id, row.id, position, title).await?;
    Ok((StatusCode::CREATED, Json(bookmark.into())))
}

/// 404 alike for an unknown bookmark and someone else's.
async fn remove(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let not_found = || ApiError::not_found("Unbekanntes Lesezeichen");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    if !remove_bookmark(&state.db, user.id, id).await? {
        return Err(not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
