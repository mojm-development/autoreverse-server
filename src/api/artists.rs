//! `GET /artists`, `GET|PUT|DELETE /artists/{name}/cover`, `GET|POST /artists/{name}/image`:
//! an artist's picture, and choosing it — an album of theirs, or an uploaded image. And
//! `GET /artists/{name}/info`, `GET /artists/{name}/pictures/{index}`: who the artist is.

use std::path::PathBuf;

use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get};
use serde::Serialize;
use serde_json::{Value, json};

use super::images::{cover_roots, serve_image, thumb_size};
use super::validate::{ApiJson, ApiQuery, QueryMap};
use crate::auth::{ApiAdmin, ApiUser};
use crate::error::{ApiError, ApiResult};
use crate::library::artist_covers::{
    MAX_IMAGE_BYTES, TOO_LARGE, chosen_cover, clear_cover, effective_album, fallback_covers,
    select_album, store_image,
};
use crate::library::artist_info::{self, NO_PICTURE};
use crate::library::queries;
use crate::serialize::{TrackSummary, js_number, to_iso};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/artists", get(list))
        .route("/genres", get(genres))
        .route(
            "/artists/{name}/cover",
            get(cover).put(choose_album).delete(reset_cover),
        )
        .route("/artists/{name}/info", get(info))
        .route("/artists/{name}/pictures/{index}", get(picture))
        .route(
            "/artists/{name}/image",
            get(image)
                .post(upload)
                // Room for the largest allowed picture plus the multipart framing; reading stops
                // at the limit anyway.
                .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
}

#[derive(Serialize)]
struct ArtistOut {
    name: String,
    album_count: i32,
}

#[derive(Serialize)]
struct ArtistsOut {
    artists: Vec<ArtistOut>,
}

#[derive(Serialize)]
struct GenresOut {
    genres: Vec<ArtistOut>,
}

/// `GET /genres` (new in Rust): every album genre with its album count, for `/items?genre=`.
async fn genres(State(state): State<AppState>, _user: ApiUser) -> ApiResult<Json<GenresOut>> {
    let rows = queries::genres(&state.db).await?;
    Ok(Json(GenresOut {
        genres: rows
            .into_iter()
            .map(|g| ArtistOut {
                name: g.name,
                album_count: g.album_count,
            })
            .collect(),
    }))
}

async fn list(State(state): State<AppState>, _user: ApiUser) -> ApiResult<Json<ArtistsOut>> {
    let rows = queries::artists(&state.db).await?;
    Ok(Json(ArtistsOut {
        artists: rows
            .into_iter()
            .map(|a| ArtistOut {
                name: a.name,
                album_count: a.album_count,
            })
            .collect(),
    }))
}

fn artist_name(name: String) -> ApiResult<String> {
    if name.is_empty() {
        return Err(ApiError::unprocessable("Kein Interpret angegeben"));
    }
    Ok(name)
}

#[derive(Serialize)]
struct CoverOut {
    artist: String,
    /// `item_id` and `has_image` exclude each other (`artist_cover_xor`). Both are always there:
    /// from a missing id alone a client could not tell "nothing chosen" from "an uploaded file".
    item_id: Option<i32>,
    has_image: bool,
}

/// What was chosen for an artist, without resolving the fallback.
async fn cover(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(name): Path<String>,
) -> ApiResult<Json<CoverOut>> {
    let artist = artist_name(name)?;
    let chosen = chosen_cover(&state.db, &artist).await?;
    Ok(Json(CoverOut {
        item_id: chosen.as_ref().and_then(|c| c.item_id),
        has_image: chosen.as_ref().is_some_and(|c| c.image_path.is_some()),
        artist,
    }))
}

const NO_IMAGE: &str = "Kein Bild";
const IMAGE_CACHE: &str = "private, max-age=300";

