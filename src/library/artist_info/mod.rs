//! What the wider world knows about an artist: a biography, a few facts, pictures and their most
//! played tracks.
//!
//! The library's tags say none of this, so it comes from two public catalogues that need no
//! account: TheAudioDB (text, facts, pictures) and Apple's music catalogue (the most played
//! songs). Only the artist's name is sent to them, and only for artists of the library: those
//! with an album here and those named as the artist of a track (a sampler's songs).
//!
//! Answers are kept in `artist_info`. The job in [`job`] fetches them ahead of time for every such
//! artist and asks again after the interval set under Einstellungen → Server (a month by default), so a page
//! never waits for a catalogue: a request finds the answer kept, an outdated one is served at once
//! and renewed behind the scenes, and only an artist the job has not reached yet is asked for
//! while the request waits. Pictures are stored under `artists/info/` — those of album artists by
//! the job, the others on first request — and served like any other stored image, so no client
//! ever talks to a catalogue itself.

pub mod job;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Json;

use crate::error::{ApiError, ApiResult};
use crate::library::artist_covers::{IMAGE_TYPES, image_file_name};
use crate::library::queries::TrackHit;
use crate::podcasts::directory::DEFAULT_COUNTRY;
use crate::state::AppState;

/// TheAudioDB's API under its public key.
pub const AUDIODB_URL: &str = "https://www.theaudiodb.com/api/v1/json/123";
/// The iTunes Search API, which the podcast directory uses as well.
pub const CATALOG_URL: &str = "https://itunes.apple.com";

const USER_AGENT: &str = concat!("Autoreverse-Server/", env!("CARGO_PKG_VERSION"));
const TIMEOUT: Duration = Duration::from_secs(12);
const PICTURE_TIMEOUT: Duration = Duration::from_secs(30);
/// The most played songs a page shows: those of the library, in the catalogue's order.
pub const TOP_TRACKS: usize = 10;
/// How far down the catalogue's chart is looked for songs the library has.
const CHART_LENGTH: usize = 50;
/// Shorter records than this are singles and EPs, not albums.
const MIN_ALBUM_TRACKS: u64 = 5;
/// A name nobody knows today may have a page tomorrow: asked again after a week at the latest.
const KEEP_UNKNOWN_DAYS: f64 = 7.0;
const MAX_PICTURES: usize = 5;
/// The size asked of the catalogue for a song's cover; it names a 100-pixel one.
const COVER_PIXELS: u32 = 600;
pub const MAX_PICTURE_BYTES: usize = 8 * 1024 * 1024;
pub const NO_PICTURE: &str = "Kein Bild";

/// A picture and where it can be fetched. Clients only ever see its position in the list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// `portrait` (roughly square, the artist) or `backdrop` (wide).
    pub kind: String,
    pub url: String,
}

/// One of the artist's most played songs, as the catalogue names it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TopTrack {
    pub title: String,
    pub album: String,
    pub duration: f64,
    /// Where the catalogue keeps the record's cover; `null` when it named none. Clients only see
    /// whether there is one (`has_cover`) and fetch it from the server.
    #[serde(default)]
    pub artwork: Option<String>,
}

/// The `info` column: everything kept about one artist.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Stored {
    /// By two-letter language code, `en` always among them when there is any.
    #[serde(default)]
    pub biographies: BTreeMap<String, String>,
    pub genre: Option<String>,
    pub country: Option<String>,
    pub formed_year: Option<i32>,
    pub born_year: Option<i32>,
    pub members: Option<i32>,
    #[serde(default)]
    pub pictures: Vec<Picture>,
    /// The catalogue's chart of the artist, [`CHART_LENGTH`] songs at most.
    #[serde(default)]
    pub top_tracks: Vec<TopTrack>,
    /// The artist's albums in the catalogue, newest first; the page lists those the library
    /// lacks. `None` for answers kept before albums were asked for.
    #[serde(default)]
    pub albums: Option<Vec<CatalogAlbum>>,
}

/// One of the artist's albums, as the catalogue names it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CatalogAlbum {
    pub title: String,
    pub year: Option<i32>,
    pub track_count: u64,
    /// Where the catalogue keeps the cover; clients fetch it from the server.
    pub artwork: Option<String>,
}

impl Stored {
    pub fn is_empty(&self) -> bool {
        self.biographies.is_empty()
            && self.pictures.is_empty()
            && self.top_tracks.is_empty()
            && self.albums.as_ref().is_none_or(Vec::is_empty)
    }

    /// The biography in `language`, else the English one, with the language it is in.
    pub fn biography(&self, language: &str) -> Option<(String, &str)> {
        // TheAudioDB names a few of its languages after the country.
        let code = match language {
            "ja" => "jp",
            "zh" => "cn",
            "he" => "il",
            "sv" => "se",
            other => other,
        };
        self.biographies
            .get(code)
            .map(|text| (language.to_owned(), text.as_str()))
            .or_else(|| {
                self.biographies
                    .get("en")
                    .map(|text| ("en".to_owned(), text.as_str()))
            })
    }
}

/// One `artist_info` row.
#[derive(Debug, Clone)]
pub struct ArtistInfo {
    pub found: bool,
    pub info: Stored,
    pub fetched_at: Option<DateTime<Utc>>,
}

/// Letters and digits only, so "AC/DC" and "Ac-Dc" are the same name.
pub fn strict(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A title without "(Remastered 2011)", "[Live]" and " - Single Version". A title that is
/// nothing but an addition, such as "(Intro)", stays as it is.
pub fn without_additions(title: &str) -> String {
    static BRACKETS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").unwrap());
    let mut cut = BRACKETS.replace_all(title, "").into_owned();
    if let Some(dash) = cut.find(" - ").filter(|&at| at > 0) {
        cut.truncate(dash);
    }
    let cut = cut.trim();
    if strict(cut).is_empty() {
        title.trim().to_owned()
    } else {
        cut.to_owned()
    }
}

/// [`strict`] of the title [`without_additions`].
pub fn loose(text: &str) -> String {
    strict(&without_additions(text))
}

fn is_other_version(title: &str) -> bool {
    static WORDS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(live|demo|remix|instrumental|acoustic|karaoke|edit)\b").unwrap()
    });
    WORDS.is_match(title)
}

