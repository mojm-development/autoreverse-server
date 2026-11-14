//! Integration tests: route handlers through the real router (`tower::ServiceExt::oneshot`) against
//! a throwaway Postgres database per test (`#[sqlx::test]`, migrations applied). One test binary,
//! so the crate links once; each area is a module.

mod common;

mod ai_api;
mod api_edges;
mod artist_covers;
mod artist_info;
mod auth_api;
mod bootstrap;
mod collections_api;
mod devices;
mod discs;
mod duplicates;
mod items_api;
mod library_paths;
mod loudness;
mod lyrics;
mod metadata_api;
mod music;
mod passwords;
mod playback_api;
mod playlists_api;
mod playlists_sharing;
mod podcasts;
mod queries;
mod radio_api;
mod scan_api;
mod scan_store;
mod scanner_folders;
mod security;
mod server_info;
mod startup;
mod stations;
mod tokens;
mod top_api;
mod users_api;
mod web_auth;
mod web_collections;
mod web_library;
mod web_metadata;
mod web_mobile_nav;
mod web_music;
mod web_playback;
mod web_playlists;
mod web_podcasts;
mod web_users;
