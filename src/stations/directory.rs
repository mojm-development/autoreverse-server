//! The station directory: radio-browser.info, a free community catalogue that needs no account.
//!
//! Only the search words (and a country code) are sent there, and only when a user searches;
//! `AUTOREVERSE_RADIO_DIRECTORY=0` keeps the server from asking at all. The directory's servers
//! share one host name that resolves to all of them, so the default address needs no server
//! list of its own; `AUTOREVERSE_RADIO_DIRECTORY_URL` names a particular server or a mirror.

use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

/// All servers of radio-browser.info behind one name (DNS round robin).
pub const DIRECTORY_URL: &str = "https://all.api.radio-browser.info";
pub const DEFAULT_LIMIT: i64 = 25;
pub const MAX_LIMIT: i64 = 50;
pub const UNREACHABLE: &str = "Senderverzeichnis nicht erreichbar";
pub const UNEXPECTED: &str = "Unerwartete Antwort des Senderverzeichnisses";

const TIMEOUT: Duration = Duration::from_secs(12);
/// Tags are free text in the directory, some stations carry dozens.
const MAX_TAGS: usize = 8;
const MAX_TAG_CHARS: usize = 40;

/// A station as the directory describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryStation {
    pub directory_id: String,
    pub name: String,
    /// The address to play: the directory's own resolution of playlists and redirects where it
    /// has one, else what was entered there.
    pub stream_url: String,
    pub homepage: Option<String>,
    pub country: Option<String>,
    pub tags: Vec<String>,
    pub codec: Option<String>,
    pub bitrate: Option<i32>,
    pub logo_url: Option<String>,
    pub hls: bool,
}

impl DirectoryStation {
    /// Whether it is worth asking for this station's logo: the directory names one, or it
    /// names a homepage whose icon can stand in (`site_icon`). Neither has been fetched when
    /// this is said — the search must not wait for dozens of other servers — so a client that
    /// asks may still get a 404.
    pub fn may_have_logo(&self) -> bool {
        self.logo_url.is_some() || self.homepage.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DirectoryError(pub String);

fn text<'a>(entry: &'a Value, key: &str) -> Option<&'a str> {
    entry[key].as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// An `http(s)` address, or nothing: a homepage or logo is shown and fetched, never trusted.
fn web_url(entry: &Value, key: &str) -> Option<String> {
    let raw = text(entry, key)?;
    Url::parse(raw)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .map(|_| raw.to_owned())
}

fn flag(entry: &Value, key: &str) -> bool {
    entry[key].as_i64() == Some(1) || entry[key].as_bool() == Some(true)
}

/// A two-letter country code in upper case; the directory has stations without one.
pub fn country_code(raw: &str) -> Option<String> {
    let code = raw.trim();
    (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| code.to_ascii_uppercase())
}

fn tags(raw: Option<&str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    raw.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty() && tag.chars().count() <= MAX_TAG_CHARS)
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .take(MAX_TAGS)
        .map(str::to_owned)
        .collect()
}

/// One directory entry; `None` for what cannot be played from here: no id, no name, no usable
/// address, or a station whose last check by the directory failed.
fn parse_station(entry: &Value) -> Option<DirectoryStation> {
    let stream_url = web_url(entry, "url_resolved").or_else(|| web_url(entry, "url"))?;
    // A missing field counts as "fine": only a station the directory says is down is dropped.
    if entry.get("lastcheckok").is_some() && !flag(entry, "lastcheckok") {
        return None;
    }
    Some(DirectoryStation {
        directory_id: text(entry, "stationuuid")?.to_owned(),
        name: text(entry, "name")?.to_owned(),
        stream_url,
        homepage: web_url(entry, "homepage"),
        country: text(entry, "countrycode").and_then(country_code),
        tags: tags(text(entry, "tags")),
        codec: text(entry, "codec")
            .filter(|codec| !codec.eq_ignore_ascii_case("unknown"))
            .map(str::to_owned),
        bitrate: entry["bitrate"]
            .as_i64()
            .and_then(|kbps| i32::try_from(kbps).ok())
            .filter(|kbps| *kbps > 0),
        logo_url: web_url(entry, "favicon"),
        hls: flag(entry, "hls"),
    })
}

/// The stations in a directory answer (a JSON array), unplayable ones left out.
pub fn parse_stations(body: &str) -> Result<Vec<DirectoryStation>, DirectoryError> {
    let unexpected = || DirectoryError(UNEXPECTED.into());
    let parsed: Value = serde_json::from_str(body).map_err(|_| unexpected())?;
    let entries = parsed.as_array().ok_or_else(unexpected)?;
    Ok(entries.iter().filter_map(parse_station).collect())
}

async fn get(
    http: &reqwest::Client,
    url: String,
    query: &[(&str, &str)],
) -> Result<Vec<DirectoryStation>, DirectoryError> {
    let unreachable = || DirectoryError(UNREACHABLE.into());
    let response = http
        .get(url)
        .query(query)
        .timeout(TIMEOUT)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|_| unreachable())?;
    if !response.status().is_success() {
        return Err(unreachable());
    }
    let body = response.text().await.map_err(|_| unreachable())?;
    parse_stations(&body)
}

