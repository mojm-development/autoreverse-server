//! Which studio albums of the library's artists are not in the library, according to MusicBrainz.
//!
//! MusicBrainz asks for at most one request per second and a descriptive User-Agent. So one
//! background run at a time fills a cache, and clients read what is there.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Json;

use crate::state::AppState;

const API: &str = "https://musicbrainz.org/ws/2";
const USER_AGENT: &str = concat!(
    "Autoreverse-Server/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/mojm-development/autoreverse-server )"
);
const PAUSE: Duration = Duration::from_millis(1100);
const TIMEOUT: Duration = Duration::from_secs(30);
/// Artists with the most albums first; the rest would mostly be single stray albums.
const ARTISTS_PER_RUN: i64 = 60;
const STALE_DAYS: i32 = 30;
const MIN_SCORE: i64 = 95;
const PAGE_SIZE: i64 = 100;
const MAX_PAGES: i64 = 5;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownAlbum {
    pub title: String,
    pub year: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MissingAlbum {
    pub artist: String,
    pub title: String,
    pub year: Option<i32>,
}

/// Lower-case letters and digits only, without trailing additions such as "(Deluxe Edition)".
pub fn normalize(title: &str) -> String {
    let cut = title
        .find(['(', '['])
        .filter(|&at| at > 0)
        .unwrap_or(title.len());
    title[..cut]
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Only an exact name with a high score counts: a wrong artist would produce nonsense hints.
fn artist_id(search: &Value, name: &str) -> Option<String> {
    search["artists"].as_array()?.iter().find_map(|artist| {
        let matches = artist["name"].as_str()?.eq_ignore_ascii_case(name)
            && artist["score"].as_i64()? >= MIN_SCORE;
        matches.then(|| artist["id"].as_str().map(str::to_owned))?
    })
}

/// Released studio albums; live records and compilations carry secondary types.
fn studio_albums(groups: &Value, today: &str) -> Vec<KnownAlbum> {
    groups["release-groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|g| g["primary-type"] == "Album")
        .filter(|g| g["secondary-types"].as_array().is_none_or(|t| t.is_empty()))
        .filter_map(|g| {
            let date = g["first-release-date"].as_str().filter(|d| !d.is_empty())?;
            (date <= today).then(|| KnownAlbum {
                title: g["title"].as_str().unwrap_or("").to_owned(),
                year: date.get(..4).and_then(|y| y.parse().ok()),
            })
        })
        .filter(|a| !a.title.is_empty())
        .collect()
}

fn missing_from(artist: &str, known: &[KnownAlbum], owned: &HashSet<String>) -> Vec<MissingAlbum> {
    let mut seen = HashSet::new();
    known
        .iter()
        .filter(|a| {
            let key = normalize(&a.title);
            !key.is_empty() && !owned.contains(&key) && seen.insert(key)
        })
        .map(|a| MissingAlbum {
            artist: artist.to_owned(),
            title: a.title.clone(),
            year: a.year,
        })
        .collect()
}

async fn get(http: &reqwest::Client, url: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let response = http
        .get(url)
        .query(query)
        .timeout(TIMEOUT)
        .header("user-agent", USER_AGENT)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let text = response.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

async fn lookup(
    http: &reqwest::Client,
    artist: &str,
    today: &str,
) -> Result<Vec<KnownAlbum>, String> {
    let escaped = artist.replace('\\', "\\\\").replace('"', "\\\"");
    let search = get(
        http,
        &format!("{API}/artist"),
        &[
            ("query", &format!("artist:\"{escaped}\"")),
            ("fmt", "json"),
            ("limit", "5"),
        ],
    )
    .await?;
    tokio::time::sleep(PAUSE).await;
    let Some(id) = artist_id(&search, artist) else {
        return Ok(Vec::new());
    };
    // Prolific artists exceed one page, and the order mixes in compilations.
    let mut albums = Vec::new();
    for page in 0..MAX_PAGES {
        let offset = (page * PAGE_SIZE).to_string();
        let groups = get(
            http,
            &format!("{API}/release-group"),
            &[
                ("artist", id.as_str()),
                ("type", "album"),
                ("fmt", "json"),
                ("limit", "100"),
                ("offset", offset.as_str()),
            ],
        )
        .await?;
        tokio::time::sleep(PAUSE).await;
        albums.extend(studio_albums(&groups, today));
        if groups["release-group-count"].as_i64().unwrap_or(0) <= (page + 1) * PAGE_SIZE {
            break;
        }
    }
    Ok(albums)
}

/// Artists still to be asked about. Those somebody listens to come first, because only they can
/// lead to a suggestion.
pub async fn pending_artists(db: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT coalesce(i.album_artist, i.artist) AS name
         FROM items i
         LEFT JOIN musicbrainz_artists m ON m.artist = coalesce(i.album_artist, i.artist)
         LEFT JOIN (SELECT item_id, sum(seconds) AS heard FROM listen_events GROUP BY item_id) l
                ON l.item_id = i.id
         WHERE i.kind = 'album' AND i.missing_since IS NULL AND i.duplicate_of IS NULL
           AND coalesce(i.album_artist, i.artist) IS NOT NULL
           AND lower(coalesce(i.album_artist, i.artist)) NOT IN ('various artists', 'various')
           AND (m.artist IS NULL OR m.fetched_at < now() - make_interval(days => $1))
         GROUP BY 1 ORDER BY coalesce(sum(l.heard), 0) DESC, count(*) DESC, 1 LIMIT $2",
    )
    .bind(STALE_DAYS)
    .bind(ARTISTS_PER_RUN)
    .fetch_all(db)
    .await
}

/// Starts a run unless one is active or nothing is due. Stops at the first error, so that
/// MusicBrainz is not hammered while it is unwell; the next visit continues.
pub fn start_refresh(state: &AppState) {
    if state.musicbrainz_running.swap(true, Ordering::SeqCst) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        match pending_artists(&state.db).await {
            Ok(artists) => {
                for artist in artists {
                    let albums = match lookup(&state.http, &artist, &today).await {
                        Ok(albums) => albums,
                        Err(e) => {
                            tracing::warn!("[musicbrainz] {artist}: {e}");
                            break;
                        }
                    };
                    let saved = sqlx::query(
                        "INSERT INTO musicbrainz_artists (artist, albums) VALUES ($1, $2)
                         ON CONFLICT (artist) DO UPDATE
                         SET albums = excluded.albums, fetched_at = now()",
                    )
                    .bind(&artist)
                    .bind(Json(&albums))
                    .execute(&state.db)
                    .await;
                    if let Err(e) = saved {
                        tracing::warn!("[musicbrainz] {e}");
                        break;
                    }
                }
            }
            Err(e) => tracing::warn!("[musicbrainz] {e}"),
        }
        state.musicbrainz_running.store(false, Ordering::SeqCst);
    });
}

/// Real albums of these artists that the library lacks: the only titles a model may pick from.
pub async fn candidates(
    db: &PgPool,
    artists: &[String],
    per_artist: usize,
) -> Result<Vec<MissingAlbum>, sqlx::Error> {
    let cached: Vec<(String, Json<Vec<KnownAlbum>>)> =
        sqlx::query_as("SELECT artist, albums FROM musicbrainz_artists WHERE artist = ANY($1)")
            .bind(artists)
            .fetch_all(db)
            .await?;
    let owned_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT coalesce(album_artist, artist), title FROM items
         WHERE kind = 'album' AND coalesce(album_artist, artist) = ANY($1)",
    )
    .bind(artists)
    .fetch_all(db)
    .await?;
    let mut owned: HashMap<String, HashSet<String>> = HashMap::new();
    for (artist, title) in owned_rows {
        owned.entry(artist).or_default().insert(normalize(&title));
    }
    let by_artist: HashMap<&str, &[KnownAlbum]> = cached
        .iter()
        .map(|(a, k)| (a.as_str(), k.0.as_slice()))
        .collect();
    let empty = HashSet::new();
    Ok(artists
        .iter()
        .filter_map(|artist| by_artist.get(artist.as_str()).map(|known| (artist, known)))
        .flat_map(|(artist, known)| {
            let mut found = missing_from(artist, known, owned.get(artist).unwrap_or(&empty));
            found.truncate(per_artist);
            found
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizing_ignores_case_punctuation_and_editions() {
        assert_eq!(
            normalize("Autobahn (Remaster 2009)"),
            normalize("AUTOBAHN!")
        );
        assert_ne!(normalize("Radio-Aktivität"), normalize("Autobahn"));
        assert_eq!(normalize("(What's the Story)"), "whatsthestory");
    }

    #[test]
    fn takes_only_an_exact_name_with_a_high_score() {
        let search = json!({"artists": [
            {"id": "a", "name": "Kraftwerk Tribute", "score": 100},
            {"id": "b", "name": "kraftwerk", "score": 90},
            {"id": "c", "name": "Kraftwerk", "score": 100},
        ]});
        assert_eq!(artist_id(&search, "Kraftwerk").as_deref(), Some("c"));
        assert_eq!(artist_id(&json!({"artists": []}), "Kraftwerk"), None);
    }

    #[test]
    fn keeps_released_studio_albums_only() {
        let groups = json!({"release-groups": [
            {"title": "Studio", "primary-type": "Album", "secondary-types": [], "first-release-date": "1974-11-01"},
            {"title": "Live", "primary-type": "Album", "secondary-types": ["Live"], "first-release-date": "1980"},
            {"title": "EP", "primary-type": "EP", "secondary-types": [], "first-release-date": "1981"},
            {"title": "Kommt noch", "primary-type": "Album", "secondary-types": [], "first-release-date": "2999-01-01"},
            {"title": "Ohne Datum", "primary-type": "Album", "secondary-types": [], "first-release-date": ""},
        ]});
        assert_eq!(
            studio_albums(&groups, "2026-11-08"),
            vec![KnownAlbum {
                title: "Studio".into(),
                year: Some(1974)
            }]
        );
    }

    #[test]
    fn leaves_out_what_is_owned_and_repeats() {
        let known = [
            KnownAlbum {
                title: "Autobahn".into(),
                year: Some(1974),
            },
            KnownAlbum {
                title: "Neu".into(),
                year: None,
            },
            KnownAlbum {
                title: "NEU!".into(),
                year: None,
            },
        ];
        let owned = HashSet::from([normalize("Autobahn (Deluxe)")]);
        let found = missing_from("K", &known, &owned);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Neu");
    }
}
