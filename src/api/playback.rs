//! Playback: `POST /play/{itemId}`, `PUT|POST /progress/{itemId}`, `GET /progress/continue`,
//! `POST /sessions/{sessionId}/close`, `GET /me/stats`, `GET /me/history`, `POST /me/listens`,
//! `GET|PUT /me/playback`.
//!
//! The iOS app lives on these routes; shapes, defaults and `detail` texts follow legacy exactly.
//! `listened` on closing a session, closing without a `position`, `/me/listens` and `/me/history`
//! are newer than legacy and purely additive.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing::get, routing::post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};

use super::validate::{ApiJson, ApiQuery, INVALID_JSON, JsonBody, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::auth::preferences::{
    Preferences, SKIP_MAX, SKIP_MIN, SPEED_MAX, SPEED_MIN, get_preferences, set_preferences,
};
use crate::error::{ApiError, ApiResult};
use crate::library::playback::{
    ListenOutcome, ListenReport, ListeningStats, TrackShare, book_listen, close_session,
    continue_listening, listening_history, listening_stats, open_session, save_position,
};
use crate::library::queries::{self, Item};
use crate::serialize::{ChapterOut, HistoryEntry, ItemSummary, TrackOut, js_number, js_number_opt};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/play/{item_id}", post(play))
        // POST too: `navigator.sendBeacon` on page unload can only POST.
        .route("/progress/{item_id}", post(put_progress).put(put_progress))
        .route("/progress/continue", get(continue_list))
        .route("/sessions/{session_id}/close", post(close))
        .route("/me/stats", get(stats))
        .route("/me/history", get(history))
        .route("/me/listens", post(report_listens))
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
    // A finished item starts over: its saved position is the very end, where the player would
    // stop again at once (legacy handed that position out unchanged).
    let start_position = queries::progress(&state.db, user.id, row.id)
        .await?
        .filter(|p| !p.finished)
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

/// A number the body may leave out: absent or `null` is none, anything but a finite number is
/// 422 "`name` muss eine Zahl sein".
fn optional_number(body: &Map<String, Value>, name: &str) -> ApiResult<Option<f64>> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| ApiError::unprocessable(format!("{name} muss eine Zahl sein"))),
    }
}

/// `{position?, listened?}`: closes the session, saves the position and books the listening time
/// — `listened` (media seconds the client measured) when it is there, else the distance from the
/// session's start to `position`, as clients from before the field expect. Other fields (the iOS
/// app sends `duration`) are ignored.
///
/// A `position` that is `null` or absent closes the session without touching the item's saved
/// progress: what a client sends after a single track out of a playlist, a mix or the radio, so
/// the track's album neither turns up under "Weiter hören" nor loses the place it was left at.
/// `listened` is still booked; without either, nothing is. (Before this, a missing position was
/// a 422 — no client could have relied on that to close a session.)
///
/// `tracks: [{track_id, seconds}]` divides the time among the item's tracks, so that the
/// statistics know which songs were heard (`library::playback::divide_session` has the rule for
/// how it combines with `listened`). Absent, `null` or empty: as before.
async fn close(
    State(state): State<AppState>,
    user: ApiUser,
    Path(session_id): Path<String>,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<StatusCode> {
    let position = optional_number(&body, "position")?;
    let listened = optional_number(&body, "listened")?;
    let shares = track_shares(&body)?;
    close_session(&state.db, user.id, &session_id, position, listened, &shares)
        .await?
        .ok_or_else(|| ApiError::not_found("Unbekannte Sitzung"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// No item has more tracks than this, and a share per track is all a session can have.
const MAX_TRACK_SHARES: usize = 5000;

/// The `tracks` of a session's close. Anything but a list is a 422; inside the list an entry
/// that is not `{track_id: whole number, seconds: positive number}` is dropped like one for a
/// track of another item — a client's one bad entry must not cost the session its time.
fn track_shares(body: &Map<String, Value>) -> ApiResult<Vec<TrackShare>> {
    let entries = match body.get("tracks") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(entries)) => entries,
        Some(_) => return Err(ApiError::unprocessable("tracks muss eine Liste sein")),
    };
    if entries.len() > MAX_TRACK_SHARES {
        return Err(ApiError::unprocessable(format!(
            "Höchstens {MAX_TRACK_SHARES} Einträge in tracks"
        )));
    }
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let track_id = entry
                .get("track_id")
                .and_then(Value::as_i64)
                .and_then(|id| i32::try_from(id).ok())?;
            let seconds = entry
                .get("seconds")
                .and_then(Value::as_f64)
                .filter(|s| s.is_finite() && *s > 0.0)?;
            Some(TrackShare { track_id, seconds })
        })
        .collect())
}

