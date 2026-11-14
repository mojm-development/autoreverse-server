//! Checking an address before it becomes a station: is it reachable, and is it audio?
//!
//! Stream addresses come from users and from a public directory, so every one of them is
//! somebody else's word. They are fetched through the fenced client of `podcasts::net` only,
//! which reaches no private, loopback or link-local address and refuses to be redirected to
//! one — otherwise "add a station" would be a way to make the server read the router's admin
//! page. The one exception is the host the operator configured as the directory: tests and a
//! mirror in the local network serve their streams from there.
//!
//! Playlist files (.m3u, .pls) are followed to the address they name. HLS (.m3u8 with
//! `#EXT-X-…` tags) is refused: it is not one stream but a playlist of ever new segment files,
//! often on other hosts. Passing it on would mean rewriting every playlist and fetching
//! whatever address a playlist names on a client's request — an open proxy to guard — and it
//! carries no ICY titles. Nearly every station offers an MP3 or AAC stream as well, and those
//! are what every client plays natively.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Url;
use reqwest::header::CONTENT_TYPE;

use super::fetch::{self, Answer, Failed};
use super::logos::{self, Picture};
use crate::podcasts::net::is_public;
use crate::state::AppState;

pub const NOT_HTTP: &str = "Die Adresse muss mit http:// oder https:// beginnen.";
pub const LOCAL_ADDRESS: &str =
    "Adressen im lokalen Netz können nicht als Sender hinzugefügt werden.";
pub const UNREACHABLE: &str = "Der Sender ist unter dieser Adresse nicht erreichbar.";
pub const NO_AUDIO: &str = "Unter dieser Adresse gibt es keinen Audio-Stream. Bitte die direkte Stream-Adresse des Senders angeben.";
pub const NO_DATA: &str = "Der Sender antwortet, liefert aber keinen Ton.";
pub const HLS_UNSUPPORTED: &str = "HLS-Sender (.m3u8) werden noch nicht unterstützt. Bitte die MP3- oder AAC-Adresse des Senders verwenden.";
pub const EMPTY_PLAYLIST: &str = "Die Playlist-Datei nennt keine Stream-Adresse.";
pub const TOO_MANY_HOPS: &str = "Die Adresse verweist zu oft auf weitere Playlist-Dateien.";

pub const USER_AGENT: &str = concat!("Autoreverse/", env!("CARGO_PKG_VERSION"));
/// How long a sender may take to answer with its headers — when adding and when playing.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const FIRST_BYTES_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_PLAYLIST_BYTES: usize = 64 * 1024;
/// A playlist naming a playlist naming a stream happens; more than that is a loop.
const MAX_HOPS: usize = 4;
pub const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;

/// An `http(s)` address with a host, or why not.
pub fn parse_url(raw: &str) -> Result<Url, &'static str> {
    Url::parse(raw.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .ok_or(NOT_HTTP)
}

/// The URL's host as an IP address, if it is written as one.
fn host_ip(url: &Url) -> Option<IpAddr> {
    url.host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

/// Whether `url` is on the very host and port the directory was configured to be, and that one
/// is a local address written as such. Only then is the fence lowered.
fn is_local_directory_host(url: &Url, directory_url: &str) -> bool {
    Url::parse(directory_url).is_ok_and(|base| {
        host_ip(&base).is_some_and(|ip| !is_public(ip))
            && base.host_str() == url.host_str()
            && base.port_or_known_default() == url.port_or_known_default()
    })
}

/// The client to fetch `url` with: the fenced one, except for the configured local directory
/// host. An address in the local network written as a number never reaches the fenced client's
/// resolver, so it is refused here.
pub fn client_for<'a>(state: &'a AppState, url: &Url) -> Result<&'a reqwest::Client, &'static str> {
    if is_local_directory_host(url, &state.config.radio_directory_url) {
        return Ok(&state.http);
    }
    if host_ip(url).is_some_and(|ip| !is_public(ip)) {
        return Err(LOCAL_ADDRESS);
    }
    Ok(state.http.public())
}

/// What kind of thing answered.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// A stream, with the media type to pass on.
    Audio(String),
    M3u,
    Pls,
    Other,
}

const M3U_TYPES: &[&str] = &[
    "audio/x-mpegurl",
    "audio/mpegurl",
    "application/x-mpegurl",
    "application/mpegurl",
    "application/vnd.apple.mpegurl",
];
const PLS_TYPES: &[&str] = &[
    "audio/x-scpls",
    "audio/scpls",
    "application/pls+xml",
    "application/x-scpls",
];
/// Types that say nothing about the content; then the first bytes and the file name decide.
const VAGUE_TYPES: &[&str] = &[
    "",
    "application/octet-stream",
    "binary/octet-stream",
    "text/plain",
];

