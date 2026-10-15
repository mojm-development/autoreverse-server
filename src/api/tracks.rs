//! `GET|PATCH /tracks/{id}` and `GET|HEAD /tracks/{id}/stream`.

use std::path::Path as FsPath;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use axum::{Json, Router, routing::get};
use serde::Serialize;
use serde_json::Value;

use super::validate::ApiJson;
use crate::auth::{ApiAdmin, ApiUser};
use crate::error::{ApiError, ApiResult};
use crate::library::metadata::{TRACK_FIELDS, edit_track, parse_request};
use crate::library::queries;
use crate::serialize::{TrackDetail, TrackOut};
use crate::state::AppState;
use crate::streaming::ranges::range_response;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tracks/{id}", get(detail).patch(patch))
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

#[derive(Serialize)]
struct EditedTrack {
    #[serde(flatten)]
    track: TrackOut,
    disc: Option<i32>,
    locked_fields: Vec<String>,
    changed: Vec<String>,
    batch_id: String,
}

/// Correcting a track's own metadata — its title, mostly, which is what a badly tagged rip gets
/// wrong. Locked against the scanner like an item's fields.
async fn patch(
    State(state): State<AppState>,
    admin: ApiAdmin,
    Path(raw_id): Path<String>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<EditedTrack>> {
    let request = parse_request(TRACK_FIELDS, &body)?;
    let id: i32 = raw_id
        .parse()
        .map_err(|_| ApiError::not_found("Unbekannter Titel"))?;
    let result = edit_track(&state.db, admin.id, id, request).await?;
    Ok(Json(EditedTrack {
        track: TrackOut::from(&result.row),
        disc: result.row.disc,
        locked_fields: result.row.locked_fields.clone(),
        changed: result.changed,
        batch_id: result.batch_id,
    }))
}
