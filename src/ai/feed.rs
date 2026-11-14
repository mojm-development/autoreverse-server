//! The "Entdecken" page: suggestions generated in the background and kept per user, plus rows
//! computed from the database on every view (music and spoken word) that need no model.
//!
//! A page view never waits for a model and never pays for one directly. A view finds the stored
//! result and, when it is old or the library has grown, starts one background run per user.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Json;

use super::AiConfig;
use super::music::{
    self, AiFailure, CatalogAlbum, DiscoveredArtist, PlaylistTrack, SYSTEM, Suggestion,
};
use super::musicbrainz::{self, MissingAlbum};
use crate::library::queries;
use crate::serialize::{BookSummary, NewEpisodeOut};
use crate::state::AppState;

const MAX_AGE_HOURS: i32 = 24;
/// A failing provider or a busy scan must not turn every page view into a paid request.
const MIN_RETRY: Duration = Duration::from_secs(3600);
const MAX_SUGGESTIONS: usize = 12;
const MAX_MIXES: usize = 4;
const MAX_MIX_ALBUMS: usize = 20;
const RAIL_LENGTH: i64 = 14;
const MAX_WANTED: usize = 16;
const CANDIDATES_PER_ARTIST: usize = 25;
/// Length of the rows of spoken word: a row to pick from, not a second library list.
const SPOKEN_ROW_LENGTH: i64 = 12;
/// A book untouched for this long is offered again. Shorter than for albums (90 days): whoever
/// puts a book down for a month has lost the thread, not just the habit.
const BOOK_IDLE_DAYS: i32 = 30;

#[derive(Clone, Copy)]
pub struct Attempt {
    started: Instant,
    running: bool,
}

pub type Attempts = HashMap<i32, Attempt>;

#[derive(Serialize, Deserialize, Clone)]
pub struct Mix {
    pub name: String,
    pub description: String,
    pub cover_ids: Vec<i32>,
    pub tracks: Vec<PlaylistTrack>,
}

/// An album the library lacks, chosen for this user out of what MusicBrainz lists.
#[derive(Serialize, Deserialize, Clone)]
pub struct WantedAlbum {
    pub artist: String,
    pub title: String,
    pub year: Option<i32>,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Stored {
    wanted: Vec<WantedAlbum>,
    suggestions: Vec<Suggestion>,
    mixes: Vec<Mix>,
    artists: Vec<DiscoveredArtist>,
    truncated: bool,
}

#[derive(Serialize)]
pub struct FeedOut {
    enabled: bool,
    refreshing: bool,
    has_history: bool,
    /// Still comparing with MusicBrainz, so more wanted albums may follow.
    checking_catalog: bool,
    generated_at: Option<DateTime<Utc>>,
    truncated: bool,
    suggestions: Vec<Suggestion>,
    mixes: Vec<Mix>,
    artists: Vec<DiscoveredArtist>,
    wanted: Vec<WantedAlbum>,
    rediscover: Vec<Suggestion>,
    fresh: Vec<Suggestion>,
    // Spoken word, straight from the database: there with or without a model, and with or
    // without any music in the library.
    /// Unheard episodes of the subscriptions, newest first — the head of `/podcasts/new`.
    new_episodes: Vec<NewEpisodeOut>,
    /// Books this user never began, the latest additions first.
    unstarted_books: Vec<BookSummary>,
    /// The next unbegun volume of every series this user finished a book of.
    continue_series: Vec<BookSummary>,
    /// Books begun, not finished and not touched for a while.
    rediscover_books: Vec<BookSummary>,
}

fn lock(state: &AppState) -> std::sync::MutexGuard<'_, Attempts> {
    state
        .ai_feed_attempts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// Starts a run unless one is active or the last began too recently. Returns whether one runs now.
pub fn start(state: &AppState, user_id: i32, force: bool) -> bool {
    let Some(config) = state.settings.get().ai() else {
        return false;
    };
    {
        let mut attempts = lock(state);
        if let Some(attempt) = attempts.get(&user_id) {
            if attempt.running {
                return true;
            }
            if !force && attempt.started.elapsed() < MIN_RETRY {
                return false;
            }
        }
        attempts.insert(
            user_id,
            Attempt {
                started: Instant::now(),
                running: true,
            },
        );
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(failure) = run(&state, &config, user_id).await {
            warn_about(&failure);
        }
        if let Some(attempt) = lock(&state).get_mut(&user_id) {
            attempt.running = false;
        }
    });
    true
}

