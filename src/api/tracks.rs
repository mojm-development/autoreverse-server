//! `GET /tracks/{id}` and `GET|HEAD /tracks/{id}/stream`. `PATCH` comes with metadata editing
//! (slice 8).

use std::path::Path as FsPath;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use axum::{Json, Router, routing::get};

use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::queries;
use crate::serialize::TrackDetail;
use crate::state::AppState;
use crate::streaming::ranges::range_response;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tracks/{id}", get(detail))
        // axum answers HEAD from the GET route; the handler skips opening the file for it.
        .route("/tracks/{id}/stream", get(stream))
}

async fn find_track(state: &AppState, raw_id: &str) -> ApiResult<queries::Track> {
    let not_found = || ApiError::not_found("Unbekannter Track");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    queries::track(&state.db, id).await?.ok_or_else(not_found)
}

/// The audio file with HTTP range support. Works with the session cookie as well as a bearer
/// token, so the web player's `<audio src>` needs no extra auth.
async fn stream(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(raw_id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let row = find_track(&state, &raw_id).await?;
    let path = FsPath::new(&row.path);
    let gone = || ApiError::new(StatusCode::GONE, "Datei nicht mehr vorhanden");
    if !tokio::fs::metadata(path).await.is_ok_and(|m| m.is_file()) {
        return Err(gone());
    }
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    range_response(path, range, method == Method::HEAD)
        .await
        .map_err(|e| {
            tracing::warn!("streaming {}: {e}", row.path);
            gone()
        })
}

async fn detail(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<TrackDetail>> {
    let row = find_track(&state, &raw_id).await?;
    Ok(Json(TrackDetail::from(&row)))
}
