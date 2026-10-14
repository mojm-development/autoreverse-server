//! The server-rendered web UI (German). Handlers call the library modules directly, not the API.

pub mod home;
pub mod layout;
pub mod login;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().merge(home::router()).merge(login::router())
}