/// More would be a client replaying its whole history in one request.
const MAX_LISTENS_PER_REQUEST: usize = 500;
const MAX_KEY_CHARS: usize = 100;

/// The answer for one entry of `POST /me/listens`, in the order the entries came.
#[derive(Serialize)]
struct ListenResult {
    /// The entry's key, `null` where it had no usable one.
    key: Option<String>,
    status: &'static str,
    /// What was booked, which may be fewer seconds than reported.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "js_number_opt"
    )]
    seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'static str>,
}

#[derive(Serialize)]
struct ListensOut {
    results: Vec<ListenResult>,
}

/// An optional id: absent or `null` is none, anything but a whole number in range is the error.
fn optional_id(
    entry: &Value,
    name: &str,
    error: &'static str,
) -> Result<Option<i32>, &'static str> {
    match entry.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .and_then(|id| i32::try_from(id).ok())
            .map(Some)
            .ok_or(error),
    }
}

fn listen_key(entry: &Value) -> Option<String> {
    entry
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.trim().is_empty() && key.chars().count() <= MAX_KEY_CHARS)
        .map(str::to_owned)
}

fn listen_report(entry: &Value, key: Option<String>) -> Result<ListenReport, &'static str> {
    let key = key.ok_or("key muss 1–100 Zeichen haben")?;
    let seconds = entry
        .get("seconds")
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite() && *s > 0.0)
        .ok_or("seconds muss eine positive Zahl sein")?;
    let started_at = entry
        .get("started_at")
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|at| at.with_timezone(&Utc))
        .ok_or("started_at muss ein ISO-8601-Zeitpunkt sein")?;
    Ok(ListenReport {
        key,
        item_id: optional_id(entry, "item_id", "item_id muss eine ganze Zahl sein")?,
        track_id: optional_id(entry, "track_id", "track_id muss eine ganze Zahl sein")?,
        seconds,
        started_at,
    })
}

/// `{listens: [{key, item_id | track_id, seconds, started_at}]}`: books listening time that no
/// session saw — media played offline and handed in later, single tracks, playlists.
///
/// Every entry gets its own verdict instead of one bad entry failing the request: a client hands
/// in a queue it collected offline, and an entry that can never be booked (its item was deleted
/// meanwhile) must not keep the rest of the queue from ever getting through. Whatever the
/// verdict, the client can drop the entry; sending it again changes nothing.
async fn report_listens(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Json<ListensOut>> {
    let entries = body
        .get("listens")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::unprocessable("listens muss eine Liste sein"))?;
    if entries.len() > MAX_LISTENS_PER_REQUEST {
        return Err(ApiError::unprocessable(format!(
            "Höchstens {MAX_LISTENS_PER_REQUEST} Einträge je Anfrage"
        )));
    }
    let mut results = Vec::with_capacity(entries.len());
    for entry in entries {
        let key = listen_key(entry);
        let outcome = match listen_report(entry, key.clone()) {
            Ok(report) => book_listen(&state.db, user.id, &report).await?,
            Err(detail) => ListenOutcome::Rejected(detail),
        };
        results.push(match outcome {
            ListenOutcome::Booked(seconds) => ListenResult {
                key,
                status: "booked",
                seconds: Some(seconds),
                detail: None,
            },
            ListenOutcome::Duplicate => ListenResult {
                key,
                status: "duplicate",
                seconds: None,
                detail: None,
            },
            ListenOutcome::Rejected(detail) => ListenResult {
                key,
                status: "rejected",
                seconds: None,
                detail: Some(detail),
            },
        });
    }
    Ok(Json(ListensOut { results }))
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

#[derive(Serialize)]
struct HistoryOut {
    entries: Vec<HistoryEntry>,
}

/// `?limit=`: what the user heard last, newest first, one entry per stretch of the same item.
async fn history(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<HistoryOut>> {
    let limit = int_param(&query, "limit", 50, 1, 200)?;
    let runs = listening_history(&state.db, user.id, limit).await?;
    let ids: Vec<i32> = runs.iter().map(|run| run.item_id).collect();
    let items: HashMap<i32, ItemSummary> = queries::items_by_ids(&state.db, &ids)
        .await?
        .iter()
        .map(|item| (item.id, ItemSummary::from(item)))
        .collect();
    Ok(Json(HistoryOut {
        entries: runs
            .into_iter()
            .filter_map(|run| {
                Some(HistoryEntry {
                    listened_at: run.listened_at,
                    seconds: run.seconds,
                    item: items.get(&run.item_id)?.clone(),
                })
            })
            .collect(),
    }))
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
