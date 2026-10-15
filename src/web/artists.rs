//! `/library/artists` and `/library/artists/{name}` (legacy `library/artists/**`). The detail page
//! is where a Verwalter picks an artist's picture: upload one, choose an album, or reset.

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Router, routing::get, routing::post};
use chrono::Utc;

use super::format::{encode_component, initials};
use super::layout::Shell;
use crate::api::artists::{UPLOAD_BODY_LIMIT, read_image_upload};
use crate::auth::{WebAdmin, WebUser};
use crate::error::WebResult;
use crate::library::artist_covers::{
    chosen_cover, chosen_covers, clear_cover, effective_album, fallback_covers, select_album,
    store_image,
};
use crate::library::queries::{albums_of_artist, artists};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/artists", get(list))
        .route("/library/artists/{name}", get(detail))
        .route("/library/artists/{name}/select", post(select))
        .route(
            "/library/artists/{name}/upload",
            post(upload).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/library/artists/{name}/reset", post(reset))
}

/// An uploaded picture's address, with a cache buster: the file behind it changes in place.
fn image_url(artist: &str) -> String {
    format!(
        "/artists/{}/image?v={}",
        encode_component(artist),
        Utc::now().timestamp_millis()
    )
}

pub struct ArtistTile {
    pub name: String,
    /// Lower-cased, for the page's own filter.
    pub search: String,
    pub albums_href: String,
    pub edit_href: String,
    pub cover_url: Option<String>,
    pub initials: String,
    /// "1 Album", "3 Alben".
    pub count_line: String,
}

#[derive(Template)]
#[template(path = "pages/library/artists.html")]
struct ArtistsPage {
    shell: Shell,
    artists: Vec<ArtistTile>,
}

async fn list(State(state): State<AppState>, user: WebUser, uri: Uri) -> WebResult<Html<String>> {
    let (shell, rows, chosen, fallback) = tokio::try_join!(
        Shell::load(&state.db, &user, &uri),
        async { Ok(artists(&state.db).await?) },
        async { Ok(chosen_covers(&state.db).await?) },
        async { Ok(fallback_covers(&state.db).await?) },
    )?;
    let tiles = rows
        .into_iter()
        .map(|artist| {
            let pick = chosen.get(&artist.name);
            let cover_url = if pick.is_some_and(|p| p.image_path.is_some()) {
                Some(format!("{}&size=384", image_url(&artist.name)))
            } else {
                effective_album(pick, &fallback, &artist.name)
                    .map(|id| format!("/items/{id}/cover?size=384"))
            };
            let encoded = encode_component(&artist.name);
            ArtistTile {
                search: artist.name.to_lowercase(),
                albums_href: format!("/library/albums?artist={encoded}"),
                edit_href: format!("/library/artists/{encoded}"),
                cover_url,
                initials: initials(&artist.name),
                count_line: format!(
                    "{} {}",
                    artist.album_count,
                    if artist.album_count == 1 {
                        "Album"
                    } else {
                        "Alben"
                    }
                ),
                name: artist.name,
            }
        })
        .collect();
    let page = ArtistsPage {
        shell,
        artists: tiles,
    };
    Ok(Html(page.render()?))
}

pub struct AlbumChoice {
    pub id: i32,
    pub title: String,
    pub year: String,
    pub has_cover: bool,
    /// Its cover is the artist's picture right now.
    pub active: bool,
    /// Chosen by hand (and overridden by an uploaded image).
    pub saved: bool,
}

#[derive(Template)]
#[template(path = "pages/library/artist.html")]
struct ArtistPage {
    shell: Shell,
    artist: String,
    /// `/library/artists/<encoded name>`, where the forms post to.
    base: String,
    error: Option<String>,
    custom_image_url: Option<String>,
    albums: Vec<AlbumChoice>,
}

async fn detail(
    State(state): State<AppState>,
    admin: WebAdmin,
    uri: Uri,
    Path(artist): Path<String>,
) -> WebResult<Html<String>> {
    render_detail(&state, admin, &uri, artist, None).await
}