/// A non-empty text value; TheAudioDB writes numbers as strings and "0" for unknown.
fn text(entry: &Value, key: &str) -> Option<String> {
    let value = match &entry[key] {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!value.is_empty() && value != "0").then_some(value)
}

fn number(entry: &Value, key: &str) -> Option<i32> {
    text(entry, key)?.parse().ok().filter(|n| *n > 0)
}

/// What TheAudioDB's artist search says about exactly this artist; nothing when it only knows
/// others.
pub fn parse_audiodb(body: &Value, name: &str) -> Stored {
    let wanted = strict(name);
    let Some(artist) = body["artists"].as_array().and_then(|artists| {
        artists
            .iter()
            .find(|a| a["strArtist"].as_str().is_some_and(|n| strict(n) == wanted))
    }) else {
        return Stored::default();
    };
    let mut biographies = BTreeMap::new();
    for (key, value) in artist.as_object().into_iter().flatten() {
        let (Some(code), Some(text)) = (key.strip_prefix("strBiography"), value.as_str()) else {
            continue;
        };
        if !text.trim().is_empty() {
            let code = if code.is_empty() {
                "en".to_owned()
            } else {
                code.to_lowercase()
            };
            biographies.insert(code, text.trim().to_owned());
        }
    }
    let mut pictures = Vec::new();
    for (key, kind) in [
        ("strArtistThumb", "portrait"),
        ("strArtistFanart", "backdrop"),
        ("strArtistFanart2", "backdrop"),
        ("strArtistFanart3", "backdrop"),
        ("strArtistFanart4", "backdrop"),
    ] {
        if let Some(url) = text(artist, key) {
            pictures.push(Picture {
                kind: kind.to_owned(),
                url,
            });
        }
    }
    pictures.truncate(MAX_PICTURES);
    Stored {
        biographies,
        genre: text(artist, "strGenre").or_else(|| text(artist, "strStyle")),
        country: text(artist, "strCountry"),
        formed_year: number(artist, "intFormedYear"),
        born_year: number(artist, "intBornYear"),
        members: number(artist, "intMembers"),
        pictures,
        top_tracks: Vec::new(),
        albums: None,
    }
}

/// The catalogue's id of the artist of exactly this name, or failing that of the first whose
/// name differs only in punctuation: "Afro B" is not "Afrob", though both come down to the same
/// letters.
pub fn pick_artist(body: &Value, name: &str) -> Option<i64> {
    let candidates = body["results"].as_array()?;
    let named = |artist: &&Value| {
        artist["artistName"]
            .as_str()
            .map(str::trim)
            .map(str::to_owned)
    };
    let wanted = strict(name);
    candidates
        .iter()
        .find(|a| named(a).is_some_and(|n| n.to_lowercase() == name.trim().to_lowercase()))
        .or_else(|| {
            candidates
                .iter()
                .find(|a| !wanted.is_empty() && named(a).is_some_and(|n| strict(&n) == wanted))
        })
        .and_then(|a| a["artistId"].as_i64())
}

/// The artist's songs in the catalogue's order, which is by how much they are played. The same
/// song is sold on several records and in several versions; the first one stands for all.
pub fn parse_top_tracks(body: &Value) -> Vec<TopTrack> {
    static RELEASE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+-\s+(Single|EP)$").unwrap());
    let mut seen = HashSet::new();
    body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|song| {
            let title = song["trackName"].as_str().filter(|t| !t.is_empty())?;
            seen.insert(loose(title)).then(|| TopTrack {
                title: title.to_owned(),
                album: RELEASE
                    .replace(song["collectionName"].as_str().unwrap_or(""), "")
                    .into_owned(),
                duration: song["trackTimeMillis"].as_f64().unwrap_or(0.0) / 1000.0,
                artwork: larger_artwork(song),
            })
        })
        .take(CHART_LENGTH)
        .collect()
}

/// Apple names a 100-pixel cover; [`COVER_PIXELS`] are asked for instead.
fn larger_artwork(entry: &Value) -> Option<String> {
    entry["artworkUrl100"]
        .as_str()
        .filter(|url| !url.is_empty())
        .map(|url| url.replace("/100x100bb.", &format!("/{COVER_PIXELS}x{COVER_PIXELS}bb.")))
}

/// The artist's albums in the catalogue's answer, newest first: no singles or EPs, and of an
/// album sold in several editions (clean, deluxe) only the newest.
pub fn parse_albums(body: &Value) -> Vec<CatalogAlbum> {
    static RELEASE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+-\s+(Single|EP)$").unwrap());
    let mut albums: Vec<(String, CatalogAlbum)> = body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["wrapperType"] == "collection")
        .filter_map(|entry| {
            let title = entry["collectionName"].as_str().filter(|t| !t.is_empty())?;
            let track_count = entry["trackCount"].as_u64().unwrap_or(0);
            if RELEASE.is_match(title) || track_count < MIN_ALBUM_TRACKS {
                return None;
            }
            let date = entry["releaseDate"].as_str().unwrap_or("").to_owned();
            Some((
                date.clone(),
                CatalogAlbum {
                    title: title.to_owned(),
                    year: date.get(..4).and_then(|y| y.parse().ok()),
                    track_count,
                    artwork: larger_artwork(entry),
                },
            ))
        })
        .collect();
    albums.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen = HashSet::new();
    albums
        .into_iter()
        .map(|(_, album)| album)
        .filter(|album| seen.insert(loose(&album.title)))
        .collect()
}

