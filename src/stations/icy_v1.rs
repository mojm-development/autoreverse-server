//! SHOUTcast v1: senders that answer `ICY 200 OK` instead of `HTTP/1.x 200 OK`.
//!
//! The rest of such an answer is HTTP as usual — header lines, an empty line, then the stream
//! (with ICY metadata woven in when asked for) — but the HTTP client refuses the status line
//! outright. These senders are old, but plenty of small stations still run them, so a plain
//! `http://` address that the client could not read gets one more try here
//! ([`super::fetch::get`]).
//!
//! This is no general HTTP client and is not meant to become one: one GET, no redirects, no
//! TLS (a v1 server has none), no chunked encoding (a v1 server sends none). It goes through
//! the same fence as everything else a station names: an address in the local network is
//! refused unless the caller lowered the fence ([`super::probe::client_for`]'s one exception).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Bytes;
use reqwest::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::podcasts::net::is_public;

/// The header block of an answer is a few hundred bytes; more is not a sender.
const MAX_HEAD_BYTES: usize = 16 * 1024;
/// Like the HTTP client's read timeout: a sender silent for this long has gone.
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const CHUNK_BYTES: usize = 16 * 1024;

/// A v1 sender's answer: its headers (names lower-cased) and the open connection.
pub struct IcyAnswer {
    url: Url,
    headers: HashMap<String, String>,
    /// What came in after the empty line together with the headers.
    pending: Option<Bytes>,
    stream: TcpStream,
}

impl IcyAnswer {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// The next piece of the stream; `None` when the sender closed it.
    pub async fn chunk(&mut self) -> std::io::Result<Option<Bytes>> {
        if let Some(pending) = self.pending.take() {
            return Ok(Some(pending));
        }
        let mut buffer = vec![0u8; CHUNK_BYTES];
        let read = tokio::time::timeout(READ_TIMEOUT, self.stream.read(&mut buffer))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "keine Daten"))??;
        if read == 0 {
            return Ok(None);
        }
        buffer.truncate(read);
        Ok(Some(Bytes::from(buffer)))
    }
}

/// Why there is no v1 answer. The caller reports its own error then; this one only tells
/// whether trying was worth it.
#[derive(Debug, PartialEq, Eq)]
pub enum NotIcy {
    /// The host resolves only into the local network, and the fence is up.
    Local,
    /// Unreachable, or it answered something other than `ICY 200`.
    Other(String),
}

/// The addresses to try for `url`: all of them with the fence down, the public ones otherwise.
async fn addresses(url: &Url, fenced: bool) -> Result<Vec<SocketAddr>, NotIcy> {
    let host = url
        .host_str()
        .ok_or_else(|| NotIcy::Other("kein Host".into()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(80);
    let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| NotIcy::Other(e.to_string()))?
        .collect();
    let allowed: Vec<SocketAddr> = found
        .into_iter()
        .filter(|addr| !fenced || is_public(addr.ip()))
        .collect();
    if allowed.is_empty() {
        return Err(NotIcy::Local);
    }
    Ok(allowed)
}

/// The request line and headers, as a v1 server expects them. HTTP/1.0: no chunked answer,
/// no keep-alive.
fn request(url: &Url, user_agent: &str, icy_metadata: bool) -> String {
    let mut target = url.path().to_owned();
    if target.is_empty() {
        target.push('/');
    }
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_owned(),
    };
    let mut text = format!(
        "GET {target} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: {user_agent}\r\nAccept: */*\r\n"
    );
    if icy_metadata {
        text.push_str("Icy-MetaData: 1\r\n");
    }
    text.push_str("\r\n");
    text
}

/// Splits an answer's head into its status line and headers. `None` when the status line is
/// not `ICY 200`.
fn parse_head(head: &str) -> Option<HashMap<String, String>> {
    let mut lines = head.split("\r\n").flat_map(|line| line.split('\n'));
    let status = lines.next()?.trim();
    let mut parts = status.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("ICY") || parts.next()? != "200" {
        return None;
    }
    Some(
        lines
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            })
            .collect(),
    )
}