/// The artist's picture from whichever source applies: an uploaded file, the chosen album's
/// cover, or the fallback album's. Served under the artist's address rather than redirected to
/// `/items/{id}/cover`, so a changed choice changes the content, not the location.
async fn image(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(artist): Path<String>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    let size = thumb_size(&query)?;
    let chosen = chosen_cover(&state.db, &artist).await?;
    if let Some(path) = chosen.as_ref().and_then(|c| c.image_path.clone()) {
        return serve_image(
            &PathBuf::from(path),
            std::slice::from_ref(&state.config.artists_dir),
            NO_IMAGE,
            IMAGE_CACHE,
            size,
            &state.config.thumbs_dir,
            &headers,
        )
        .await;
    }
    let fallback = fallback_covers(&state.db).await?;
    let item_id = effective_album(chosen.as_ref(), &fallback, &artist)
        .ok_or_else(|| ApiError::not_found(NO_IMAGE))?;
    let cover_path = queries::item(&state.db, item_id)
        .await?
        .and_then(|item| item.cover_path)
        .ok_or_else(|| ApiError::not_found(NO_IMAGE))?;
    // The same roots `/items/{id}/cover` allows: an album cover is no less a stored path.
    let roots = cover_roots(&state.db, &state.config).await?;
    serve_image(
        &PathBuf::from(cover_path),
        &roots,
        NO_IMAGE,
        IMAGE_CACHE,
        size,
        &state.config.thumbs_dir,
        &headers,
    )
    .await
}

#[derive(Serialize)]
struct PictureOut {
    /// The picture's place in `GET /artists/{name}/pictures/{index}`.
    index: usize,
    /// `portrait` or `backdrop`.
    kind: String,
}

#[derive(Serialize)]
struct TopTrackOut {
    title: String,
    album: String,
    #[serde(serialize_with = "js_number")]
    duration: f64,
    /// The library's own track of this song; `null` when it is not there.
    track: Option<TrackSummary>,
}

#[derive(Serialize)]
struct InfoOut {
    artist: String,
    /// Whether a catalogue knew the artist. Without it everything below is empty.
    found: bool,
    biography: Option<String>,
    /// The language the biography is in, which is English when the asked one is not there.
    biography_language: Option<String>,
    genre: Option<String>,
    country: Option<String>,
    formed_year: Option<i32>,
    born_year: Option<i32>,
    members: Option<i32>,
    pictures: Vec<PictureOut>,
    top_tracks: Vec<TopTrackOut>,
    fetched_at: Option<String>,
}

/// `GET /artists/{name}/info?lang=` (new in Rust): biography, facts, pictures and the most
/// played tracks, each with the library's own track when there is one. The first request for an
/// artist asks the catalogues and so takes a moment; after that the answer comes from the
/// database. A catalogue that cannot be reached is no error here: `found` is then false.
async fn info(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(name): Path<String>,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<InfoOut>> {
    let artist = artist_name(name)?;
    if !artist_info::is_album_artist(&state.db, &artist).await? {
        return Err(ApiError::not_found("Interpret nicht gefunden"));
    }
    let language: String = query
        .get("lang")
        .map(|l| l.chars().take(2).flat_map(char::to_lowercase).collect())
        .filter(|l: &String| l.len() == 2 && l.chars().all(|c| c.is_ascii_lowercase()))
        .unwrap_or_else(|| "de".into());
    let known = artist_info::artist_info(&state, &artist).await?;
    let tracks = if known.info.top_tracks.is_empty() {
        Vec::new()
    } else {
        artist_info::library_tracks(&state.db, &artist).await?
    };
    let biography = known.info.biography(&language);
    Ok(Json(InfoOut {
        found: known.found,
        biography: biography.as_ref().map(|(_, text)| (*text).to_owned()),
        biography_language: biography.map(|(language, _)| language),
        genre: known.info.genre.clone(),
        country: known.info.country.clone(),
        formed_year: known.info.formed_year,
        born_year: known.info.born_year,
        members: known.info.members,
        pictures: known
            .info
            .pictures
            .iter()
            .enumerate()
            .map(|(index, picture)| PictureOut {
                index,
                kind: picture.kind.clone(),
            })
            .collect(),
        top_tracks: known
            .info
            .top_tracks
            .iter()
            .map(|top| TopTrackOut {
                title: top.title.clone(),
                album: top.album.clone(),
                duration: top.duration,
                track: artist_info::best_hit(&tracks, &artist, top).map(TrackSummary::from),
            })
            .collect(),
        fetched_at: known.fetched_at.map(to_iso),
        artist,
    }))
}

/// One of the pictures `info` lists, from the server's own disk; fetched from the catalogue the
/// first time somebody asks for it.
async fn picture(
    State(state): State<AppState>,
    _user: ApiUser,
    Path((artist, index)): Path<(String, String)>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    let size = thumb_size(&query)?;
    let index: usize = index.parse().map_err(|_| ApiError::not_found(NO_PICTURE))?;
    let path = artist_info::picture(&state, &artist, index).await?;
    serve_image(
        &path,
        std::slice::from_ref(&state.config.artists_dir),
        NO_PICTURE,
        IMAGE_CACHE,
        size,
        &state.config.thumbs_dir,
        &headers,
    )
    .await
}

/// The request body an upload may have.
pub const UPLOAD_BODY_LIMIT: usize = MAX_IMAGE_BYTES + 1024 * 1024;

/// The `image` field of a multipart upload.
pub struct ImageUpload {
    pub content_type: String,
    /// The real size, or one past the limit when reading stopped there.
    pub size: usize,
    pub bytes: Vec<u8>,
}

fn upload_error(e: &axum::extract::multipart::MultipartError) -> ApiError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, TOO_LARGE)
    } else {
        ApiError::unprocessable("Ungültiger Upload")
    }
}

