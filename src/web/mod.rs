//! The server-rendered web UI (German). Handlers call the library modules directly, not the API.

pub mod albums;
pub mod artists;
pub mod books;
pub mod format;
pub mod home;
pub mod layout;
pub mod login;
pub mod player;
pub mod series;
pub mod settings;

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
        .merge(player::router())
        .merge(settings::router())
}