/// Stations whose name contains `term`, the most listened first. HLS stations are left out (see
/// `stations::probe`): the directory lists each stream as a station of its own, so there is no
/// MP3 or AAC alternative to fall back to within an entry.
pub async fn search(
    http: &reqwest::Client,
    base: &str,
    term: &str,
    country: Option<&str>,
    limit: i64,
) -> Result<Vec<DirectoryStation>, DirectoryError> {
    // Ask for more than wanted: what is filtered out here must not shorten the list.
    let ask = (limit * 2).min(100).to_string();
    let mut query = vec![
        ("name", term),
        ("limit", ask.as_str()),
        ("hidebroken", "true"),
        ("order", "clickcount"),
        ("reverse", "true"),
    ];
    if let Some(country) = country {
        query.push(("countrycode", country));
    }
    let url = format!("{}/json/stations/search", base.trim_end_matches('/'));
    let mut found = get(http, url, &query).await?;
    found.retain(|station| !station.hls);
    found.truncate(usize::try_from(limit).unwrap_or(0));
    Ok(found)
}

/// Whether `id` looks like one of the directory's ids (a UUID). It becomes part of a path, so
/// nothing else is passed on.
pub fn is_directory_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The station with this id, `None` when the directory does not know it (any more).
pub async fn lookup(
    http: &reqwest::Client,
    base: &str,
    id: &str,
) -> Result<Option<DirectoryStation>, DirectoryError> {
    if !is_directory_id(id) {
        return Ok(None);
    }
    let url = format!("{}/json/stations/byuuid/{id}", base.trim_end_matches('/'));
    let found = get(http, url, &[]).await?;
    Ok(found
        .into_iter()
        .find(|station| station.directory_id.eq_ignore_ascii_case(id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_an_entry_and_prefers_the_resolved_address() {
        let body = json!([{
            "stationuuid": "d28420a4-eccf-47a2-ace1-088c7e7cb7e0",
            "name": " Smooth Jazz ",
            "url": "http://example.com/listen.m3u",
            "url_resolved": "http://stream.example.com/128mp3",
            "homepage": "http://example.com/",
            "favicon": "http://example.com/favicon.png",
            "tags": "easy listening,jazz, Jazz ,,smooth jazz",
            "countrycode": "us",
            "codec": "MP3", "bitrate": 192, "hls": 0, "lastcheckok": 1,
        }])
        .to_string();
        assert_eq!(
            parse_stations(&body).unwrap(),
            vec![DirectoryStation {
                directory_id: "d28420a4-eccf-47a2-ace1-088c7e7cb7e0".into(),
                name: "Smooth Jazz".into(),
                stream_url: "http://stream.example.com/128mp3".into(),
                homepage: Some("http://example.com/".into()),
                country: Some("US".into()),
                tags: vec!["easy listening".into(), "jazz".into(), "smooth jazz".into()],
                codec: Some("MP3".into()),
                bitrate: Some(192),
                logo_url: Some("http://example.com/favicon.png".into()),
                hls: false,
            }]
        );
    }

    #[test]
    fn drops_what_cannot_be_played_and_blanks_what_is_unknown() {
        let body = json!([
            {"stationuuid": "a1b2c3d4", "name": "Offline", "url": "http://x/a", "lastcheckok": 0},
            {"stationuuid": "a1b2c3d5", "name": "No address", "url": ""},
            {"stationuuid": "a1b2c3d6", "name": "Odd address", "url": "rtmp://x/a"},
            {"stationuuid": "", "name": "No id", "url": "http://x/a"},
            {"stationuuid": "a1b2c3d7", "name": "", "url": "http://x/a"},
            42,
            {"stationuuid": "a1b2c3d8", "name": "Bare", "url": "http://x/a", "url_resolved": "",
             "homepage": "javascript:alert(1)", "favicon": "", "countrycode": "",
             "codec": "UNKNOWN", "bitrate": 0, "tags": "", "hls": 1},
        ])
        .to_string();
        let found = parse_stations(&body).unwrap();
        assert_eq!(found.len(), 1);
        let bare = &found[0];
        assert_eq!(bare.name, "Bare");
        assert_eq!(bare.stream_url, "http://x/a");
        assert_eq!(bare.homepage, None);
        assert_eq!(bare.logo_url, None);
        assert_eq!(bare.country, None);
        assert_eq!(bare.codec, None);
        assert_eq!(bare.bitrate, None);
        assert!(bare.tags.is_empty());
        assert!(bare.hls);
    }

    #[test]
    fn refuses_a_body_that_is_no_array() {
        for body in ["not json", "{}", r#"{"results":[]}"#] {
            assert_eq!(
                parse_stations(body),
                Err(DirectoryError(UNEXPECTED.into())),
                "{body}"
            );
        }
    }

    #[test]
    fn only_a_uuid_becomes_part_of_the_lookup_path() {
        assert!(is_directory_id("d28420a4-eccf-47a2-ace1-088c7e7cb7e0"));
        for bad in [
            "",
            "abc",
            "../../json/stats",
            "d28420a4/../x",
            "d28420a4?x=1",
        ] {
            assert!(!is_directory_id(bad), "{bad}");
        }
    }
}
