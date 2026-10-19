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

/// The body as text, or `None` once it grows past `max_bytes` — a feed or chapter file is read
/// into memory whole, so a hostile or broken server must not be able to make it unbounded.
pub async fn text_capped(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Option<String>, reqwest::Error> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > max_bytes {
            return Ok(None);
        }
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}
