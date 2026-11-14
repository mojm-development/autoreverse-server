//! `GET|PATCH /tracks/{id}`, `GET|HEAD /tracks/{id}/stream` and `GET /tracks/{id}/lyrics`.

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
use crate::library::lyrics::Lyrics;
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
        .route("/tracks/{id}/lyrics", get(lyrics))
}

pub const NO_LYRICS: &str = "Kein Songtext vorhanden";

/// The track's lyrics out of the collection itself — a `.lrc`/`.txt` beside the file, else its
/// tags (`library::lyrics`). Read from the files at this moment, so a text added a minute ago
/// is there without a scan; `has_lyrics` in the track lists only catches up with the next one,
/// and is put right here already when the two disagree.
async fn lyrics(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<Lyrics>> {
    let row = find_track(&state, &raw_id).await?;
    let path = std::path::PathBuf::from(&row.path);
    let (present, found) =
        tokio::task::spawn_blocking(move || (path.is_file(), crate::library::lyrics::read(&path)))
            .await
            .map_err(|e| ApiError::internal(format!("lyrics task failed: {e}")))?;
    // Not while the file cannot be seen (a share that is not mounted): that says nothing.
    if present && found.is_some() != row.has_lyrics {
        sqlx::query("UPDATE tracks SET has_lyrics = $2 WHERE id = $1")
            .bind(row.id)
            .bind(found.is_some())
            .execute(&state.db)
            .await?;
    }
    found
        .map(Json)
        .ok_or_else(|| ApiError::not_found(NO_LYRICS))
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