/// The words of a title, sorted, without the ones that only name an edition: "Best Of (Deluxe
/// Edition)" and "Deluxe: Best Of" come to the same.
fn album_words(title: &str) -> Vec<String> {
    const EDITION: [&str; 9] = [
        "deluxe",
        "edition",
        "version",
        "bonus",
        "expanded",
        "remastered",
        "remaster",
        "special",
        "anniversary",
    ];
    let mut words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| !EDITION.contains(&w.as_str()))
        .collect();
    words.sort();
    words
}

/// The albums of `catalogue` that are not among `library` (titles of the artist's albums here),
/// with their positions in `catalogue`. "LOVE? (Deluxe Edition)" here counts as "LOVE?" there,
/// and "Classics: Night Songs" there as "Night Songs (Classics)" here.
pub fn missing_albums<'a>(
    catalogue: &'a [CatalogAlbum],
    library: &[String],
) -> Vec<(usize, &'a CatalogAlbum)> {
    let have: HashSet<String> = library.iter().map(|t| loose(t)).collect();
    let have_words: HashSet<Vec<String>> = library
        .iter()
        .map(|t| album_words(t))
        .filter(|w| !w.is_empty())
        .collect();
    catalogue
        .iter()
        .enumerate()
        .filter(|(_, album)| {
            !have.contains(&loose(&album.title)) && !have_words.contains(&album_words(&album.title))
        })
        .collect()
}

/// Titles of the artist's present albums.
pub async fn library_albums(db: &PgPool, artist: &str) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT title FROM items WHERE kind = 'album' AND lower(artist) = lower($1)
           AND missing_since IS NULL AND duplicate_of IS NULL",
    )
    .bind(artist)
    .fetch_all(db)
    .await
}

/// The library's track for a popular one: the artist's own recording of that title, preferring
/// the same version, the same album and the closest length. A live take is better than nothing
/// to play, so it counts when it is all there is.
pub fn best_hit<'a>(hits: &'a [TrackHit], artist: &str, wanted: &TopTrack) -> Option<&'a TrackHit> {
    let wanted_artist = strict(artist);
    let wanted_loose = loose(&wanted.title);
    let wanted_strict = strict(&wanted.title);
    let wanted_album = loose(&wanted.album);
    let wants_other_version = is_other_version(&wanted.title);
    hits.iter()
        .filter_map(|hit| {
            let title = hit.title.as_deref()?;
            let by_artist = hit
                .artist
                .as_deref()
                .is_none_or(|a| strict(a).contains(&wanted_artist));
            if loose(title) != wanted_loose || !by_artist {
                return None;
            }
            let mut score = 0.0;
            if strict(title) == wanted_strict {
                score += 100.0;
            }
            if is_other_version(title) != wants_other_version {
                score -= 50.0;
            }
            if loose(&hit.item_title) == wanted_album {
                score += 20.0;
            }
            if wanted.duration > 0.0 {
                score -= (hit.duration - wanted.duration).abs().min(30.0) / 3.0;
            }
            Some((score, hit))
        })
        // The first of equals, so the answer does not depend on the order ties arrive in.
        .fold(
            None,
            |best: Option<(f64, &TrackHit)>, (score, hit)| match best {
                Some((top, _)) if top >= score => best,
                _ => Some((score, hit)),
            },
        )
        .map(|(_, hit)| hit)
}

/// Every track that could be one of the artist's: those on their albums and those that name them
/// as the track's own artist (a sampler's songs).
pub async fn library_tracks(db: &PgPool, artist: &str) -> Result<Vec<TrackHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT track.id, track.title, track.artist, track.duration, track.item_id,
                item.title AS item_title, item.kind AS item_kind, track.has_lyrics,
                track.gain_db, track.album_gain_db, track.peak
         FROM tracks AS track JOIN items AS item ON item.id = track.item_id
         WHERE item.kind = 'album' AND item.missing_since IS NULL AND item.duplicate_of IS NULL
           AND (lower(item.artist) = lower($1) OR lower(track.artist) = lower($1))
         ORDER BY track.id",
    )
    .bind(artist)
    .fetch_all(db)
    .await
}

/// Whether the artist has a present album or is named as the artist of a track on one: nobody
/// else has a page, and no other name is sent out.
pub async fn is_library_artist(db: &PgPool, artist: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM items WHERE kind = 'album' AND artist = $1
                        AND missing_since IS NULL AND duplicate_of IS NULL)
             OR EXISTS (SELECT 1 FROM tracks AS track JOIN items AS item ON item.id = track.item_id
                        WHERE item.kind = 'album' AND track.artist = $1
                          AND item.missing_since IS NULL AND item.duplicate_of IS NULL)",
    )
    .bind(artist)
    .fetch_one(db)
    .await
}

/// Whether the artist has a present album; only their pictures are fetched ahead of time.
pub async fn is_album_artist(db: &PgPool, artist: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM items WHERE kind = 'album' AND artist = $1
                        AND missing_since IS NULL AND duplicate_of IS NULL)",
    )
    .bind(artist)
    .fetch_one(db)
    .await
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

/// The artist's chart and albums from Apple's catalogue; both empty when it does not know them.
async fn catalogue(
    http: &reqwest::Client,
    base: &str,
    name: &str,
) -> Result<(Vec<TopTrack>, Vec<CatalogAlbum>), String> {
    let search = get(
        http,
        &format!("{base}/search"),
        &[
            ("term", name),
            ("entity", "musicArtist"),
            ("attribute", "artistTerm"),
            ("limit", "10"),
            ("country", DEFAULT_COUNTRY),
        ],
    )
    .await?;
    let Some(id) = pick_artist(&search, name) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let id = id.to_string();
    let lookup = |entity: &'static str| {
        let id = id.clone();
        async move {
            get(
                http,
                &format!("{base}/lookup"),
                &[
                    ("id", id.as_str()),
                    ("entity", entity),
                    ("limit", "50"),
                    ("country", DEFAULT_COUNTRY),
                ],
            )
            .await
        }
    };
    let (songs, albums) = tokio::join!(lookup("song"), lookup("album"));
    Ok((parse_top_tracks(&songs?), parse_albums(&albums?)))
}

