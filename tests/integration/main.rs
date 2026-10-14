//! Integration tests: route handlers through the real router (`tower::ServiceExt::oneshot`) against
//! a throwaway Postgres database per test (`#[sqlx::test]`, migrations applied). One test binary,
//! so the crate links once; each area is a module.

mod common;

mod auth_api;
mod bootstrap;
mod passwords;
mod server_info;
mod tokens;
mod web_auth;
