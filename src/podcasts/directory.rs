//! Searching the iTunes podcast directory (legacy `podcasts/directory.ts`).

use std::time::Duration;

use serde_json::Value;

pub const DEFAULT_COUNTRY: &str = "DE";
pub const DEFAULT_LIMIT: i64 = 25;
pub const DIRECTORY_URL: &str = "https://itunes.apple.com/search";
pub const UNEXPECTED: &str = "Unerwartete Antwort des Podcast-Verzeichnisses";
pub const UNREACHABLE: &str = "Podcast-Verzeichnis nicht erreichbar";

#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryResult {
    pub name: String,
    pub author: String,
    pub feed_url: String,
    pub artwork_url: Option<String>,
    pub episode_count: Option<i64>,
    pub genre: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DirectorySearchError(pub String);

/// A non-empty string value.
fn truthy<'a>(entry: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// Maps the iTunes response body; entries without a feed URL are dropped.
pub fn parse_results(body: &str) -> Result<Vec<DirectoryResult>, DirectorySearchError> {
    let unexpected = || DirectorySearchError(UNEXPECTED.into());
    let parsed: Value = serde_json::from_str(body).map_err(|_| unexpected())?;
    let results = parsed
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(unexpected)?;
    Ok(results
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|entry| {
            let feed_url = truthy(entry, "feedUrl")?.to_owned();
            Some(DirectoryResult {
                name: truthy(entry, "collectionName")
                    .or_else(|| truthy(entry, "trackName"))
                    .unwrap_or(&feed_url)
                    .to_owned(),
                author: truthy(entry, "artistName").unwrap_or("").to_owned(),
                artwork_url: entry
                    .get("artworkUrl100")
                    .and_then(Value::as_str)
                    .or_else(|| entry.get("artworkUrl60").and_then(Value::as_str))
                    .map(str::to_owned),
                episode_count: entry.get("trackCount").and_then(Value::as_i64),
                genre: entry
                    .get("genres")
                    .and_then(Value::as_array)
                    .and_then(|g| g.first())
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                feed_url,
            })
        })
        .collect())
}

/// Searches `directory_url` (the iTunes Search API; a local server in tests). An empty term asks
/// nothing and finds nothing.
pub async fn search_directory(
    http: &reqwest::Client,
    directory_url: &str,
    term: &str,
    limit: i64,
    country: &str,
) -> Result<Vec<DirectoryResult>, DirectorySearchError> {
    if term.trim().is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.to_string();
    let response = http
        .get(directory_url)
        .query(&[
            ("media", "podcast"),
            ("entity", "podcast"),
            ("term", term),
            ("limit", limit.as_str()),
            ("country", country),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| DirectorySearchError(UNREACHABLE.into()))?;
    let body = response
        .text()
        .await
        .map_err(|_| DirectorySearchError(UNREACHABLE.into()))?;
    parse_results(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_results_dropping_entries_without_a_feed_url() {
        let body = serde_json::json!({ "results": [
            { "collectionName": "Maschinenraum", "artistName": "M. Raum",
              "feedUrl": "https://x/feed.xml", "artworkUrl100": "https://x/art.png",
              "trackCount": 118, "genres": ["Technology"] },
            { "collectionName": "No Feed" }
        ]})
        .to_string();
        assert_eq!(
            parse_results(&body).unwrap(),
            vec![DirectoryResult {
                name: "Maschinenraum".into(),
                author: "M. Raum".into(),
                feed_url: "https://x/feed.xml".into(),
                artwork_url: Some("https://x/art.png".into()),
                episode_count: Some(118),
                genre: Some("Technology".into()),
            }]
        );
    }

    #[test]
    fn refuses_a_body_that_is_no_json_or_has_no_results_array() {
        for body in ["not json", r#"{"results":{}}"#, "{}"] {
            assert_eq!(
                parse_results(body),
                Err(DirectorySearchError(UNEXPECTED.into())),
                "{body}"
            );
        }
    }

    #[test]
    fn skips_entries_that_are_not_objects() {
        let body = serde_json::json!({ "results": [123, null, "invalid",
            { "collectionName": "Valid Podcast", "feedUrl": "https://x/feed.xml" }]})
        .to_string();
        let results = parse_results(&body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Valid Podcast");
        assert_eq!(results[0].author, "");
        assert_eq!(results[0].episode_count, None);
    }

    #[test]
    fn falls_back_to_track_name_then_feed_url_for_the_name() {
        let body = serde_json::json!({ "results": [
            { "trackName": "T", "feedUrl": "https://a" }, { "feedUrl": "https://b" }]})
        .to_string();
        let results = parse_results(&body).unwrap();
        assert_eq!(results[0].name, "T");
        assert_eq!(results[1].name, "https://b");
    }

    #[tokio::test]
    async fn an_empty_term_asks_nothing() {
        // An unroutable URL: a request would fail, so success proves none was made.
        let http = crate::podcasts::http_client();
        let results = search_directory(&http, "http://127.0.0.1:9/", "   ", 25, "DE").await;
        assert_eq!(results, Ok(Vec::new()));
    }
}
