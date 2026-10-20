//! The server-rendered web UI (German). Handlers call the library modules directly, not the API.

pub mod albums;
pub mod artists;
pub mod assets;
pub mod books;
pub mod duplicates;
pub mod errors;
pub mod favorites;
pub mod format;
pub mod home;
pub mod layout;
pub mod login;
pub mod metadata;
pub mod player;
pub mod playlists;
pub mod podcasts;
pub mod scan_card;
pub mod search;
pub mod series;
pub mod settings;
pub mod users;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(home::router())
        .merge(login::router())
        .merge(books::router())
        .merge(albums::router())
        .merge(artists::router())
        .merge(series::router())
        .merge(favorites::router())
        .merge(playlists::router())
        .merge(podcasts::router())
        .merge(search::router())
        .merge(player::router())
        .merge(settings::router())
        .merge(duplicates::router())
        .merge(users::router())
}