/// Asks both catalogues at once. An error when either could not be reached: half an answer must
/// not be kept for a month.
async fn fetch(state: &AppState, name: &str) -> Result<Stored, String> {
    let config = &state.config;
    let search = format!("{}/search.php", config.audiodb_url);
    let query = [("s", name)];
    let (about, catalogue) = tokio::join!(
        get(&state.http, &search, &query),
        catalogue(&state.http, &config.music_catalog_url, name),
    );
    let mut info = parse_audiodb(&about?, name);
    let (chart, albums) = catalogue?;
    info.top_tracks = chart;
    info.albums = Some(albums);
    Ok(info)
}

pub(crate) async fn stored(db: &PgPool, artist: &str) -> Result<Option<ArtistInfo>, sqlx::Error> {
    let row: Option<(bool, Json<Stored>, DateTime<Utc>)> =
        sqlx::query_as("SELECT found, info, fetched_at FROM artist_info WHERE artist = $1")
            .bind(artist)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(found, info, fetched_at)| ArtistInfo {
        found,
        info: info.0,
        fetched_at: Some(fetched_at),
    }))
}

/// How many days an answer is kept before it is asked for again, `None` for ever (`refresh_days`
/// 0). An unknown name never waits longer than [`KEEP_UNKNOWN_DAYS`].
pub fn keep_days(found: bool, refresh_days: f64) -> Option<f64> {
    if refresh_days <= 0.0 {
        None
    } else if found {
        Some(refresh_days)
    } else {
        Some(refresh_days.min(KEEP_UNKNOWN_DAYS))
    }
}

fn is_fresh(info: &ArtistInfo, now: DateTime<Utc>, refresh_days: f64) -> bool {
    let Some(at) = info.fetched_at else {
        return false;
    };
    // Kept before the catalogue was asked for albums: outdated, however young — the job asks
    // again too (`job::due`), but a page should not wait for it.
    if info.found && info.info.albums.is_none() {
        return false;
    }
    match keep_days(info.found, refresh_days) {
        None => true,
        Some(days) => ((now - at).num_seconds() as f64) < days * 86_400.0,
    }
}

/// The artists whose answers are being renewed, so that a page visited twice asks only once.
#[derive(Clone, Default)]
pub struct Renewing(Arc<Mutex<HashSet<String>>>);

impl Renewing {
    /// True when the caller is the one to renew `artist`.
    pub(crate) fn claim(&self, artist: &str) -> bool {
        self.0.lock().unwrap().insert(artist.to_owned())
    }

    pub(crate) fn release(&self, artist: &str) {
        self.0.lock().unwrap().remove(artist);
    }
}

/// What is known about `artist`. Never waits for a catalogue when anything is kept: an outdated
/// answer is served as it is and renewed behind the scenes. Only an artist nothing is kept for
/// yet is asked for while the caller waits. Never an error because of the catalogues: an
/// unreachable one leaves the old answer, or none, and the next request asks again.
pub async fn artist_info(state: &AppState, artist: &str) -> Result<ArtistInfo, sqlx::Error> {
    let kept = stored(&state.db, artist).await?;
    let refresh_days = state.settings.get().artist_info_days;
    if let Some(info) = kept
        .as_ref()
        .filter(|info| is_fresh(info, Utc::now(), refresh_days))
    {
        return Ok(info.clone());
    }
    let unknown = ArtistInfo {
        found: false,
        info: Stored::default(),
        fetched_at: None,
    };
    if !state.settings.get().artist_info {
        return Ok(kept.unwrap_or(unknown));
    }
    if let Some(outdated) = kept {
        renew_later(state, artist);
        return Ok(outdated);
    }
    Ok(renew(state, artist, None).await?.unwrap_or(unknown))
}

/// Renews `artist` in a task of its own, unless that is happening already.
fn renew_later(state: &AppState, artist: &str) {
    if !state.artist_info_renewing.claim(artist) {
        return;
    }
    let state = state.clone();
    let artist = artist.to_owned();
    tokio::spawn(async move {
        let kept = stored(&state.db, &artist).await;
        match kept {
            Ok(kept) => {
                if let Err(e) = renew(&state, &artist, kept).await {
                    tracing::warn!("[artist-info] {artist}: {e}");
                }
            }
            Err(e) => tracing::warn!("[artist-info] {artist}: {e}"),
        }
        state.artist_info_renewing.release(&artist);
    });
}

/// Asks the catalogues about `artist` and keeps the answer in place of `kept`. `None` when a
/// catalogue could not be reached; nothing is changed then.
pub async fn renew(
    state: &AppState,
    artist: &str,
    kept: Option<ArtistInfo>,
) -> Result<Option<ArtistInfo>, sqlx::Error> {
    let info = match fetch(state, artist).await {
        Ok(info) => info,
        Err(e) => {
            tracing::warn!("[artist-info] {artist}: {e}");
            return Ok(None);
        }
    };
    let found = !info.is_empty();
    let fetched_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO artist_info (artist, found, info) VALUES ($1, $2, $3)
         ON CONFLICT (artist) DO UPDATE
         SET found = excluded.found, info = excluded.info, fetched_at = now()
         RETURNING fetched_at",
    )
    .bind(artist)
    .bind(found)
    .bind(Json(&info))
    .fetch_one(&state.db)
    .await?;
    // The list may have changed, and a file is found by its position in it.
    if let Some(old) = kept {
        if old.info.pictures != info.pictures {
            remove_pictures(&state.config.artists_dir, artist).await;
        }
        if old.info.top_tracks != info.top_tracks {
            remove_covers(&state.config.artists_dir, artist).await;
        }
        if old.info.albums != info.albums {
            remove_album_covers(&state.config.artists_dir, artist).await;
        }
    }
    Ok(Some(ArtistInfo {
        found,
        info,
        fetched_at: Some(fetched_at),
    }))
}