fn warn_about(failure: &AiFailure) {
    match failure {
        AiFailure::Database(e) => tracing::warn!("[ai] {e}"),
        AiFailure::Model(e) => tracing::warn!("[ai] {e}"),
    }
}

async fn run(state: &AppState, config: &AiConfig, user_id: i32) -> Result<(), AiFailure> {
    let _permit = state
        .ai_semaphore
        .acquire()
        .await
        .map_err(|e| AiFailure::Model(e.to_string()))?;
    let (albums, truncated) = music::catalog(&state.db).await?;
    let (taste, heard) = music::listening_taste(&state.db, user_id).await?;
    // Without a history there is nothing to base a suggestion on, and nothing worth paying for.
    if taste.is_empty() {
        return store(state, user_id, &Stored::default()).await;
    }
    let answer = config
        .generate(&state.http, SYSTEM, &prompt(&albums, &taste), &schema())
        .await?;
    let suggestions = checked_suggestions(&answer, &albums, &heard);
    let mixes = build_mixes(&state.db, &answer, &albums).await?;
    let previous = previous_feed(&state.db, user_id).await?;
    let mixes = if mixes.is_empty() {
        tracing::warn!("[ai] keine brauchbaren Mixes in der Antwort, die bisherigen bleiben");
        previous.mixes
    } else {
        mixes
    };
    // These are separate requests; one failing must not throw away what already succeeded.
    let artists = match music::discover(config, &state.http, &state.db, user_id).await {
        Ok(artists) => artists,
        Err(failure) => {
            warn_about(&failure);
            previous.artists
        }
    };
    let wanted = match wanted_albums(state, config, user_id, &taste).await {
        Ok(wanted) => wanted,
        Err(failure) => {
            warn_about(&failure);
            previous.wanted
        }
    };
    let stored = Stored {
        wanted,
        suggestions,
        mixes,
        artists,
        truncated,
    };
    store(state, user_id, &stored).await
}

/// What to fall back on, so that a partly failed run does not cost the user what they have.
async fn previous_feed(db: &PgPool, user_id: i32) -> Result<Stored, sqlx::Error> {
    let row: Option<Json<Stored>> =
        sqlx::query_scalar("SELECT payload FROM ai_feed WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|stored| stored.0).unwrap_or_default())
}

async fn store(state: &AppState, user_id: i32, stored: &Stored) -> Result<(), AiFailure> {
    sqlx::query(
        "INSERT INTO ai_feed (user_id, payload) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET payload = excluded.payload, generated_at = now()",
    )
    .bind(user_id)
    .bind(Json(stored))
    .execute(&state.db)
    .await?;
    Ok(())
}

/// The model only ranks: every title comes from MusicBrainz, so none can be invented.
async fn wanted_albums(
    state: &AppState,
    config: &AiConfig,
    user_id: i32,
    taste: &str,
) -> Result<Vec<WantedAlbum>, AiFailure> {
    let artists = music::heard_artists(&state.db, user_id).await?;
    let candidates = musicbrainz::candidates(&state.db, &artists, CANDIDATES_PER_ARTIST).await?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let lines = candidates
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let year = a.year.map(|y| y.to_string()).unwrap_or_default();
            format!("{i}|{}|{}|{year}", a.artist, a.title)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "Bisher gern gehört:\n{taste}\n\nAlben, die in der Sammlung fehlen \
         (nr|Interpret|Titel|Jahr):\n{lines}\n\nWähle höchstens {MAX_WANTED} Alben aus, die dieser \
         Mensch wirklich haben wollen würde. Lieber wenige und treffende als viele. Lass \
         Kuriositäten, Tribute, Kinder- und Sprachkurs-Alben weg, wenn sie nicht zum Gehörten \
         passen. Begründe jedes in einem kurzen Satz mit Bezug auf das Gehörte."
    );
    let schema = json!({
        "type": "object",
        "properties": {"albums": {"type": "array", "items": {
            "type": "object",
            "properties": {"nr": {"type": "integer"}, "reason": {"type": "string"}},
            "required": ["nr", "reason"],
            "additionalProperties": false,
        }}},
        "required": ["albums"],
        "additionalProperties": false,
    });
    let answer = config
        .generate(&state.http, SYSTEM, &prompt, &schema)
        .await?;
    Ok(pick_wanted(&answer, &candidates))
}

