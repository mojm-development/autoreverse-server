//! Integration tests: route handlers through the real router (`tower::ServiceExt::oneshot`) against
//! a throwaway Postgres database per test (`#[sqlx::test]`, migrations applied). One test binary,
//! so the crate links once; each area is a module.

mod common;

mod auth_api;
mod bootstrap;
mod collections_api;
mod items_api;
mod library_paths;
mod music;
mod passwords;
mod playback_api;
mod playlists_api;
mod queries;
mod scan_api;
mod scan_store;
mod scanner_folders;
mod server_info;
mod tokens;
mod web_auth;
mod web_collections;
mod web_library;
mod web_music;
mod web_playback;
mod web_playlists;