/// Whether a media type names a playlist file rather than a stream — some of them begin with
/// `audio/` all the same.
pub fn is_playlist_type(media_type: &str) -> bool {
    M3U_TYPES.contains(&media_type) || PLS_TYPES.contains(&media_type)
}

/// The media type without parameters, lower-cased.
pub fn media_type(header: Option<&str>) -> String {
    header
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// The audio format the first bytes of a body are in, by their signature.
fn sniff_audio(first: &[u8]) -> Option<&'static str> {
    match first {
        [b'I', b'D', b'3', ..] => Some("audio/mpeg"),
        [b'O', b'g', b'g', b'S', ..] => Some("audio/ogg"),
        [b'f', b'L', b'a', b'C', ..] => Some("audio/flac"),
        // A frame sync word: ADTS (AAC) has the layer bits at zero, MPEG audio has not.
        [0xff, second, ..] if second & 0xf6 == 0xf0 => Some("audio/aac"),
        [0xff, second, ..] if second & 0xe0 == 0xe0 => Some("audio/mpeg"),
        _ => None,
    }
}

fn classify(declared: &str, path: &str, first: &[u8]) -> Kind {
    let start = String::from_utf8_lossy(&first[..first.len().min(64)]).to_lowercase();
    let start = start.trim_start_matches('\u{feff}').trim_start();
    let path = path.to_ascii_lowercase();
    // What the body says it is counts for more than its label: playlists are served under every
    // media type there is.
    if start.starts_with("[playlist]") || PLS_TYPES.contains(&declared) {
        return Kind::Pls;
    }
    if start.starts_with("#extm3u") || M3U_TYPES.contains(&declared) {
        return Kind::M3u;
    }
    if declared.starts_with("audio/") || declared == "application/ogg" {
        return Kind::Audio(declared.to_owned());
    }
    if !VAGUE_TYPES.contains(&declared) {
        return Kind::Other;
    }
    if path.ends_with(".pls") {
        return Kind::Pls;
    }
    if path.ends_with(".m3u") || path.ends_with(".m3u8") {
        return Kind::M3u;
    }
    sniff_audio(first).map_or(Kind::Other, |found| Kind::Audio(found.to_owned()))
}

/// Whether an m3u body is an HLS playlist rather than a list of stream addresses.
pub fn is_hls(body: &str) -> bool {
    body.lines()
        .any(|line| line.trim_start().starts_with("#EXT-X-"))
}

/// The first address an m3u playlist names.
pub fn m3u_entry(body: &str) -> Option<&str> {
    body.lines()
        .map(|line| line.trim_start_matches('\u{feff}').trim())
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

/// The first `FileN=` address of a pls playlist.
pub fn pls_entry(body: &str) -> Option<&str> {
    body.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        let key = key.trim().to_ascii_lowercase();
        let number = key.strip_prefix("file")?;
        (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
            .then(|| value.trim())
            .filter(|value| !value.is_empty())
    })
}

/// A station's codec as clients show it, from the media type of its stream.
pub fn codec_of(content_type: &str) -> Option<&'static str> {
    match content_type {
        "audio/mpeg" | "audio/mp3" | "audio/x-mpeg" => Some("MP3"),
        "audio/aac" | "audio/aacp" | "audio/x-aac" | "audio/mp4" => Some("AAC"),
        "audio/ogg" | "application/ogg" | "audio/vorbis" => Some("OGG"),
        "audio/opus" => Some("OPUS"),
        "audio/flac" | "audio/x-flac" => Some("FLAC"),
        _ => None,
    }
}

/// An address that was checked: it answers, and with audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    /// The address that delivers the audio — behind any playlist file, before any redirect (a
    /// redirect often leads to an address that is valid for one listener and a few minutes).
    pub stream_url: String,
    pub content_type: String,
    /// kbit/s, where the sender says so (`icy-br`).
    pub bitrate: Option<i32>,
}

/// The rest of a body after `first`, up to `max` bytes, within `wait`.
async fn read_rest(
    response: &mut Answer,
    mut body: Vec<u8>,
    max: usize,
    wait: Duration,
) -> Vec<u8> {
    let rest = async {
        while body.len() < max {
            match response.chunk().await {
                Ok(Some(chunk)) => body.extend_from_slice(&chunk),
                _ => break,
            }
        }
    };
    // Whatever arrived in time is what there is; a playlist is a few hundred bytes.
    let _ = tokio::time::timeout(wait, rest).await;
    body.truncate(max);
    body
}

