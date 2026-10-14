//! `GET /artists`, `GET /artists/{name}/cover`, `GET /artists/{name}/image`. Choosing an album or
//! uploading a picture (`PUT`/`DELETE …/cover`, `POST …/image`) joins with metadata editing
//! (slice 8).

use std::path::PathBuf;

use axum::extract::{Path, State};
use axum::response::Response;
use axum::{Json, Router, routing::get};
use serde::Serialize;

use super::images::{cover_roots, serve_image};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::artist_covers::{chosen_cover, effective_album, fallback_covers};
use crate::library::queries;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/artists", get(list))
        .route("/artists/{name}/cover", get(cover))
        .route("/artists/{name}/image", get(image))
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
) -> ApiResult<Response> {
    let chosen = chosen_cover(&state.db, &artist).await?;
    if let Some(path) = chosen.as_ref().and_then(|c| c.image_path.clone()) {
        return serve_image(
            &PathBuf::from(path),
            std::slice::from_ref(&state.config.artists_dir),
            NO_IMAGE,
            IMAGE_CACHE,
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
    serve_image(&PathBuf::from(cover_path), &roots, NO_IMAGE, IMAGE_CACHE).await
}