fn pick_wanted(answer: &Value, candidates: &[MissingAlbum]) -> Vec<WantedAlbum> {
    let mut seen = HashSet::new();
    answer["albums"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let nr = usize::try_from(entry["nr"].as_i64()?).ok()?;
            let album = candidates.get(nr)?;
            seen.insert(nr).then(|| WantedAlbum {
                artist: album.artist.clone(),
                title: album.title.clone(),
                year: album.year,
                reason: entry["reason"].as_str().unwrap_or("").to_owned(),
            })
        })
        .take(MAX_WANTED)
        .collect()
}

fn prompt(albums: &[CatalogAlbum], taste: &str) -> String {
    format!(
        "Bisher gern gehört:\n{taste}\n\nAlben der Sammlung (id|Interpret|Titel|Jahr|Genre):\n{}\n\n\
         1. albums: Wähle bis zu {MAX_SUGGESTIONS} Alben, die zu diesem Geschmack passen und oben \
         noch nicht als gehört stehen. Begründe jedes in einem kurzen, konkreten Satz.\n\
         2. mixes: Erfinde {MAX_MIXES} unterschiedliche Mixes nach Stimmung oder Anlass, die zu diesem \
         Geschmack passen, jeweils mit kurzem Namen, einem Satz Beschreibung und möglichst {MAX_MIX_ALBUMS} passenden Album-ids.",
        music::catalog_lines(albums)
    )
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "albums": {"type": "array", "items": {
                "type": "object",
                "properties": {"id": {"type": "integer"}, "reason": {"type": "string"}},
                "required": ["id", "reason"],
                "additionalProperties": false,
            }},
            "mixes": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "description": {"type": "string"},
                    "ids": {"type": "array", "items": {"type": "integer"}},
                },
                "required": ["name", "description", "ids"],
                "additionalProperties": false,
            }},
        },
        "required": ["albums", "mixes"],
        "additionalProperties": false,
    })
}

fn checked_suggestions(
    answer: &Value,
    albums: &[CatalogAlbum],
    heard: &HashSet<i32>,
) -> Vec<Suggestion> {
    let by_id: HashMap<i32, &CatalogAlbum> = albums.iter().map(|a| (a.id, a)).collect();
    let mut seen = HashSet::new();
    answer["albums"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = i32::try_from(entry["id"].as_i64()?).ok()?;
            let album = by_id.get(&id)?;
            (!heard.contains(&id) && seen.insert(id))
                .then(|| album.suggestion(entry["reason"].as_str().unwrap_or("")))
        })
        .take(MAX_SUGGESTIONS)
        .collect()
}