/// Where the head ends: the index after the empty line (`\r\n\r\n`, or `\n\n` from servers
/// that are sloppier still).
fn head_end(data: &[u8]) -> Option<usize> {
    let crlf = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4);
    let lf = data.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// One GET to `url`, which must be `http://`. Ok only for an `ICY 200` answer.
pub async fn get(
    url: &Url,
    fenced: bool,
    user_agent: &str,
    icy_metadata: bool,
) -> Result<IcyAnswer, NotIcy> {
    if url.scheme() != "http" {
        return Err(NotIcy::Other("nur http://".into()));
    }
    let addresses = addresses(url, fenced).await?;
    let mut stream = TcpStream::connect(addresses.as_slice())
        .await
        .map_err(|e| NotIcy::Other(e.to_string()))?;
    stream
        .write_all(request(url, user_agent, icy_metadata).as_bytes())
        .await
        .map_err(|e| NotIcy::Other(e.to_string()))?;

    let mut data = Vec::new();
    let mut buffer = [0u8; 2048];
    let end = loop {
        if let Some(end) = head_end(&data) {
            break end;
        }
        if data.len() > MAX_HEAD_BYTES {
            return Err(NotIcy::Other("Kopfzeilen zu lang".into()));
        }
        let read = tokio::time::timeout(READ_TIMEOUT, stream.read(&mut buffer))
            .await
            .map_err(|_| NotIcy::Other("keine Antwort".into()))?
            .map_err(|e| NotIcy::Other(e.to_string()))?;
        if read == 0 {
            return Err(NotIcy::Other("Verbindung beendet".into()));
        }
        data.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&data[..end]);
    let headers = parse_head(&head).ok_or_else(|| {
        let status = head.lines().next().unwrap_or_default();
        NotIcy::Other(format!("keine ICY-Antwort: {status}"))
    })?;
    let rest = data.split_off(end);
    Ok(IcyAnswer {
        url: url.clone(),
        headers,
        pending: (!rest.is_empty()).then(|| Bytes::from(rest)),
        stream,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_v1_head_and_nothing_else() {
        let head = "ICY 200 OK\r\nicy-name:Jazz Eins\r\nContent-Type: audio/mpeg\r\nicy-metaint:8192\r\nicy-br:128\r\n\r\n";
        let headers = parse_head(head).unwrap();
        assert_eq!(headers["content-type"], "audio/mpeg");
        assert_eq!(headers["icy-metaint"], "8192");
        assert_eq!(headers["icy-name"], "Jazz Eins");
        assert_eq!(parse_head("ICY 404 Resource Not Found\r\n\r\n"), None);
        assert_eq!(parse_head("HTTP/1.0 200 OK\r\n\r\n"), None);
        // Bare line feeds, as some servers send them.
        assert!(parse_head("ICY 200 OK\nicy-br:64\n\n").is_some());
    }

    #[test]
    fn finds_where_the_head_ends() {
        assert_eq!(head_end(b"ICY 200 OK\r\n\r\nabc"), Some(14));
        assert_eq!(head_end(b"ICY 200 OK\n\nabc"), Some(12));
        assert_eq!(head_end(b"ICY 200 OK\r\nicy-br:1"), None);
    }

    #[test]
    fn asks_like_a_v1_client() {
        let url = Url::parse("http://radio.example:8000/;stream.mp3?x=1").unwrap();
        assert_eq!(
            request(&url, "Autoreverse/1", true),
            "GET /;stream.mp3?x=1 HTTP/1.0\r\nHost: radio.example:8000\r\nUser-Agent: Autoreverse/1\r\nAccept: */*\r\nIcy-MetaData: 1\r\n\r\n"
        );
        let bare = Url::parse("http://radio.example").unwrap();
        assert!(
            request(&bare, "A", false).starts_with("GET / HTTP/1.0\r\nHost: radio.example\r\n")
        );
    }

    #[tokio::test]
    async fn the_fence_holds_here_too() {
        let url = Url::parse("http://127.0.0.1:9/").unwrap();
        assert_eq!(get(&url, true, "A", false).await.err(), Some(NotIcy::Local));
    }
}
