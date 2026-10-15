//! Podcasts: subscribing to feeds, keeping them fresh, downloading and pruning episodes, and the
//! iTunes directory search (legacy `lib/server/podcasts/**`).

pub mod chapters;
pub mod directory;
pub mod download;
pub mod feed;
pub mod job;
pub mod paths;
pub mod relocate;
pub mod retention;
pub mod store;

use std::time::Duration;

/// The client for every outgoing request (feeds, artwork, episodes, chapters, directory). Redirects
/// are followed as `fetch` did. Per-request timeouts sit on the requests; the read timeout here
/// is what bounds an episode download — it may take long, but must not stall.
pub fn http_client() -> reqwest::Client {
    // ring, the provider sqlx already brings; installing twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(concat!("Autoreverse/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(20))
        .build()
        .expect("HTTP client")
}
