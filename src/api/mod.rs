//! The JSON API for native clients. Paths, status codes, snake_case fields and German `detail`
//! texts are wire-compatible with the legacy server.

pub mod artists;
pub mod auth;
pub mod bookmarks;
pub mod favorites;
pub mod images;
pub mod items;
pub mod me;
pub mod playback;
pub mod playlists;
pub mod podcasts;
pub mod scan;
pub mod search;
pub mod server;
pub mod tracks;
pub mod validate;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(auth::router())
        .merge(items::router())
        .merge(tracks::router())
        .merge(artists::router())
        .merge(scan::router())
        .merge(favorites::router())
        .merge(bookmarks::router())
        .merge(playlists::router())
        .merge(podcasts::router())
        .merge(search::router())
        .merge(me::router())
        .merge(playback::router())
        .merge(server::router())
}
