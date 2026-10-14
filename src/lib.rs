//! Autoreverse Server: a self-hosted media server for audiobooks, music and podcasts.
//!
//! One process serves the JSON API for the native clients and the server-rendered web UI.

pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod library;
pub mod serialize;
pub mod state;
pub mod web;

pub use app::app;
pub use config::Config;
pub use state::AppState;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