fn pictures_dir(artists_dir: &Path) -> PathBuf {
    artists_dir.join("info")
}

fn picture_path(artists_dir: &Path, artist: &str, index: usize, extension: &str) -> PathBuf {
    pictures_dir(artists_dir).join(format!(
        "{}-{index}{extension}",
        image_file_name(artist, "")
    ))
}

fn cover_path(artists_dir: &Path, artist: &str, index: usize, extension: &str) -> PathBuf {
    pictures_dir(artists_dir).join(format!(
        "{}-top-{index}{extension}",
        image_file_name(artist, "")
    ))
}

fn album_cover_path(artists_dir: &Path, artist: &str, index: usize, extension: &str) -> PathBuf {
    pictures_dir(artists_dir).join(format!(
        "{}-album-{index}{extension}",
        image_file_name(artist, "")
    ))
}

async fn remove_album_covers(artists_dir: &Path, artist: &str) {
    let Ok(mut dir) = tokio::fs::read_dir(pictures_dir(artists_dir)).await else {
        return;
    };
    let prefix = format!("{}-album-", image_file_name(artist, ""));
    while let Ok(Some(entry)) = dir.next_entry().await {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

async fn remove_covers(artists_dir: &Path, artist: &str) {
    for index in 0..CHART_LENGTH {
        for (_, extension) in IMAGE_TYPES {
            let _ = tokio::fs::remove_file(cover_path(artists_dir, artist, index, extension)).await;
        }
    }
}

async fn remove_pictures(artists_dir: &Path, artist: &str) {
    for index in 0..MAX_PICTURES {
        for (_, extension) in IMAGE_TYPES {
            let _ =
                tokio::fs::remove_file(picture_path(artists_dir, artist, index, extension)).await;
        }
    }
}

/// Whether a picture may be fetched from `url`: TheAudioDB's own hosts over TLS, or wherever the
/// catalogue itself was configured to be. What a catalogue names is still somebody else's word.
pub fn is_picture_source(url: &Url, audiodb_url: &str) -> bool {
    let same_as_catalogue = Url::parse(audiodb_url).is_ok_and(|base| {
        base.host_str() == url.host_str()
            && base.port_or_known_default() == url.port_or_known_default()
    });
    let theirs = url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host == "theaudiodb.com" || host.ends_with(".theaudiodb.com"));
    same_as_catalogue || theirs
}

/// Whether a song's cover may be fetched from `url`: Apple's image hosts over TLS, or wherever
/// the music catalogue itself was configured to be.
pub fn is_cover_source(url: &Url, catalog_url: &str) -> bool {
    let same_as_catalogue = Url::parse(catalog_url).is_ok_and(|base| {
        base.host_str() == url.host_str()
            && base.port_or_known_default() == url.port_or_known_default()
    });
    let theirs = url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(".mzstatic.com"));
    same_as_catalogue || theirs
}

/// Fetches an image from `url`. `catalogue` is the base address of the catalogue that named it.
async fn download(
    state: &AppState,
    url: &Url,
    catalogue: &str,
) -> Result<(&'static str, Vec<u8>), String> {
    // A catalogue moved to a local address (tests, a mirror) is reached plainly; everything else
    // through the client that refuses to be redirected into the local network.
    let local = Url::parse(catalogue).is_ok_and(|base| base.host_str() == url.host_str());
    let http: &reqwest::Client = if local {
        &state.http
    } else {
        state.http.public()
    };
    let mut response = http
        .get(url.clone())
        .timeout(PICTURE_TIMEOUT)
        .header("user-agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_PICTURE_BYTES {
            return Err("zu groß".into());
        }
    }
    // By what the bytes are, not by what the sender calls them.
    let extension = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Jpeg) => ".jpg",
        Ok(image::ImageFormat::Png) => ".png",
        Ok(image::ImageFormat::WebP) => ".webp",
        Ok(image::ImageFormat::Gif) => ".gif",
        _ => return Err("kein Bild".into()),
    };
    Ok((extension, bytes))
}

/// The stored file of the artist's picture number `index`, fetched now if this is the first
/// request for it. 404 "Kein Bild" for anything that cannot be had.
pub async fn picture(state: &AppState, artist: &str, index: usize) -> ApiResult<PathBuf> {
    let not_found = || ApiError::not_found(NO_PICTURE);
    let info = stored(&state.db, artist).await?.ok_or_else(not_found)?;
    let wanted = info.info.pictures.get(index).ok_or_else(not_found)?;
    if let Some(path) = stored_picture(&state.config.artists_dir, artist, index).await {
        return Ok(path);
    }
    fetch_picture(state, artist, index, wanted).await
}

/// The file of picture `index`, when it has been fetched before.
async fn stored_picture(artists_dir: &Path, artist: &str, index: usize) -> Option<PathBuf> {
    for (_, extension) in IMAGE_TYPES {
        let path = picture_path(artists_dir, artist, index, extension);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Some(path);
        }
    }
    None
}

