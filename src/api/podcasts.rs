//! `POST /podcasts`, `DELETE /podcasts/{id}`, `POST /podcasts/{id}/refresh`,
//! `POST /podcasts/preview`, `GET /podcasts/search`, `POST /episodes/{id}/download`
//! (legacy `routes/podcasts/**`, `routes/episodes/**`).
//!
//! Every signed-in user may subscribe: a podcast someone adds for themselves is not
//! administration, and the native clients offer it on every account.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing::delete, routing::get, routing::post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use super::validate::{ApiJson, ApiQuery, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::podcasts::directory::{DEFAULT_COUNTRY, DEFAULT_LIMIT, search_directory};
use crate::podcasts::download::{DownloadError, download_episode};
use crate::podcasts::retention::retain_for_podcast;
use crate::podcasts::store::{FeedError, preview_feed, refresh, subscribe, unsubscribe};
use crate::serialize::iso_opt;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/podcasts", post(create))
        .route("/podcasts/{id}", delete(remove))
        .route("/podcasts/{id}/refresh", post(refresh_one))
        .route("/podcasts/preview", post(preview))
        .route("/podcasts/search", get(search))
        .route("/episodes/{id}/download", post(download))
}

pub const UNKNOWN_PODCAST: &str = "Unbekannter Podcast";
pub const UNKNOWN_EPISODE: &str = "Unbekannte Folge";
pub const FEED_URL_INVALID: &str = "feed_url muss eine nicht-leere Zeichenkette sein";

impl From<FeedError> for ApiError {
    fn from(e: FeedError) -> Self {
        match e {
            FeedError::Fetch(detail) => ApiError::new(StatusCode::BAD_GATEWAY, detail),
            FeedError::Invalid(detail) => ApiError::unprocessable(detail),
            FeedError::NotFound => ApiError::not_found(UNKNOWN_PODCAST),
            FeedError::Db(e) => e.into(),
        }
    }
}

impl From<DownloadError> for ApiError {
    fn from(e: DownloadError) -> Self {
        match e {
            DownloadError::NotFound => ApiError::not_found(UNKNOWN_EPISODE),
            DownloadError::NotDownloadable(detail) => ApiError::unprocessable(detail),
            DownloadError::Fetch(detail) => ApiError::new(StatusCode::BAD_GATEWAY, detail),
            DownloadError::Storage(detail) => {
                ApiError::new(StatusCode::INSUFFICIENT_STORAGE, detail)
            }
            DownloadError::Db(e) => e.into(),
        }
    }
}

/// `feed_url` out of the body: a non-empty string, else 422.
fn feed_url(body: &Value) -> ApiResult<String> {
    body.get("feed_url")
        .and_then(Value::as_str)
        .filter(|u| !u.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::unprocessable(FEED_URL_INVALID))
}

fn path_id(raw: &str, detail: &'static str) -> ApiResult<i32> {
    raw.parse().map_err(|_| ApiError::not_found(detail))
}

/// Applies retention for a podcast in the background (legacy `void retainForPodcast(…)`).
pub fn retain_in_background(state: &AppState, podcast_id: i32) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = retain_for_podcast(
            &state.db,
            &state.http,
            podcast_id,
            &state.config.podcasts_dir,
        )
        .await
        {
            tracing::error!("[podcasts] Vorhalten für Podcast {podcast_id} fehlgeschlagen: {e}");
        }
    });
}

#[derive(Serialize)]
struct PodcastOut {
    id: i32,
    title: String,
    feed_url: Option<String>,
    new_episodes: u32,
    updated_episodes: u32,
}

