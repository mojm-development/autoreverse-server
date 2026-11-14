//! `/radio/stations…`: a user's internet radio stations — list, search in the directory, add,
//! rename, reorder, remove, the logo, the logo of a search hit, the live stream and what plays
//! on it. New in Rust; the
//! work is in `stations`. (`GET /radio` itself is the Titel-Radio of `api::radio`.)

use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get};
use serde::Serialize;
use serde_json::{Map, Value};

use super::images::{image_response, serve_image, thumb_size};
use super::validate::{ApiQuery, JsonBody, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::serialize::StationOut;
use crate::state::AppState;
use crate::stations::add::{self, AddError, DIRECTORY_OFF, NAME_INVALID, valid_name};
use crate::stations::directory::{self, DEFAULT_LIMIT, MAX_LIMIT, country_code};
use crate::stations::icy::now_playing;
use crate::stations::logos::{logo_path, logos_dir};
use crate::stations::store::{self, Station};
use crate::stations::{SearchLogos, live, logos};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/radio/stations", get(list).post(create))
        .route("/radio/stations/search", get(search))
        .route("/radio/stations/search/logo", get(search_logo))
        .route(
            "/radio/stations/{id}",
            axum::routing::patch(patch).delete(remove),
        )
        // axum answers HEAD from the GET route; the handler does not call the sender for it.
        .route("/radio/stations/{id}/stream", get(stream))
        .route("/radio/stations/{id}/now", get(now))
        .route("/radio/stations/{id}/logo", get(logo))
}

pub const RADIO_OFF: &str = "Radio ist auf diesem Server abgeschaltet";
pub const UNKNOWN_STATION: &str = "Unbekannter Sender";
pub const NO_LOGO: &str = "Kein Logo";
pub const STATION_UNREACHABLE: &str = "Sender nicht erreichbar";
pub const TOO_MANY_STREAMS: &str = "Zu viele Radio-Streams gleichzeitig";
pub const BODY_INVALID: &str =
    "Entweder directory_id oder stream_url und name müssen angegeben sein";
pub const PATCH_INVALID: &str =
    "name (1–100 Zeichen) oder position (ganze Zahl ab 1) muss angegeben sein";
const LOGO_CACHE: &str = "private, max-age=86400";

/// The caller of a radio route: 404 on a server with the feature switched off — before the
/// login is looked at, so such a server answers as if the routes did not exist — else an
/// authenticated user (bearer token or session cookie, as on every API route).
pub struct RadioUser(pub ApiUser);

impl FromRequestParts<AppState> for RadioUser {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if !state.settings.get().radio {
            return Err(ApiError::not_found(RADIO_OFF));
        }
        ApiUser::from_request_parts(parts, state)
            .await
            .map(RadioUser)
    }
}

impl From<AddError> for ApiError {
    fn from(e: AddError) -> Self {
        match e {
            AddError::Invalid(detail) => ApiError::unprocessable(detail),
            AddError::DirectoryOff | AddError::UnknownStation => ApiError::not_found(e.to_string()),
            AddError::Directory(detail) => ApiError::new(StatusCode::BAD_GATEWAY, detail),
            AddError::Db(e) => e.into(),
        }
    }
}

/// The user's own station of the path; anything else — no number, unknown, somebody else's —
/// names no station.
async fn own(state: &AppState, user: &ApiUser, raw_id: &str) -> ApiResult<Station> {
    let not_found = || ApiError::not_found(UNKNOWN_STATION);
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    store::owned(&state.db, user.id, id)
        .await?
        .ok_or_else(not_found)
}

#[derive(Serialize)]
struct StationsOut {
    stations: Vec<StationOut>,
}

async fn list(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
) -> ApiResult<Json<StationsOut>> {
    let stations = store::list(&state.db, user.id).await?;
    logos::backfill(&state, &stations);
    Ok(Json(StationsOut {
        stations: stations.into_iter().map(StationOut::from).collect(),
    }))
}

#[derive(Serialize)]
struct ResultOut {
    directory_id: String,
    name: String,
    homepage: Option<String>,
    country: Option<String>,
    tags: Vec<String>,
    codec: Option<String>,
    bitrate: Option<i32>,
    /// The directory's word, kept for older clients. Nobody should load it: the logo comes
    /// from `GET /radio/stations/search/logo?id=`.
    logo_url: Option<String>,
    /// "Worth asking for": the directory names a logo or a homepage whose icon can stand in.
    /// Nothing has been fetched when the search answers, so the route may still say 404.
    has_logo: bool,
    /// Whether the user has this station already.
    added: bool,
}