/// A model picks albums, the server picks tracks. That needs one request instead of one per mix,
/// and the track list only ever holds what exists.
async fn build_mixes(
    db: &PgPool,
    answer: &Value,
    albums: &[CatalogAlbum],
) -> Result<Vec<Mix>, sqlx::Error> {
    let known: HashMap<i32, &CatalogAlbum> = albums.iter().map(|a| (a.id, a)).collect();
    let known_ids: HashSet<i32> = known.keys().copied().collect();
    let mut mixes = Vec::new();
    for entry in answer["mixes"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_MIXES)
    {
        let ids = music::known_ids(entry, "ids", &known_ids, MAX_MIX_ALBUMS);
        let tracks = music::sample_tracks(db, &ids, music::DEFAULT_SECONDS).await?;
        if tracks.is_empty() {
            continue;
        }
        mixes.push(Mix {
            name: entry["name"]
                .as_str()
                .unwrap_or("Mix")
                .chars()
                .take(80)
                .collect(),
            description: entry["description"].as_str().unwrap_or("").to_owned(),
            cover_ids: ids
                .iter()
                .filter(|id| known[id].has_cover)
                .take(4)
                .copied()
                .collect(),
            tracks,
        });
    }
    Ok(mixes)
}

/// Albums listened to properly, but not for a while.
async fn rediscover(db: &PgPool, user_id: i32) -> Result<Vec<Suggestion>, sqlx::Error> {
    let rows: Vec<CatalogAlbum> = sqlx::query_as(
        "SELECT i.id, coalesce(i.album_artist, i.artist) AS artist, i.title, i.year, i.genre,
                i.cover_path IS NOT NULL AS has_cover
         FROM items i
         JOIN (SELECT item_id, sum(seconds) AS heard, max(at) AS last FROM listen_events
               WHERE user_id = $1 GROUP BY item_id) l ON l.item_id = i.id
         WHERE i.kind = 'album' AND i.missing_since IS NULL AND i.duplicate_of IS NULL
           AND l.last < now() - interval '90 days'
         ORDER BY l.heard DESC LIMIT $2",
    )
    .bind(user_id)
    .bind(RAIL_LENGTH)
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(|a| a.suggestion("")).collect())
}

/// Recently added, unplayed by this user, and by an artist or in a genre they do listen to.
async fn fresh(db: &PgPool, user_id: i32) -> Result<Vec<Suggestion>, sqlx::Error> {
    let rows: Vec<CatalogAlbum> = sqlx::query_as(
        "SELECT i.id, coalesce(i.album_artist, i.artist) AS artist, i.title, i.year, i.genre,
                i.cover_path IS NOT NULL AS has_cover
         FROM items i
         WHERE i.kind = 'album' AND i.missing_since IS NULL AND i.duplicate_of IS NULL
           AND i.added_at > now() - interval '60 days'
           AND NOT EXISTS (SELECT 1 FROM listen_events l
                           WHERE l.user_id = $1 AND l.item_id = i.id)
           AND EXISTS (SELECT 1 FROM listen_events l JOIN items h ON h.id = l.item_id
                       WHERE l.user_id = $1
                         AND (coalesce(h.album_artist, h.artist) = coalesce(i.album_artist, i.artist)
                              OR lower(h.genre) = lower(i.genre)))
         ORDER BY i.added_at DESC, i.id DESC LIMIT $2",
    )
    .bind(user_id)
    .bind(RAIL_LENGTH)
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(|a| a.suggestion("")).collect())
}

/// Drops what vanished from the library since the feed was generated, and reads the tracks'
/// lyrics flags afresh: the payload keeps what was true when the model ran, and a scan may have
/// found a `.lrc` since (or a payload may predate the field).
async fn without_removed(db: &PgPool, stored: &mut Stored) -> Result<(), sqlx::Error> {
    let mut ids: Vec<i32> = stored.suggestions.iter().map(|s| s.id).collect();
    ids.extend(
        stored
            .mixes
            .iter()
            .flat_map(|m| m.tracks.iter().map(|t| t.item_id)),
    );
    let present: HashSet<i32> =
        sqlx::query_scalar("SELECT id FROM items WHERE id = ANY($1) AND missing_since IS NULL")
            .bind(&ids)
            .fetch_all(db)
            .await?
            .into_iter()
            .collect();
    let track_ids: Vec<i32> = stored
        .mixes
        .iter()
        .flat_map(|m| m.tracks.iter().map(|t| t.id))
        .collect();
    let lyrics: HashMap<i32, bool> =
        sqlx::query_as::<_, (i32, bool)>("SELECT id, has_lyrics FROM tracks WHERE id = ANY($1)")
            .bind(&track_ids)
            .fetch_all(db)
            .await?
            .into_iter()
            .collect();
    stored.suggestions.retain(|s| present.contains(&s.id));
    for mix in &mut stored.mixes {
        mix.tracks
            .retain(|t| present.contains(&t.item_id) && lyrics.contains_key(&t.id));
        for track in &mut mix.tracks {
            track.has_lyrics = lyrics[&track.id];
        }
        mix.cover_ids.retain(|id| present.contains(id));
    }
    stored.mixes.retain(|m| !m.tracks.is_empty());
    Ok(())
}

