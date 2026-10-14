//! The JSON API for native clients. Paths, status codes, snake_case fields and German `detail`
//! texts are wire-compatible with the legacy server.

pub mod auth;
pub mod items;
pub mod me;
pub mod scan;
pub mod server;
pub mod validate;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(auth::router())
        .merge(items::router())
        .merge(scan::router())
        .merge(me::router())
        .merge(server::router())
}