#[derive(Serialize)]
struct SearchOut {
    results: Vec<ResultOut>,
}

/// `q` (1–100 characters), `country` (two letters, optional), `limit` (1–50, default 25).
async fn search(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<SearchOut>> {
    if !state.settings.get().radio_directory_on() {
        return Err(ApiError::not_found(DIRECTORY_OFF));
    }
    let q = query.get("q").map_or("", |q| q.trim());
    if !(1..=100).contains(&q.chars().count()) {
        return Err(ApiError::unprocessable("q muss 1–100 Zeichen haben"));
    }
    let limit = int_param(&query, "limit", DEFAULT_LIMIT, 1, MAX_LIMIT)?;
    let country = match query
        .get("country")
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
    {
        None => None,
        Some(raw) => Some(country_code(raw).ok_or_else(|| {
            ApiError::unprocessable("country muss ein Ländercode aus 2 Buchstaben sein")
        })?),
    };
    let found = directory::search(
        &state.http,
        &state.config.radio_directory_url,
        q,
        country.as_deref(),
        limit,
    )
    .await
    .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.0))?;
    state.radio_logos.remember(&found);
    let mine = store::directory_ids(&state.db, user.id).await?;
    Ok(Json(SearchOut {
        results: found
            .into_iter()
            .map(|station| ResultOut {
                added: mine
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&station.directory_id)),
                has_logo: station.may_have_logo(),
                directory_id: station.directory_id,
                name: station.name,
                homepage: station.homepage,
                country: station.country,
                tags: station.tags,
                codec: station.codec,
                bitrate: station.bitrate,
                logo_url: station.logo_url,
            })
            .collect(),
    }))
}

/// The logo of a search hit: `id` is the hit's `directory_id`, `size` as on every image route.
/// There is deliberately no way to name an address here — the server fetches only what the
/// directory names for that id — its logo, else the icons of its homepage
/// (`stations::search_logos`, `site_icon`) — so the route cannot be used to make it fetch
/// something else. Whatever keeps the logo from being shown is a 404.
async fn search_logo(
    State(state): State<AppState>,
    RadioUser(_user): RadioUser,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    if !state.settings.get().radio_directory_on() {
        return Err(ApiError::not_found(DIRECTORY_OFF));
    }
    let size = thumb_size(&query)?;
    let id = query.get("id").map_or("", String::as_str);
    let logo = SearchLogos::get(&state, id)
        .await
        .ok_or_else(|| ApiError::not_found(NO_LOGO))?;
    let picture = state.radio_logos.picture(&logo, size).await;
    Ok(image_response(
        picture.content_type,
        LOGO_CACHE,
        picture.data,
        &headers,
    ))
}

/// A string field: `Ok(None)` when absent or `null`, an error when it is anything but a string.
fn string_field<'a>(body: &'a Map<String, Value>, key: &str) -> ApiResult<Option<&'a str>> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(ApiError::unprocessable(format!(
            "{key} muss eine Zeichenkette sein"
        ))),
    }
}

/// `{directory_id}` or `{stream_url, name, homepage?}` → 201 with the new station, 200 with the
/// one the user has already.
async fn create(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
    body: JsonBody,
) -> ApiResult<(StatusCode, Json<StationOut>)> {
    let body: Map<String, Value> = body.parse()?;
    let directory_id = string_field(&body, "directory_id")?;
    let stream_url = string_field(&body, "stream_url")?;
    let name = string_field(&body, "name")?;
    let homepage = string_field(&body, "homepage")?;
    let (station, created) = match (directory_id, stream_url, name) {
        (Some(id), None, None) => add::add_from_directory(&state, user.id, id).await?,
        (None, Some(url), Some(name)) => {
            add::add_by_address(&state, user.id, url, name, homepage).await?
        }
        _ => return Err(ApiError::unprocessable(BODY_INVALID)),
    };
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(station.into())))
}