/// Follows `raw` to an audio stream: through playlist files, with a short look at the headers
/// and the first bytes of each answer. The error is a German sentence for the user.
pub async fn probe(state: &AppState, raw: &str) -> Result<Probed, &'static str> {
    let mut url = parse_url(raw)?;
    for _ in 0..MAX_HOPS {
        let mut response = match fetch::get(state, &url, false).await {
            Ok(response) => response,
            Err(Failed::Refused(why)) => return Err(why),
            Err(Failed::Status(status)) => {
                tracing::info!("[radio] {url}: HTTP {status}");
                return Err(UNREACHABLE);
            }
            Err(Failed::Error(e)) => {
                tracing::info!("[radio] {url}: {e}");
                return Err(UNREACHABLE);
            }
        };
        let header = |name: &str| response.header(name).map(str::to_owned);
        let declared = media_type(header(CONTENT_TYPE.as_str()).as_deref());
        let bitrate = header("icy-br")
            .and_then(|raw| raw.split(',').next()?.trim().parse::<i32>().ok())
            .filter(|kbps| (1..=10_000).contains(kbps));
        // Playlists are relative to where they were really served from.
        let base = response.url().clone();
        let first = match tokio::time::timeout(FIRST_BYTES_TIMEOUT, response.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk.to_vec(),
            _ => Vec::new(),
        };
        let kind = classify(&declared, base.path(), &first);
        let next = match &kind {
            Kind::Audio(_) if first.is_empty() => return Err(NO_DATA),
            Kind::Audio(content_type) => {
                return Ok(Probed {
                    stream_url: url.to_string(),
                    content_type: content_type.clone(),
                    bitrate,
                });
            }
            Kind::Other => return Err(NO_AUDIO),
            Kind::M3u | Kind::Pls => {
                let body = read_rest(
                    &mut response,
                    first,
                    MAX_PLAYLIST_BYTES,
                    FIRST_BYTES_TIMEOUT,
                )
                .await;
                let body = String::from_utf8_lossy(&body);
                if kind == Kind::M3u && is_hls(&body) {
                    return Err(HLS_UNSUPPORTED);
                }
                let entry = if kind == Kind::M3u {
                    m3u_entry(&body)
                } else {
                    pls_entry(&body)
                };
                let entry = entry.ok_or(EMPTY_PLAYLIST)?;
                base.join(entry).map_err(|_| EMPTY_PLAYLIST)?
            }
        };
        if !matches!(next.scheme(), "http" | "https") || next.host_str().is_none() {
            return Err(EMPTY_PLAYLIST);
        }
        url = next;
    }
    Err(TOO_MANY_HOPS)
}

