//! `GET /me/top`, `GET /me/on-repeat`, `GET /me/repeat-rewind`, `GET /me/review`,
//! `GET /me/review/years`: what the user listened to most and the year in review. New in Rust;
//! the queries are in `library::top`, and none of it involves a model.

use axum::extract::State;
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;

use super::validate::{ApiQuery, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::top::{
    MAX_YEAR, MIN_YEAR, Period, Range, on_repeat, repeat_rewind, review, review_years, top_albums,
    top_artists, top_genres, top_tracks,
};
use crate::serialize::{
    ReviewOut, TopAlbumOut, TopArtistOut, TopGenreOut, TopTrackOut, TrackSummary, iso, iso_opt,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/top", get(top))
        .route("/me/on-repeat", get(repeat))
        .route("/me/repeat-rewind", get(rewind))
        .route("/me/review", get(year_review))
        .route("/me/review/years", get(years))
}

pub const KIND_INVALID: &str = "kind muss tracks, albums, artists oder genres sein";
pub const PERIOD_INVALID: &str = "period muss week, month, year oder all sein";

/// The entries of one kind; each kind has its own shape.
#[derive(Serialize)]
#[serde(untagged)]
enum Entries {
    Tracks(Vec<TopTrackOut>),
    Albums(Vec<TopAlbumOut>),
    Artists(Vec<TopArtistOut>),
    Genres(Vec<TopGenreOut>),
}

#[derive(Serialize)]
struct TopOut {
    kind: String,
    /// Start of the period, inclusive; `null` for `period=all`.
    #[serde(serialize_with = "iso_opt")]
    from: Option<DateTime<Utc>>,
    /// End of the period, exclusive: now, or the first second after a calendar year.
    #[serde(serialize_with = "iso")]
    to: DateTime<Utc>,
    entries: Entries,
}

/// `?year=`, when there is one: a calendar year within the bounds, else 422.
fn year_param(query: &QueryMap) -> ApiResult<Option<i32>> {
    if !query.contains_key("year") {
        return Ok(None);
    }
    int_param(query, "year", 0, i64::from(MIN_YEAR), i64::from(MAX_YEAR))
        .map(|year| Some(year as i32))
}

/// The time a top list covers: a calendar year if `year` names one — it wins over `period` —
/// else the sliding period, the last 30 days by default.
fn range_of(query: &QueryMap) -> ApiResult<Range> {
    if let Some(year) = year_param(query)? {
        return Ok(Range::calendar_year(year));
    }
    let period = match query.get("period") {
        None => Period::Month,
        Some(raw) => Period::parse(raw).ok_or_else(|| ApiError::unprocessable(PERIOD_INVALID))?,
    };
    Ok(Range::last(period, Utc::now()))
}

/// `?kind=tracks|albums|artists|genres&period=week|month|year|all&year=&limit=`.
async fn top(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<TopOut>> {
    let kind = query.get("kind").map(String::as_str).unwrap_or_default();
    if !matches!(kind, "tracks" | "albums" | "artists" | "genres") {
        return Err(ApiError::unprocessable(KIND_INVALID));
    }
    let range = range_of(&query)?;
    let limit = int_param(&query, "limit", 25, 1, 100)?;
    let db = &state.db;
    let entries = match kind {
        "tracks" => Entries::Tracks(
            top_tracks(db, user.id, &range, limit)
                .await?
                .iter()
                .map(TopTrackOut::from)
                .collect(),
        ),
        "albums" => Entries::Albums(
            top_albums(db, user.id, &range, limit)
                .await?
                .iter()
                .map(TopAlbumOut::from)
                .collect(),
        ),
        "artists" => Entries::Artists(
            top_artists(db, user.id, &range, limit)
                .await?
                .iter()
                .map(TopArtistOut::from)
                .collect(),
        ),
        _ => Entries::Genres(
            top_genres(db, user.id, &range, limit)
                .await?
                .iter()
                .map(TopGenreOut::from)
                .collect(),
        ),
    };
    Ok(Json(TopOut {
        kind: kind.to_owned(),
        from: range.from,
        to: range.to,
        entries,
    }))
}

#[derive(Serialize)]
struct TracksOut {
    tracks: Vec<TrackSummary>,
}

/// `?limit=`: the tracks played most often in the last 30 days.
async fn repeat(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<TracksOut>> {
    let limit = int_param(&query, "limit", 30, 1, 100)?;
    let tracks = on_repeat(&state.db, user.id, limit).await?;
    Ok(Json(TracksOut {
        tracks: tracks.iter().map(TrackSummary::from).collect(),
    }))
}

/// `?limit=`: what was played often once and not at all in the last 30 days.
async fn rewind(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<TracksOut>> {
    let limit = int_param(&query, "limit", 30, 1, 100)?;
    let tracks = repeat_rewind(&state.db, user.id, limit).await?;
    Ok(Json(TracksOut {
        tracks: tracks.iter().map(TrackSummary::from).collect(),
    }))
}

/// `?year=`: that calendar year (UTC), the current one by default. A year without listening is
/// zeros and empty lists, not a 404 — a client asks for a year before it knows what is in it.
async fn year_review(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<ReviewOut>> {
    let year = year_param(&query)?.unwrap_or_else(|| Utc::now().year());
    let found = review(&state.db, user.id, year).await?;
    Ok(Json(ReviewOut::from(&found)))
}

#[derive(Serialize)]
struct YearsOut {
    years: Vec<i32>,
}

async fn years(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<YearsOut>> {
    Ok(Json(YearsOut {
        years: review_years(&state.db, user.id).await?,
    }))
}