async fn render_detail(
    state: &AppState,
    admin: WebAdmin,
    uri: &Uri,
    artist: String,
    error: Option<String>,
) -> WebResult<Html<String>> {
    let user = WebUser {
        id: admin.id,
        token: admin.token,
    };
    let (shell, albums, cover, fallback) = tokio::try_join!(
        Shell::load(&state.db, &user, uri),
        async { Ok(albums_of_artist(&state.db, &artist).await?) },
        async { Ok(chosen_cover(&state.db, &artist).await?) },
        async { Ok(fallback_covers(&state.db).await?) },
    )?;
    let has_custom_image = cover.as_ref().is_some_and(|c| c.image_path.is_some());
    let effective = effective_album(cover.as_ref(), &fallback, &artist);
    let selected = cover.as_ref().and_then(|c| c.item_id);
    let page = ArtistPage {
        shell,
        base: format!("/library/artists/{}", encode_component(&artist)),
        error,
        custom_image_url: has_custom_image.then(|| image_url(&artist)),
        albums: albums
            .iter()
            .map(|album| {
                let active = !has_custom_image && Some(album.id) == effective;
                AlbumChoice {
                    id: album.id,
                    title: album.title.clone(),
                    year: album.year.map(|y| y.to_string()).unwrap_or_default(),
                    has_cover: album.cover_path.is_some(),
                    active,
                    saved: !active && Some(album.id) == selected,
                }
            })
            .collect(),
        artist,
    };
    Ok(Html(page.render()?))
}

/// The detail page again, with why the form was refused (legacy `fail(422, { error })`).
async fn refused(
    state: &AppState,
    admin: WebAdmin,
    artist: String,
    error: String,
) -> WebResult<Response> {
    let uri: Uri = format!("/library/artists/{}", encode_component(&artist))
        .parse()
        .unwrap_or_default();
    let page = render_detail(state, admin, &uri, artist, Some(error)).await?;
    Ok((StatusCode::UNPROCESSABLE_ENTITY, page).into_response())
}

/// "Album als Bild": this album's cover, then back to the artists.
async fn select(
    State(state): State<AppState>,
    admin: WebAdmin,
    Path(artist): Path<String>,
    body: Bytes,
) -> WebResult<Response> {
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let item_id = form
        .get("item_id")
        .and_then(|v| v.trim().parse::<i32>().ok())
        .filter(|id| *id > 0);
    let Some(item_id) = item_id else {
        return refused(&state, admin, artist, "Ungültige Album-ID".into()).await;
    };
    if let Err(e) = select_album(&state.db, &artist, item_id).await {
        return refused(&state, admin, artist, e.detail).await;
    }
    Ok(Redirect::to("/library/artists").into_response())
}

/// "Hochladen": the picture becomes the artist's, then back to the artists.
async fn upload(
    State(state): State<AppState>,
    admin: WebAdmin,
    Path(artist): Path<String>,
    multipart: Result<Multipart, MultipartRejection>,
) -> WebResult<Response> {
    let upload = match read_image_upload(multipart).await {
        Ok(Some(upload)) if upload.size > 0 => upload,
        Ok(_) => {
            return refused(&state, admin, artist, "Keine Bilddatei ausgewählt".into()).await;
        }
        Err(e) => return refused(&state, admin, artist, e.detail).await,
    };
    if let Err(e) = store_image(
        &state.db,
        &artist,
        &upload.content_type,
        upload.size,
        &upload.bytes,
        &state.config.artists_dir,
    )
    .await
    {
        return refused(&state, admin, artist, e.detail).await;
    }
    Ok(Redirect::to("/library/artists").into_response())
}

/// "Auf Zufall zurücksetzen": stays on the page, which now shows the random album.
async fn reset(
    State(state): State<AppState>,
    _admin: WebAdmin,
    Path(artist): Path<String>,
) -> WebResult<Response> {
    clear_cover(&state.db, &artist).await?;
    Ok(Redirect::to(&format!("/library/artists/{}", encode_component(&artist))).into_response())
}