/// Fetches the pictures of `artist` and the covers of the albums the library lacks that are not
/// stored yet; how many it got.
pub async fn fetch_missing_pictures(state: &AppState, artist: &str) -> Result<usize, sqlx::Error> {
    let Some(info) = stored(&state.db, artist).await? else {
        return Ok(0);
    };
    let artists_dir = &state.config.artists_dir;
    let mut fetched = 0;
    for (index, wanted) in info.info.pictures.iter().enumerate() {
        if stored_picture(artists_dir, artist, index).await.is_none()
            && fetch_picture(state, artist, index, wanted).await.is_ok()
        {
            fetched += 1;
        }
    }
    let library = library_albums(&state.db, artist).await?;
    let catalogue = info.info.albums.unwrap_or_default();
    for (index, album) in missing_albums(&catalogue, &library) {
        let Some(artwork) = album.artwork.as_deref() else {
            continue;
        };
        let path = |extension: &str| album_cover_path(artists_dir, artist, index, extension);
        if stored_file(path).await.is_none()
            && fetch_catalogue_image(state, artist, artwork, path)
                .await
                .is_ok()
        {
            fetched += 1;
        }
    }
    Ok(fetched)
}

/// The file at `path` for any image extension, when there is one.
async fn stored_file(path: impl Fn(&str) -> PathBuf) -> Option<PathBuf> {
    for (_, extension) in IMAGE_TYPES {
        let path = path(extension);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Some(path);
        }
    }
    None
}

/// Fetches an image the music catalogue named and stores it at `path`.
async fn fetch_catalogue_image(
    state: &AppState,
    artist: &str,
    artwork: &str,
    path: impl Fn(&str) -> PathBuf,
) -> ApiResult<PathBuf> {
    let url = Url::parse(artwork)
        .ok()
        .filter(|url| is_cover_source(url, &state.config.music_catalog_url));
    fetch_image(state, url, &state.config.music_catalog_url, path)
        .await
        .inspect_err(|e| tracing::warn!("[artist-info] {artist}: Cover: {e}"))
        .map_err(|_| ApiError::not_found(NO_PICTURE))
}

/// The stored cover of album number `index` of the catalogue's list, fetched now if this is the
/// first request for it. 404 "Kein Bild" for anything that cannot be had.
pub async fn album_cover(state: &AppState, artist: &str, index: usize) -> ApiResult<PathBuf> {
    let not_found = || ApiError::not_found(NO_PICTURE);
    let info = stored(&state.db, artist).await?.ok_or_else(not_found)?;
    let albums = info.info.albums.unwrap_or_default();
    let artwork = albums
        .get(index)
        .and_then(|album| album.artwork.as_deref())
        .ok_or_else(not_found)?;
    let artists_dir = &state.config.artists_dir;
    let path = |extension: &str| album_cover_path(artists_dir, artist, index, extension);
    if let Some(stored) = stored_file(path).await {
        return Ok(stored);
    }
    fetch_catalogue_image(state, artist, artwork, path).await
}

async fn fetch_picture(
    state: &AppState,
    artist: &str,
    index: usize,
    wanted: &Picture,
) -> ApiResult<PathBuf> {
    let url = Url::parse(&wanted.url)
        .ok()
        .filter(|url| is_picture_source(url, &state.config.audiodb_url));
    let artists_dir = &state.config.artists_dir;
    fetch_image(state, url, &state.config.audiodb_url, |extension| {
        picture_path(artists_dir, artist, index, extension)
    })
    .await
    .inspect_err(|e| tracing::warn!("[artist-info] {artist}: Bild {index}: {e}"))
    .map_err(|_| ApiError::not_found(NO_PICTURE))
}

/// The stored cover of song number `index` of the artist's most played, fetched now if this is
/// the first request for it. 404 "Kein Bild" for anything that cannot be had.
pub async fn top_cover(state: &AppState, artist: &str, index: usize) -> ApiResult<PathBuf> {
    let not_found = || ApiError::not_found(NO_PICTURE);
    let info = stored(&state.db, artist).await?.ok_or_else(not_found)?;
    let song = info.info.top_tracks.get(index).ok_or_else(not_found)?;
    let artwork = song.artwork.as_deref().ok_or_else(not_found)?;
    if let Some(path) = stored_cover(&state.config.artists_dir, artist, index).await {
        return Ok(path);
    }
    fetch_cover(state, artist, index, artwork).await
}

async fn stored_cover(artists_dir: &Path, artist: &str, index: usize) -> Option<PathBuf> {
    for (_, extension) in IMAGE_TYPES {
        let path = cover_path(artists_dir, artist, index, extension);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Some(path);
        }
    }
    None
}

async fn fetch_cover(
    state: &AppState,
    artist: &str,
    index: usize,
    artwork: &str,
) -> ApiResult<PathBuf> {
    let url = Url::parse(artwork)
        .ok()
        .filter(|url| is_cover_source(url, &state.config.music_catalog_url));
    let artists_dir = &state.config.artists_dir;
    fetch_image(state, url, &state.config.music_catalog_url, |extension| {
        cover_path(artists_dir, artist, index, extension)
    })
    .await
    .inspect_err(|e| tracing::warn!("[artist-info] {artist}: Cover {index}: {e}"))
    .map_err(|_| ApiError::not_found(NO_PICTURE))
}