/// `{name?, position?}`: rename, move within the list (1-based; past the end is the end), or
/// both.
async fn patch(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
    Path(raw_id): Path<String>,
    body: JsonBody,
) -> ApiResult<Json<StationOut>> {
    let station = own(&state, &user, &raw_id).await?;
    let body: Map<String, Value> = body.parse()?;
    let invalid = || ApiError::unprocessable(PATCH_INVALID);
    let name = match body.get("name") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .and_then(valid_name)
                .ok_or_else(|| ApiError::unprocessable(NAME_INVALID))?,
        ),
    };
    let position = match body.get("position") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_f64()
                .filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= f64::from(i32::MAX))
                .map(|n| n as i32)
                .ok_or_else(invalid)?,
        ),
    };
    if name.is_none() && position.is_none() {
        return Err(invalid());
    }
    if let Some(name) = name {
        store::rename(&state.db, station.id, name).await?;
    }
    if let Some(position) = position {
        store::move_to(&state.db, user.id, station.id, position).await?;
    }
    // Read back rather than patched up here: it may have been removed in between.
    let station = own(&state, &user, &raw_id).await?;
    Ok(Json(station.into()))
}

async fn remove(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let station = own(&state, &user, &raw_id).await?;
    let removed = store::remove(&state.db, user.id, station.id)
        .await?
        .ok_or_else(|| ApiError::not_found(UNKNOWN_STATION))?;
    logos::remove(&state.config.radio_dir, &removed).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn logo(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    let station = own(&state, &user, &raw_id).await?;
    let size = thumb_size(&query)?;
    let path =
        logo_path(&state.config.radio_dir, &station).ok_or_else(|| ApiError::not_found(NO_LOGO))?;
    serve_image(
        &path,
        &[logos_dir(&state.config.radio_dir)],
        NO_LOGO,
        LOGO_CACHE,
        size,
        &state.config.thumbs_dir,
        &headers,
    )
    .await
}

/// The live stream, passed on from the sender. Only ever for a station in the caller's own
/// list: the address comes out of the database, never out of the request, so the route cannot
/// be used to fetch something else through this server. Works with the session cookie as well
/// as a bearer token, so the web player's `<audio src>` needs no extra auth.
///
/// A live stream has no length and no positions: no `Content-Length`, and a `Range` header is
/// ignored (the answer is always 200 from "now").
async fn stream(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
    Path(raw_id): Path<String>,
    method: Method,
) -> ApiResult<Response> {
    let station = own(&state, &user, &raw_id).await?;
    let headers = |content_type: String| {
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (header::ACCEPT_RANGES, "none".to_owned()),
            // A reverse proxy in front (nginx) must hand the audio on as it comes instead of
            // collecting it first.
            (
                header::HeaderName::from_static("x-accel-buffering"),
                "no".to_owned(),
            ),
        ]
    };
    if method == Method::HEAD {
        let known = station.content_type.unwrap_or_else(|| "audio/mpeg".into());
        return Ok(headers(known).into_response());
    }
    let listener = state.radio.listen(user.id, station.id).ok_or_else(|| {
        ApiError::new(StatusCode::TOO_MANY_REQUESTS, TOO_MANY_STREAMS).with_retry_after(5)
    })?;
    let upstream = live::open(&state, &station, listener).await.map_err(|e| {
        tracing::info!("[radio] Sender {} ({}): {}", station.id, station.name, e.0);
        ApiError::new(StatusCode::BAD_GATEWAY, STATION_UNREACHABLE)
    })?;
    Ok((headers(upstream.content_type), upstream.body).into_response())
}

#[derive(Serialize)]
struct NowOut {
    title: Option<String>,
    artist: Option<String>,
    raw: Option<String>,
    /// Whether a stream of this station runs through the server right now — the only time a
    /// title is known.
    listening: bool,
}

/// What plays on the station, as last read from the ICY metadata of a stream that is running
/// through this server. Without such a stream everything is `null`: the server does not tune in
/// by itself just to answer this, since every such look would be a connection to the sender
/// that nobody listens to — and clients ask only while they play, which is when it is known.
async fn now(
    State(state): State<AppState>,
    RadioUser(user): RadioUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<NowOut>> {
    let station = own(&state, &user, &raw_id).await?;
    let live = state.radio.now(station.id);
    let playing = now_playing(live.as_ref().and_then(|title| title.as_deref()));
    Ok(Json(NowOut {
        title: playing.title,
        artist: playing.artist,
        raw: playing.raw,
        listening: live.is_some(),
    }))
}
