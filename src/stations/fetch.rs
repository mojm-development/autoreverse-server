//! One GET to an address a station names, for the probe when adding and for the live stream:
//! through the fenced client ([`super::probe::client_for`]), and for a plain `http://` address
//! the client could not read, once more as SHOUTcast v1 ([`super::icy_v1`]).

use axum::body::Bytes;
use reqwest::{StatusCode, Url};

use super::icy_v1::{self, IcyAnswer, NotIcy};
use super::probe::{CONNECT_TIMEOUT, USER_AGENT, client_for};
use crate::state::AppState;

/// A sender's answer, from either client.
pub enum Answer {
    Http(reqwest::Response),
    Icy(IcyAnswer),
}

impl Answer {
    /// Where the answer really came from (after redirects): playlists are relative to it.
    pub fn url(&self) -> &Url {
        match self {
            Answer::Http(response) => response.url(),
            Answer::Icy(answer) => answer.url(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        match self {
            Answer::Http(response) => response.headers().get(name)?.to_str().ok(),
            Answer::Icy(answer) => answer.header(name),
        }
    }

    /// The next piece of the body; `None` at its end.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, String> {
        match self {
            Answer::Http(response) => response.chunk().await.map_err(|e| e.to_string()),
            Answer::Icy(answer) => answer.chunk().await.map_err(|e| e.to_string()),
        }
    }
}

/// Why there is no answer to pass on.
#[derive(Debug)]
pub enum Failed {
    /// The address is refused before anything is sent (a local one), with the German sentence.
    Refused(&'static str),
    /// The sender answered with an error status.
    Status(StatusCode),
    /// Unreachable, or nothing in time.
    Error(String),
}

/// GETs `url` within [`CONNECT_TIMEOUT`] for the headers. `icy_metadata` asks the sender to
/// weave in its titles.
pub async fn get(state: &AppState, url: &Url, icy_metadata: bool) -> Result<Answer, Failed> {
    let client = client_for(state, url).map_err(Failed::Refused)?;
    let fenced = std::ptr::eq(client, state.http.public());
    let mut request = client.get(url.clone()).header("user-agent", USER_AGENT);
    if icy_metadata {
        request = request.header("icy-metadata", "1");
    }
    let started = tokio::time::Instant::now();
    let error = match tokio::time::timeout(CONNECT_TIMEOUT, request.send()).await {
        Ok(Ok(response)) if response.status().is_success() => return Ok(Answer::Http(response)),
        Ok(Ok(response)) => return Err(Failed::Status(response.status())),
        Ok(Err(e)) => e,
        Err(_) => return Err(Failed::Error("keine Antwort".into())),
    };
    // The HTTP client refuses an `ICY 200 OK` status line. Only a plain address can be a v1
    // sender, and only an answer that was there but unreadable is worth a second connection —
    // a host that cannot be reached at all will not be reachable the second time either.
    if url.scheme() != "http" || error.is_connect() || error.is_timeout() {
        return Err(Failed::Error(error.to_string()));
    }
    let left = CONNECT_TIMEOUT.saturating_sub(started.elapsed());
    match tokio::time::timeout(left, icy_v1::get(url, fenced, USER_AGENT, icy_metadata)).await {
        Ok(Ok(answer)) => Ok(Answer::Icy(answer)),
        Ok(Err(NotIcy::Local)) => Err(Failed::Refused(super::probe::LOCAL_ADDRESS)),
        _ => Err(Failed::Error(error.to_string())),
    }
}
