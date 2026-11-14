//! `GET /radio`, the "Titel-Radio": tracks from the library that go with a track, an album or an
//! artist — what a client plays on with when its queue runs out. New in Rust; the choosing is in
//! `library::radio`. (Internet radio stations are `api::stations`, under `/radio/stations`.)

use axum::extract::{RawQuery, State};
use axum::{Json, Router, routing::get};
use serde::Serialize;

use super::validate::{INVALID_QUERY, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::radio::{
    Chance, DEFAULT_LIMIT, MAX_EXCLUDED, MAX_LIMIT, RadioError, RadioSeed, radio,
};
use crate::serialize::TrackSummary;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/radio", get(tracks))
}

pub const SEED_REQUIRED: &str =
    "Genau einer der Parameter track_id, item_id oder artist muss angegeben sein";
/// "Titel-Radio" is what the UIs call this since internet radio stations (`api::stations`) took
/// the plain name.
pub const NOT_MUSIC: &str = "Titel-Radio gibt es nur für Musik";

#[derive(Serialize)]
struct RadioOut {
    tracks: Vec<TrackSummary>,
}

fn whole_id(name: &str, raw: &str) -> ApiResult<i32> {
    raw.trim()
        .parse()
        .map_err(|_| ApiError::unprocessable(format!("{name} muss eine ganze Zahl sein")))
}

/// The one seed the query names. A blank `artist` names nobody.
fn seed_of(query: &QueryMap) -> ApiResult<RadioSeed> {
    let artist = query.get("artist").filter(|name| !name.trim().is_empty());
    match (query.get("track_id"), query.get("item_id"), artist) {
        (Some(raw), None, None) => Ok(RadioSeed::Track(whole_id("track_id", raw)?)),
        (None, Some(raw), None) => Ok(RadioSeed::Album(whole_id("item_id", raw)?)),
        (None, None, Some(name)) => Ok(RadioSeed::Artist(name.clone())),
        _ => Err(ApiError::unprocessable(SEED_REQUIRED)),
    }
}

/// `?track_id=` | `?item_id=` | `?artist=`, plus `limit` and any number of `exclude=<track id>`
/// for what the client has queued already. Read from the raw query: the other routes keep only
/// the first value of a repeated key, and `exclude` is meant to repeat.
async fn tracks(
    State(state): State<AppState>,
    user: ApiUser,
    RawQuery(raw): RawQuery,
) -> ApiResult<Json<RadioOut>> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(raw.as_deref().unwrap_or(""))
        .map_err(|_| ApiError::unprocessable(INVALID_QUERY))?;
    let mut query = QueryMap::new();
    let mut excluded = Vec::new();
    for (key, value) in pairs {
        if key == "exclude" {
            if excluded.len() >= MAX_EXCLUDED {
                return Err(ApiError::unprocessable(format!(
                    "Höchstens {MAX_EXCLUDED} exclude-Werte je Anfrage"
                )));
            }
            excluded.push(whole_id("exclude", &value)?);
        } else {
            query.entry(key).or_insert(value);
        }
    }
    let seed = seed_of(&query)?;
    let limit = int_param(&query, "limit", DEFAULT_LIMIT, 1, MAX_LIMIT)?;
    let found = radio(
        &state.db,
        user.id,
        &seed,
        limit as usize,
        &excluded,
        &mut Chance::from_os(),
    )
    .await
    .map_err(|e| match e {
        RadioError::UnknownTrack => ApiError::not_found("Unbekannter Track"),
        RadioError::UnknownItem => ApiError::not_found("Unbekanntes Item"),
        RadioError::UnknownArtist => ApiError::not_found("Interpret nicht gefunden"),
        RadioError::NotMusic => ApiError::unprocessable(NOT_MUSIC),
        RadioError::Database(e) => ApiError::from(e),
    })?;
    Ok(Json(RadioOut {
        tracks: found.iter().map(TrackSummary::from).collect(),
    }))
}
