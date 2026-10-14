//! `/library/artists` and `/library/artists/{name}` (legacy `library/artists/**`). The detail page
//! is where an admin picks an artist's picture; it shows the current choice now, the forms to
//! change it join with metadata editing (slice 8).

use askama::Template;
use axum::extract::{Path, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};
use chrono::Utc;

use super::format::{encode_component, initials};
use super::layout::Shell;
use crate::auth::{WebAdmin, WebUser};
use crate::error::WebResult;
use crate::library::artist_covers::{
    chosen_cover, chosen_covers, effective_album, fallback_covers,
};
use crate::library::queries::{albums_of_artist, artists};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/artists", get(list))
        .route("/library/artists/{name}", get(detail))
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
                Some(image_url(&artist.name))
            } else {
                effective_album(pick, &fallback, &artist.name)
                    .map(|id| format!("/items/{id}/cover"))
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
    custom_image_url: Option<String>,
    albums: Vec<AlbumChoice>,
}

async fn detail(
    State(state): State<AppState>,
    admin: WebAdmin,
    uri: Uri,
    Path(artist): Path<String>,
) -> WebResult<Html<String>> {
    let user = WebUser {
        id: admin.id,
        token: admin.token,
    };
    let (shell, albums, cover, fallback) = tokio::try_join!(
        Shell::load(&state.db, &user, &uri),
        async { Ok(albums_of_artist(&state.db, &artist).await?) },
        async { Ok(chosen_cover(&state.db, &artist).await?) },
        async { Ok(fallback_covers(&state.db).await?) },
    )?;
    let has_custom_image = cover.as_ref().is_some_and(|c| c.image_path.is_some());
    let effective = effective_album(cover.as_ref(), &fallback, &artist);
    let selected = cover.as_ref().and_then(|c| c.item_id);
    let page = ArtistPage {
        shell,
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