/// Fetches `url` (`None`: not an address to fetch from) and stores it at the path `path` gives
/// for the image's extension.
async fn fetch_image(
    state: &AppState,
    url: Option<Url>,
    catalogue: &str,
    path: impl Fn(&str) -> PathBuf,
) -> Result<PathBuf, String> {
    let url = url.ok_or("keine erlaubte Adresse")?;
    let (extension, bytes) = download(state, &url, catalogue).await?;
    let path = path(extension);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| e.to_string())?;
    }
    // Written beside and moved into place, so a second request never reads half a file.
    let partial = path.with_extension(format!("part-{}", std::process::id()));
    tokio::fs::write(&partial, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&partial, &path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(id: i32, title: &str, artist: &str, duration: f64, album: &str) -> TrackHit {
        TrackHit {
            id,
            title: Some(title.into()),
            artist: Some(artist.into()),
            duration,
            item_id: id * 10,
            item_title: album.into(),
            item_kind: "album".into(),
            has_lyrics: false,
            loudness: Default::default(),
        }
    }

    fn top(title: &str, album: &str, duration: f64) -> TopTrack {
        TopTrack {
            title: title.into(),
            album: album.into(),
            duration,
            artwork: None,
        }
    }

    #[test]
    fn names_and_titles_compare_without_punctuation_and_additions() {
        assert_eq!(strict("AC/DC"), "acdc");
        assert_eq!(
            loose("Back In Black (Remastered 2011) - Single Version"),
            "backinblack"
        );
        assert_eq!(
            without_additions("(I Can't Get No) Satisfaction"),
            "Satisfaction"
        );
        assert_eq!(without_additions("(Intro)"), "(Intro)");
    }

    #[test]
    fn a_popular_title_finds_the_artists_own_studio_take() {
        let hits = [
            hit(1, "Thunderstruck (Live - 1991)", "AC/DC", 395.0, "Live"),
            hit(2, "Thunderstruck", "AC/DC", 292.0, "Iron Man 2"),
            hit(3, "Thunderstruck", "AC/DC", 293.0, "The Razors Edge"),
            hit(
                4,
                "Thunderstruck",
                "Some Cover Band",
                292.0,
                "The Razors Edge",
            ),
        ];
        let wanted = top("Thunderstruck", "The Razors Edge", 292.0);
        assert_eq!(best_hit(&hits, "AC/DC", &wanted).map(|h| h.id), Some(3));
        // Only a live take is there: better that than nothing to play.
        assert_eq!(
            best_hit(&hits[..1], "AC/DC", &wanted).map(|h| h.id),
            Some(1)
        );
        assert!(best_hit(&hits, "AC/DC", &top("Hells Bells", "Back In Black", 300.0)).is_none());
    }

    #[test]
    fn a_track_without_its_own_artist_belongs_to_the_album_artist() {
        let mut untagged = hit(1, "Thunderstruck", "", 292.0, "The Razors Edge");
        untagged.artist = None;
        let wanted = top("Thunderstruck", "The Razors Edge", 292.0);
        assert_eq!(
            best_hit(&[untagged], "AC/DC", &wanted).map(|h| h.id),
            Some(1)
        );
    }

    #[test]
    fn the_name_as_written_wins_over_one_that_only_shares_its_letters() {
        let found = json!({"results": [
            {"artistId": 1, "artistName": "Afro B"},
            {"artistId": 2, "artistName": "afrob"},
        ]});
        assert_eq!(pick_artist(&found, "Afrob"), Some(2));
        assert_eq!(pick_artist(&found, "Afro-B"), Some(1));
        assert_eq!(pick_artist(&found, "Adele"), None);
        assert_eq!(pick_artist(&json!({}), "Adele"), None);
    }

    #[test]
    fn the_chart_keeps_one_version_of_each_song() {
        let songs = json!({"results": [
            {"wrapperType": "artist", "artistName": "AC/DC"},
            {"trackName": "Thunderstruck", "collectionName": "The Razors Edge", "trackTimeMillis": 292000},
            {"trackName": "Thunderstruck (Live)", "collectionName": "Live", "trackTimeMillis": 332000},
            {"trackName": "Big Gun", "collectionName": "Big Gun - Single", "trackTimeMillis": 260500},
        ]});
        assert_eq!(
            parse_top_tracks(&songs),
            vec![
                top("Thunderstruck", "The Razors Edge", 292.0),
                top("Big Gun", "Big Gun", 260.5)
            ]
        );
    }

    #[test]
    fn albums_are_the_artists_long_records_newest_first_each_once() {
        let body = json!({"results": [
            {"wrapperType": "artist", "artistName": "Jennifer Lopez"},
            {"wrapperType": "collection", "collectionName": "On the 6", "trackCount": 14,
             "releaseDate": "1999-06-01T07:00:00Z",
             "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/a/100x100bb.jpg"},
            {"wrapperType": "collection", "collectionName": "This Is Me...Now", "trackCount": 13,
             "releaseDate": "2024-02-16T08:00:00Z"},
            {"wrapperType": "collection", "collectionName": "This Is Me...Now (Deluxe)", "trackCount": 16,
             "releaseDate": "2024-02-16T08:00:00Z"},
            {"wrapperType": "collection", "collectionName": "Can't Get Enough - Single", "trackCount": 1,
             "releaseDate": "2024-01-10T08:00:00Z"},
            {"wrapperType": "collection", "collectionName": "Remixes - EP", "trackCount": 6},
            {"wrapperType": "collection", "collectionName": "Short", "trackCount": 3},
            {"trackName": "Waiting for Tonight", "collectionName": "On the 6"},
        ]});
        let albums = parse_albums(&body);
        let titles: Vec<&str> = albums.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["This Is Me...Now", "On the 6"]);
        assert_eq!(albums[1].year, Some(1999));
        assert_eq!(albums[1].track_count, 14);
        assert_eq!(
            albums[1].artwork.as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/thumb/a/600x600bb.jpg")
        );
    }

    #[test]
    fn an_album_here_in_another_edition_is_not_missing() {
        let album = |title: &str| CatalogAlbum {
            title: title.into(),
            year: None,
            track_count: 12,
            artwork: None,
        };
        let catalogue = [
            album("LOVE?"),
            album("Brave"),
            album("This Is Me...Then"),
            album("Classics: Night Songs"),
            album("Harbour Lights: Live"),
            album("Early Days (Remastered)"),
        ];
        let library = [
            "LOVE? (Deluxe Edition)".to_owned(),
            "this is me... then".to_owned(),
            "Night Songs (Classics)".to_owned(),
            "Harbour Lights LIVE".to_owned(),
            "Early Days (Deluxe Edition)".to_owned(),
        ];
        let missing: Vec<(usize, &str)> = missing_albums(&catalogue, &library)
            .into_iter()
            .map(|(i, a)| (i, a.title.as_str()))
            .collect();
        assert_eq!(missing, [(1, "Brave")]);
    }

    #[test]
    fn a_songs_cover_is_asked_for_larger_than_the_catalogue_names_it() {
        let songs = json!({"results": [
            {"trackName": "Thunderstruck", "collectionName": "The Razors Edge",
             "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/Music/ab/source/100x100bb.jpg"},
            {"trackName": "Big Gun", "collectionName": "Big Gun", "artworkUrl100": ""},
        ]});
        let chart = parse_top_tracks(&songs);
        assert_eq!(
            chart[0].artwork.as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/thumb/Music/ab/source/600x600bb.jpg")
        );
        assert_eq!(chart[1].artwork, None);
    }

    #[test]
    fn covers_come_only_from_apples_image_hosts_or_the_configured_catalogue() {
        let allowed = |url: &str, base: &str| is_cover_source(&Url::parse(url).unwrap(), base);
        let base = CATALOG_URL;
        assert!(allowed("https://is1-ssl.mzstatic.com/image/a.jpg", base));
        assert!(!allowed("http://is1-ssl.mzstatic.com/image/a.jpg", base));
        assert!(!allowed("https://mzstatic.com.example.org/a.jpg", base));
        assert!(!allowed("https://192.168.1.1/a.jpg", base));
        assert!(allowed(
            "http://127.0.0.1:4000/art/a.jpg",
            "http://127.0.0.1:4000/apple"
        ));
        assert!(!allowed(
            "http://127.0.0.1:4001/art/a.jpg",
            "http://127.0.0.1:4000/apple"
        ));
    }

    #[test]
    fn theaudiodb_gives_text_facts_and_pictures_for_exactly_this_artist() {
        let body = json!({"artists": [
            {"strArtist": "AC/DC Tribute", "strBiography": "Not them."},
            {"strArtist": "AC/DC", "strBiography": "A band.", "strBiographyDE": "Eine Band.",
             "strBiographyFR": "  ", "strGenre": "Hard Rock", "strCountry": "Sydney, Australia",
             "intFormedYear": "1973", "intBornYear": "0", "intMembers": 5,
             "strArtistThumb": "https://r2.theaudiodb.com/t.jpg",
             "strArtistFanart": "https://r2.theaudiodb.com/1.jpg", "strArtistFanart2": null},
        ]});
        let info = parse_audiodb(&body, "AC/DC");
        assert_eq!(info.biography("de"), Some(("de".to_owned(), "Eine Band.")));
        assert_eq!(info.biography("fr"), Some(("en".to_owned(), "A band.")));
        assert_eq!(info.genre.as_deref(), Some("Hard Rock"));
        assert_eq!(info.formed_year, Some(1973));
        assert_eq!(info.born_year, None);
        assert_eq!(info.members, Some(5));
        assert_eq!(
            info.pictures
                .iter()
                .map(|p| p.kind.as_str())
                .collect::<Vec<_>>(),
            ["portrait", "backdrop"]
        );
        assert!(parse_audiodb(&body, "Adele").is_empty());
        assert!(parse_audiodb(&json!({"artists": null}), "Adele").is_empty());
    }

    #[test]
    fn pictures_come_only_from_the_catalogue() {
        let base = "https://www.theaudiodb.com/api/v1/json/123";
        let allowed = |url: &str, base: &str| is_picture_source(&Url::parse(url).unwrap(), base);
        assert!(allowed("https://r2.theaudiodb.com/images/a.jpg", base));
        assert!(allowed("https://www.theaudiodb.com/images/a.jpg", base));
        assert!(!allowed("http://r2.theaudiodb.com/images/a.jpg", base));
        assert!(!allowed("https://eviltheaudiodb.com/a.jpg", base));
        assert!(!allowed("http://192.168.1.1/a.jpg", base));
        assert!(allowed(
            "http://127.0.0.1:4000/pic/a.jpg",
            "http://127.0.0.1:4000/adb"
        ));
        assert!(!allowed(
            "http://127.0.0.1:4001/pic/a.jpg",
            "http://127.0.0.1:4000/adb"
        ));
    }

    #[test]
    fn an_unknown_name_is_asked_for_again_sooner_than_a_known_one() {
        let now = Utc::now();
        let aged = |found, days| ArtistInfo {
            found,
            info: Stored {
                albums: Some(Vec::new()),
                ..Stored::default()
            },
            fetched_at: Some(now - chrono::Duration::days(days)),
        };
        assert!(is_fresh(&aged(true, 29), now, 30.0));
        assert!(!is_fresh(&aged(true, 31), now, 30.0));
        assert!(is_fresh(&aged(false, 6), now, 30.0));
        assert!(!is_fresh(&aged(false, 8), now, 30.0));
        // Asked weekly, both wait the same; asked never again, nothing kept ages.
        assert!(!is_fresh(&aged(true, 8), now, 7.0));
        assert!(is_fresh(&aged(true, 400), now, 0.0));
        assert!(is_fresh(&aged(false, 400), now, 0.0));
        let never_asked = ArtistInfo {
            found: false,
            info: Stored::default(),
            fetched_at: None,
        };
        assert!(!is_fresh(&never_asked, now, 0.0));
        // A found answer from before albums is outdated at once; an unknown name has none anyway.
        let mut without_albums = aged(true, 1);
        without_albums.info.albums = None;
        assert!(!is_fresh(&without_albums, now, 30.0));
        without_albums.info.albums = Some(Vec::new());
        assert!(is_fresh(&without_albums, now, 30.0));
        assert!(is_fresh(&aged(false, 1), now, 30.0));
    }
}