/// The first `image` field that is a file, read up to just past [`MAX_IMAGE_BYTES`]; `None`
/// without one. A body that is no multipart form is 422 "Ungültiger Upload".
pub async fn read_image_upload(
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult<Option<ImageUpload>> {
    let mut multipart = multipart.map_err(|_| ApiError::unprocessable("Ungültiger Upload"))?;
    while let Some(mut field) = multipart.next_field().await.map_err(|e| upload_error(&e))? {
        if field.name() != Some("image") {
            continue;
        }
        if field.file_name().is_none() {
            return Ok(None);
        }
        let content_type = field.content_type().unwrap_or("").to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|e| upload_error(&e))? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_IMAGE_BYTES {
                break;
            }
        }
        return Ok(Some(ImageUpload {
            content_type,
            size: bytes.len(),
            bytes,
        }));
    }
    Ok(None)
}

/// An album of the artist's as their picture.
async fn choose_album(
    State(state): State<AppState>,
    _admin: ApiAdmin,
    Path(name): Path<String>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let artist = artist_name(name)?;
    // `Number(body.item_id)`: a number, or a string holding one.
    let item_id = match body.get("item_id") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|n| n.fract() == 0.0 && *n > 0.0 && *n <= f64::from(i32::MAX))
    .map(|n| n as i32)
    .ok_or_else(|| ApiError::unprocessable("Ungültige Album-ID"))?;
    select_album(&state.db, &artist, item_id).await?;
    Ok(Json(json!({ "artist": artist, "item_id": item_id })))
}

/// Back to the random-but-stable album.
async fn reset_cover(
    State(state): State<AppState>,
    _admin: ApiAdmin,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let artist = artist_name(name)?;
    clear_cover(&state.db, &artist).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// An uploaded picture (multipart field `image`): JPEG, PNG, WebP or GIF up to 5 MB.
async fn upload(
    State(state): State<AppState>,
    _admin: ApiAdmin,
    Path(name): Path<String>,
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult<Response> {
    let artist = artist_name(name)?;
    let upload = read_image_upload(multipart)
        .await?
        .ok_or_else(|| ApiError::unprocessable("Keine Bilddatei im Feld \"image\""))?;
    store_image(
        &state.db,
        &artist,
        &upload.content_type,
        upload.size,
        &upload.bytes,
        &state.config.artists_dir,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({ "artist": artist }))).into_response())
}
