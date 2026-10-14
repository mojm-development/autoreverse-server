//! Playback: `POST /play/{itemId}`, `PUT|POST /progress/{itemId}`, `GET /progress/continue`,
//! `POST /sessions/{sessionId}/close`, `GET /me/stats`, `GET|PUT /me/playback`.
//!
//! The iOS app lives on these routes; shapes, defaults and `detail` texts follow legacy exactly.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing::get, routing::post};
use serde::Serialize;
use serde_json::{Map, Value};

use super::validate::{ApiJson, ApiQuery, INVALID_JSON, JsonBody, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::auth::preferences::{
    Preferences, SKIP_MAX, SKIP_MIN, SPEED_MAX, SPEED_MIN, get_preferences, set_preferences,
};
use crate::error::{ApiError, ApiResult};
use crate::library::playback::{
    ListeningStats, close_session, continue_listening, listening_stats, open_session, save_position,
};
use crate::library::queries::{self, Item};
use crate::serialize::{ChapterOut, TrackOut, js_number};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/play/{item_id}", post(play))
        // POST too: `navigator.sendBeacon` on page unload can only POST.
        .route("/progress/{item_id}", post(put_progress).put(put_progress))
        .route("/progress/continue", get(continue_list))
        .route("/sessions/{session_id}/close", post(close))
        .route("/me/stats", get(stats))
        .route("/me/playback", get(get_playback).put(put_playback))
}

async fn find_item(state: &AppState, raw_id: &str) -> ApiResult<Item> {
    let not_found = || ApiError::not_found("Unbekanntes Item");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    queries::item(&state.db, id).await?.ok_or_else(not_found)
}

#[derive(Serialize)]
struct PlayOut {
    session_id: String,
    kind: String,
    has_cover: bool,
    #[serde(serialize_with = "js_number")]
    start_position: f64,
    tracks: Vec<TrackOut>,
    chapters: Vec<ChapterOut>,
}

/// Opens a playback session at the saved position and hands over everything a player needs.
async fn play(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<PlayOut>> {
    let row = find_item(&state, &raw_id).await?;
    let tracks = queries::tracks(&state.db, row.id).await?;
    if row.kind == "episode" && tracks.is_empty() {
        return Err(ApiError::conflict("Folge ist noch nicht heruntergeladen"));
    }
    let start_position = queries::progress(&state.db, user.id, row.id)
        .await?
        .map_or(0.0, |p| p.position);
    let session_id = open_session(&state.db, user.id, row.id, start_position).await?;
    let (chapters, cover) = tokio::try_join!(
        queries::chapters(&state.db, row.id),
        queries::cover_path_for(&state.db, &row)
    )?;
    Ok(Json(PlayOut {
        session_id,
        kind: row.kind,
        has_cover: cover.is_some(),
        start_position,
        tracks: tracks.iter().map(TrackOut::from).collect(),
        chapters: chapters.iter().map(ChapterOut::from).collect(),
    }))
}

/// A finite JSON number, or 422 "position muss eine Zahl sein".
fn position_of(body: &Map<String, Value>) -> ApiResult<f64> {
    body.get("position")
        .and_then(Value::as_f64)
        .filter(|p| p.is_finite())
        .ok_or_else(|| ApiError::unprocessable("position muss eine Zahl sein"))
}

/// `{position, finished?}`: stores the position; `finished` defaults to false, so a plain
/// position update also takes an item off the finished list.
async fn put_progress(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
    body: JsonBody,
) -> ApiResult<StatusCode> {
    let row = find_item(&state, &raw_id).await?;
    let body: Map<String, Value> = body.parse()?;
    let position = position_of(&body)?;
    let finished = match body.get("finished") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(ApiError::unprocessable(INVALID_JSON)),
    };
    save_position(&state.db, user.id, row.id, position, Some(finished)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct ContinueItem {
    id: i32,
    title: String,
    kind: String,
    #[serde(serialize_with = "js_number")]
    position: f64,
    /// Added after legacy's first version (commit d105277): without it a client fetches the
    /// whole item to draw a progress bar. Older clients ignore it.
    #[serde(serialize_with = "js_number")]
    duration: f64,
    has_cover: bool,
}

#[derive(Serialize)]
struct ContinueOut {
    items: Vec<ContinueItem>,
}

async fn continue_list(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<ContinueOut>> {
    let limit = int_param(&query, "limit", 20, 1, 100)?;
    let rows = continue_listening(&state.db, user.id, limit).await?;
    Ok(Json(ContinueOut {
        items: rows
            .into_iter()
            .map(|r| ContinueItem {
                id: r.id,
                title: r.title,
                kind: r.kind,
                position: r.position,
                duration: r.duration,
                has_cover: r.cover_path.is_some(),
            })
            .collect(),
    }))
}

/// `{position}`: closes the session, saves the position and books the listening time. Other
/// fields (the iOS app sends `duration`) are ignored.
async fn close(
    State(state): State<AppState>,
    user: ApiUser,
    Path(session_id): Path<String>,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<StatusCode> {
    let position = position_of(&body)?;
    close_session(&state.db, user.id, &session_id, position)
        .await?
        .ok_or_else(|| ApiError::not_found("Unbekannte Sitzung"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn stats(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<ListeningStats>> {
    let days = int_param(&query, "days", 30, 1, 365)?;
    Ok(Json(
        listening_stats(&state.db, user.id, days as i32).await?,
    ))
}

async fn get_playback(
    State(state): State<AppState>,
    user: ApiUser,
) -> ApiResult<Json<Preferences>> {
    Ok(Json(get_preferences(&state.db, user.id).await?))
}

/// One preference out of the body: the default when absent, else a number within the bounds.
fn preference(
    body: &Map<String, Value>,
    name: &str,
    default: f64,
    min: f64,
    max: f64,
) -> ApiResult<f64> {
    let Some(value) = body.get(name) else {
        return Ok(default);
    };
    value
        .as_f64()
        .filter(|v| (min..=max).contains(v))
        .ok_or_else(|| ApiError::unprocessable(format!("{name} außerhalb des gültigen Bereichs")))
}

/// Replaces all three preferences; an absent field goes back to its default, as in legacy.
/// Answers with what was stored (skips are cut to whole seconds).
async fn put_playback(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Json<Preferences>> {
    let defaults = Preferences::default();
    let (skip_min, skip_max) = (SKIP_MIN as f64, SKIP_MAX as f64);
    let speed = preference(
        &body,
        "playback_speed",
        defaults.playback_speed,
        SPEED_MIN,
        SPEED_MAX,
    )?;
    let back = preference(
        &body,
        "skip_back",
        defaults.skip_back as f64,
        skip_min,
        skip_max,
    )?;
    let forward = preference(
        &body,
        "skip_forward",
        defaults.skip_forward as f64,
        skip_min,
        skip_max,
    )?;
    Ok(Json(
        set_preferences(&state.db, user.id, speed, back, forward).await?,
    ))
}
