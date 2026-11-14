//! `GET /favorites`, `POST|DELETE /favorites/items/{id}`, `POST|DELETE /favorites/tracks/{id}`.
//!
//! Adding twice and removing what is not there are both no-ops with 204, as in legacy.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing::get, routing::post};
use serde::Serialize;

use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::favorites::{
    add_item_favorite, add_track_favorite, list_favorite_items, list_favorite_tracks,
    remove_item_favorite, remove_track_favorite,
};
use crate::library::queries;
use crate::serialize::{ItemSummary, TrackSummary};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/favorites", get(list))
        .route("/favorites/items/{id}", post(add_item).delete(remove_item))
        .route(
            "/favorites/tracks/{id}",
            post(add_track).delete(remove_track),
        )
}

#[derive(Serialize)]
struct FavoritesOut {
    items: Vec<ItemSummary>,
    tracks: Vec<TrackSummary>,
}

async fn list(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<FavoritesOut>> {
    let (items, tracks) = tokio::try_join!(
        list_favorite_items(&state.db, user.id),
        list_favorite_tracks(&state.db, user.id)
    )?;
    Ok(Json(FavoritesOut {
        items: items.iter().map(ItemSummary::from).collect(),
        tracks: tracks
            .iter()
            .map(|f| TrackSummary::new(&f.track, &f.item))
            .collect(),
    }))
}

async fn add_item(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let not_found = || ApiError::not_found("Unbekanntes Item");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let row = queries::item(&state.db, id).await?.ok_or_else(not_found)?;
    add_item_favorite(&state.db, user.id, row.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// An id that cannot exist has nothing to remove: 204 like any other non-favourite.
async fn remove_item(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    if let Ok(id) = raw_id.parse::<i32>() {
        remove_item_favorite(&state.db, user.id, id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn add_track(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let not_found = || ApiError::not_found("Unbekannter Track");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let row = queries::track(&state.db, id).await?.ok_or_else(not_found)?;
    add_track_favorite(&state.db, user.id, row.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_track(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    if let Ok(id) = raw_id.parse::<i32>() {
        remove_track_favorite(&state.db, user.id, id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