pub async fn load(state: &AppState, user_id: i32) -> Result<FeedOut, sqlx::Error> {
    let pending = musicbrainz::pending_artists(&state.db).await?;
    if !pending.is_empty() {
        musicbrainz::start_refresh(state);
    }
    let checking_catalog = state.musicbrainz_running.load(Ordering::SeqCst);
    let has_history: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM listen_events WHERE user_id = $1)
                OR EXISTS (SELECT 1 FROM favorites WHERE user_id = $1)",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    let row: Option<(Json<Stored>, DateTime<Utc>, bool)> = sqlx::query_as(
        "SELECT payload, generated_at,
                generated_at < now() - make_interval(hours => $2)
                OR EXISTS (SELECT 1 FROM items WHERE kind = 'album' AND added_at > generated_at)
                OR ($3 AND EXISTS (SELECT 1 FROM musicbrainz_artists WHERE fetched_at > generated_at))
         FROM ai_feed WHERE user_id = $1",
    )
    .bind(user_id)
    .bind(MAX_AGE_HOURS)
    .bind(!checking_catalog)
    .fetch_optional(&state.db)
    .await?;
    let empty_despite_history = has_history
        && row
            .as_ref()
            .is_some_and(|r| r.0.0.suggestions.is_empty() && r.0.0.mixes.is_empty());
    let due = row.as_ref().is_none_or(|r| r.2) || empty_despite_history;
    let generated_at = row.as_ref().map(|r| r.1);
    let mut stored = row.map(|r| r.0.0).unwrap_or_default();
    without_removed(&state.db, &mut stored).await?;
    let refreshing = if due {
        start(state, user_id, false)
    } else {
        lock(state).get(&user_id).is_some_and(|a| a.running)
    };
    // None of the rows below goes through the stored payload. That one exists because a model
    // run costs money and time; these are a handful of indexed queries, and they have to be right
    // the moment an episode arrives or a book is begun — a row that still offers yesterday's
    // state for up to a day would be worse than none.
    let db = &state.db;
    let (rediscover, fresh, new_episodes, unstarted_books, continue_series, rediscover_books) = tokio::try_join!(
        rediscover(db, user_id),
        fresh(db, user_id),
        queries::new_episodes(db, user_id, SPOKEN_ROW_LENGTH),
        queries::unstarted_books(db, user_id, SPOKEN_ROW_LENGTH),
        queries::next_in_series(db, user_id, SPOKEN_ROW_LENGTH),
        queries::paused_books(db, user_id, BOOK_IDLE_DAYS, SPOKEN_ROW_LENGTH),
    )?;
    Ok(FeedOut {
        enabled: state.settings.get().ai().is_some(),
        refreshing,
        has_history,
        checking_catalog,
        generated_at,
        truncated: stored.truncated,
        suggestions: stored.suggestions,
        mixes: stored.mixes,
        artists: stored.artists,
        wanted: stored.wanted,
        rediscover,
        fresh,
        new_episodes: new_episodes.into_iter().map(NewEpisodeOut::from).collect(),
        unstarted_books: unstarted_books.iter().map(BookSummary::from).collect(),
        continue_series: continue_series.iter().map(BookSummary::from).collect(),
        rediscover_books: rediscover_books.iter().map(BookSummary::from).collect(),
    })
}