/// A logo from `raw`: at most [`MAX_LOGO_BYTES`], fetched within `timeout`, and a picture by
/// its content (`logos::normalize`), whatever the sender calls it. `None` for anything else; a
/// station without a logo is still a station.
pub async fn fetch_logo(state: &AppState, raw: &str, timeout: Duration) -> Option<Picture> {
    let url = parse_url(raw).ok()?;
    let client = client_for(state, &url).ok()?;
    let mut response = client
        .get(url)
        // The whole answer, body included: a server that trickles is cut off as well.
        .timeout(timeout)
        .header("user-agent", USER_AGENT)
        .send()
        .await
        .ok()
        .filter(|response| response.status().is_success())?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LOGO_BYTES as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_LOGO_BYTES {
            return None;
        }
    }
    tokio::task::spawn_blocking(move || logos::normalize(bytes))
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_addresses_with_a_host_are_addresses() {
        assert!(parse_url(" https://stream.example.com/live.mp3 ").is_ok());
        assert!(parse_url("http://93.184.216.34:8000/").is_ok());
        for bad in [
            "",
            "stream.example.com",
            "ftp://x/a",
            "file:///etc/passwd",
            "http://",
            "javascript:1",
        ] {
            assert_eq!(parse_url(bad), Err(NOT_HTTP), "{bad}");
        }
    }

    #[test]
    fn the_fence_is_lowered_only_for_a_local_directory_host() {
        let url = |raw: &str| Url::parse(raw).unwrap();
        let local = "http://127.0.0.1:4000/";
        assert!(is_local_directory_host(
            &url("http://127.0.0.1:4000/stream"),
            local
        ));
        assert!(!is_local_directory_host(
            &url("http://127.0.0.1:4001/stream"),
            local
        ));
        assert!(!is_local_directory_host(
            &url("http://192.168.1.1:4000/"),
            local
        ));
        // The real directory is public: its host gets no special treatment, and nothing local
        // slips through by naming it.
        let public = "https://all.api.radio-browser.info";
        assert!(!is_local_directory_host(
            &url("https://all.api.radio-browser.info/x"),
            public
        ));
        assert!(!is_local_directory_host(&url("http://127.0.0.1/x"), public));
        assert!(!is_local_directory_host(
            &url("http://127.0.0.1/x"),
            "not a url"
        ));
    }

    #[test]
    fn answers_are_told_apart_by_content_before_label() {
        let audio = |t: &str| Kind::Audio(t.to_owned());
        assert_eq!(
            classify("audio/mpeg", "/live", b"\xff\xfb\x90"),
            audio("audio/mpeg")
        );
        assert_eq!(
            classify("audio/aacp", "/live", b"\xff\xf1"),
            audio("audio/aacp")
        );
        assert_eq!(
            classify("application/ogg", "/live", b"OggS"),
            audio("application/ogg")
        );
        // Playlists under their own types, under a vague one, and under a wrong one.
        assert_eq!(
            classify("audio/x-mpegurl", "/a", b"http://x/s\n"),
            Kind::M3u
        );
        assert_eq!(classify("audio/x-scpls", "/a", b"[playlist]\n"), Kind::Pls);
        assert_eq!(
            classify("text/plain", "/listen.m3u", b"http://x/s\n"),
            Kind::M3u
        );
        assert_eq!(classify("", "/listen.PLS", b"NumberOfEntries=1"), Kind::Pls);
        assert_eq!(
            classify("audio/mpeg", "/a", b"\xef\xbb\xbf#EXTM3U\n"),
            Kind::M3u
        );
        assert_eq!(
            classify("text/html", "/a", b" [Playlist]\nFile1=x"),
            Kind::Pls
        );
        assert_eq!(
            classify("application/vnd.apple.mpegurl", "/a.m3u8", b"#EXTM3U"),
            Kind::M3u
        );
        // No label: the signature decides.
        assert_eq!(classify("", "/live", b"ID3\x04"), audio("audio/mpeg"));
        assert_eq!(
            classify("application/octet-stream", "/live", b"\xff\xf1\x50"),
            audio("audio/aac")
        );
        assert_eq!(
            classify("application/octet-stream", "/live", b"\xff\xfb\x90"),
            audio("audio/mpeg")
        );
        assert_eq!(classify("", "/live", b"fLaC"), audio("audio/flac"));
        assert_eq!(classify("", "/live", b"<!doctype html>"), Kind::Other);
        assert_eq!(classify("text/html", "/live.mp3", b"<html>"), Kind::Other);
        assert_eq!(classify("video/mp2t", "/seg.ts", b"G@"), Kind::Other);
    }

    #[test]
    fn playlists_name_their_first_address() {
        let m3u = "\u{feff}#EXTM3U\r\n#EXTINF:-1,Sender\r\n\r\nhttp://x/one\r\nhttp://x/two\r\n";
        assert_eq!(m3u_entry(m3u), Some("http://x/one"));
        assert_eq!(m3u_entry("#EXTM3U\n# nothing\n"), None);
        assert!(!is_hls(m3u));
        assert!(is_hls(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\nseg1.ts\n"
        ));
        assert!(is_hls(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=96000\nlow/index.m3u8\n"
        ));

        let pls = "[playlist]\nNumberOfEntries=2\nTitle1=Sender\nfile1 = http://x/one \nFile2=http://x/two\n";
        assert_eq!(pls_entry(pls), Some("http://x/one"));
        assert_eq!(pls_entry("[playlist]\nFile1=\nFilename=x\n"), None);
    }

    #[test]
    fn the_codec_follows_the_media_type() {
        assert_eq!(codec_of("audio/mpeg"), Some("MP3"));
        assert_eq!(codec_of("audio/aacp"), Some("AAC"));
        assert_eq!(codec_of("application/ogg"), Some("OGG"));
        assert_eq!(codec_of("audio/x-something"), None);
        assert_eq!(media_type(Some("Audio/MPEG; charset=utf-8")), "audio/mpeg");
        assert_eq!(media_type(None), "");
    }
}