async fn create(
    State(state): State<AppState>,
    _user: ApiUser,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<PodcastOut>> {
    let url = feed_url(&body)?;
    let synced = subscribe(&state.db, &state.http, &url, Some(&state.config.cover_dir)).await?;
    retain_in_background(&state, synced.id);
    Ok(Json(PodcastOut {
        id: synced.id,
        title: synced.title,
        feed_url: synced.feed_url,
        new_episodes: synced.new_episodes,
        updated_episodes: synced.updated_episodes,
    }))
}

#[derive(Serialize)]
struct UnsubscribeOut {
    episodes: i64,
    files_deleted: u32,
    files_kept: u32,
}

async fn remove(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<UnsubscribeOut>> {
    let id = path_id(&raw_id, UNKNOWN_PODCAST)?;
    let report = unsubscribe(&state.db, id, &state.config.podcasts_dir).await?;
    Ok(Json(UnsubscribeOut {
        episodes: report.episodes,
        files_deleted: report.files_deleted,
        files_kept: report.files_kept,
    }))
}

#[derive(Serialize)]
struct RefreshOut {
    id: i32,
    title: String,
    feed_url: Option<String>,
    new_episodes: u32,
    updated_episodes: u32,
    downloaded: u32,
    freed: u32,
}

async fn refresh_one(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<RefreshOut>> {
    let id = path_id(&raw_id, UNKNOWN_PODCAST)?;
    let synced = refresh(&state.db, &state.http, id, Some(&state.config.cover_dir)).await?;
    let retention =
        retain_for_podcast(&state.db, &state.http, id, &state.config.podcasts_dir).await?;
    Ok(Json(RefreshOut {
        id: synced.id,
        title: synced.title,
        feed_url: synced.feed_url,
        new_episodes: synced.new_episodes,
        updated_episodes: synced.updated_episodes,
        downloaded: retention.downloaded,
        freed: retention.freed,
    }))
}

/// How many episodes a preview carries. Enough to judge a show, not a whole archive.
const PREVIEW_EPISODES: usize = 15;

#[derive(Serialize)]
struct PreviewEpisodeOut {
    title: String,
    #[serde(serialize_with = "iso_opt")]
    published_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "js_number_opt")]
    duration: Option<f64>,
}

fn js_number_opt<S: serde::Serializer>(n: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match n {
        Some(n) => crate::serialize::js_number(n, s),
        None => s.serialize_none(),
    }
}

#[derive(Serialize)]
struct PreviewOut {
    title: String,
    description: Option<String>,
    image_url: Option<String>,
    episodes: Vec<PreviewEpisodeOut>,
}

/// The feed as it would be subscribed, without subscribing.
async fn preview(
    State(state): State<AppState>,
    _user: ApiUser,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<PreviewOut>> {
    let url = feed_url(&body)?;
    let parsed = preview_feed(&state.http, &url).await?;
    Ok(Json(PreviewOut {
        title: parsed.title,
        description: parsed.description,
        image_url: parsed.image_url,
        episodes: parsed
            .episodes
            .into_iter()
            .take(PREVIEW_EPISODES)
            .map(|e| PreviewEpisodeOut {
                title: e.title,
                published_at: e.published_at,
                duration: e.duration_seconds,
            })
            .collect(),
    }))
}

#[derive(Serialize)]
struct SearchResultOut {
    name: String,
    author: String,
    feed_url: String,
    artwork_url: Option<String>,
    episode_count: Option<i64>,
    genre: Option<String>,
}

#[derive(Serialize)]
struct SearchOut {
    results: Vec<SearchResultOut>,
}

/// JS `length`: UTF-16 code units.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `q` (1–200 characters), `limit` (1–50, default 25), `country` (two letters, default DE).
async fn search(
    State(state): State<AppState>,
    _user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<SearchOut>> {
    let q = query.get("q").map_or("", String::as_str);
    if !(1..=200).contains(&js_len(q)) {
        return Err(ApiError::unprocessable("q muss 1–200 Zeichen haben"));
    }
    let limit = int_param(&query, "limit", DEFAULT_LIMIT, 1, 50)?;
    let country = query.get("country").map_or(DEFAULT_COUNTRY, String::as_str);
    if js_len(country) != 2 {
        return Err(ApiError::unprocessable(
            "country muss genau 2 Zeichen haben",
        ));
    }
    let results = search_directory(
        &state.http,
        &state.config.podcast_directory_url,
        q,
        limit,
        country,
    )
    .await
    .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.0))?;
    Ok(Json(SearchOut {
        results: results
            .into_iter()
            .map(|r| SearchResultOut {
                name: r.name,
                author: r.author,
                feed_url: r.feed_url,
                artwork_url: r.artwork_url,
                episode_count: r.episode_count,
                genre: r.genre,
            })
            .collect(),
    }))
}

#[derive(Serialize)]
struct DownloadOut {
    track_id: i32,
    #[serde(serialize_with = "crate::serialize::js_number")]
    duration: f64,
}

async fn download(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<DownloadOut>> {
    let id = path_id(&raw_id, UNKNOWN_EPISODE)?;
    let result = download_episode(&state.db, &state.http, id, &state.config.podcasts_dir).await?;
    Ok(Json(DownloadOut {
        track_id: result.track_id,
        duration: result.duration,
    }))
}
