//! Radio: tracks from the library that go with a track, an album or an artist (`GET /radio`).
//!
//! No model and no outside service: the answer comes from what the tags and the user's own
//! listening say. Albums are scored against the seed (genre, year, artist) and against the
//! user's taste, with the same measure the suggestions use (`ai::music`: listening below
//! `MIN_SECONDS_FOR_TASTE` says nothing, a favourite counts like `FAVORITE_SECONDS` heard), plus
//! a share of chance, so two calls differ. The best albums each give a track, round after round
//! — close ones first, so a radio starts near its seed and opens up from there.
//!
//! Scoring is per album because everything known is known per album: genre and year sit on the
//! item, and `listen_events` books time to items. Two queries whatever the library's size: every
//! present album with the user's numbers, then the tracks of the albums that made it.

use std::collections::{HashMap, HashSet, VecDeque};

use sqlx::PgPool;

use super::queries::{self, TrackHit};
use crate::ai::music::{FAVORITE_SECONDS, MIN_SECONDS_FOR_TASTE};

pub const DEFAULT_LIMIT: i64 = 50;
pub const MAX_LIMIT: i64 = 200;
/// What a client may name as already queued.
pub const MAX_EXCLUDED: usize = 500;

/// How much each sign of closeness counts. Genre leads, because it is what a listener hears
/// first; chance is as wide as artist and year together, so the order differs from call to call
/// without an unrelated album overtaking one that shares the genre and the years.
const GENRE_WEIGHT: f64 = 3.0;
const ARTIST_WEIGHT: f64 = 2.0;
const YEAR_WEIGHT: f64 = 1.5;
const TASTE_WEIGHT: f64 = 1.0;
const CHANCE_WEIGHT: f64 = 2.0;
/// A favourite track is a little more likely to be the one its album gives.
const LIKED_TRACK_BONUS: f64 = 0.6;
/// Albums this close in years count as the same time; from `YEAR_FAR` on, as unrelated.
const YEAR_NEAR: f64 = 3.0;
const YEAR_FAR: f64 = 15.0;
/// "A few per album", however short the list would otherwise be.
const MAX_PER_ALBUM: usize = 3;
/// Listening that fills the taste score.
const FULL_TASTE_SECONDS: f64 = 3600.0;

/// What a radio starts from.
#[derive(Debug, Clone, PartialEq)]
pub enum RadioSeed {
    Track(i32),
    Album(i32),
    Artist(String),
}

#[derive(Debug)]
pub enum RadioError {
    UnknownTrack,
    UnknownItem,
    UnknownArtist,
    /// The seed is a book, a podcast or an episode.
    NotMusic,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for RadioError {
    fn from(e: sqlx::Error) -> Self {
        RadioError::Database(e)
    }
}

/// A present album with what this user makes of it.
#[derive(Debug, Clone, sqlx::FromRow)]
struct Album {
    id: i32,
    /// Lower-cased album artist, `None` where the tags name nobody.
    artist: Option<String>,
    genre: Option<String>,
    year: Option<i32>,
    heard: f64,
    liked: bool,
    /// The seed's artist made it or plays on it (a sampler).
    by_seed_artist: bool,
}

/// A track on offer. `artist_key` groups for the "not three in a row" rule and the share per
/// artist: the track's own artist, else the album's, else the album itself.
#[derive(Debug, Clone, sqlx::FromRow)]
struct Candidate {
    id: i32,
    title: Option<String>,
    artist: Option<String>,
    duration: f64,
    item_id: i32,
    item_title: String,
    item_kind: String,
    has_lyrics: bool,
    #[sqlx(flatten)]
    loudness: crate::library::queries::Loudness,
    artist_key: Option<String>,
    liked: bool,
}

impl Candidate {
    fn group(&self) -> String {
        self.artist_key
            .clone()
            .unwrap_or_else(|| format!("#{}", self.item_id))
    }

    fn into_hit(self) -> TrackHit {
        TrackHit {
            id: self.id,
            title: self.title,
            artist: self.artist,
            duration: self.duration,
            item_id: self.item_id,
            item_title: self.item_title,
            item_kind: self.item_kind,
            has_lyrics: self.has_lyrics,
            loudness: self.loudness,
        }
    }
}

/// The seed as the scoring sees it.
#[derive(Debug, Clone, Default, PartialEq)]
struct Anchor {
    genres: Vec<String>,
    year: Option<i32>,
    /// The seed artist's group key (lower case), for the share that artist may take.
    artist: Option<String>,
    /// How many tracks the seed's artist may have, and how many any other artist.
    seed_artist_share: usize,
    artist_share: usize,
}

/// Small, fast and good enough to shuffle music (xorshift64*). Not for anything secret.
pub struct Chance(u64);

impl Chance {
    pub fn seeded(seed: u64) -> Self {
        // Zero is the one state xorshift never leaves.
        Chance(seed.max(1))
    }

    pub fn from_os() -> Self {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("the OS random source");
        Chance::seeded(u64::from_le_bytes(bytes))
    }

    /// A number in `[0, 1)`.
    pub fn draw(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let bits = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
        bits as f64 / (1u64 << 53) as f64
    }
}

/// The genres a tag names: "Rock; Pop", "Hip-Hop/Rap" and "Jazz, Soul" each carry two.
fn genre_names(genre: Option<&str>) -> Vec<String> {
    genre
        .unwrap_or("")
        .split([';', '/', ',', '|'])
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

/// 1 for a shared genre, 0.5 for a shared word ("Indie Rock" and "Rock"), else 0.
fn genre_similarity(seed: &[String], other: &[String]) -> f64 {
    if seed.iter().any(|name| other.contains(name)) {
        return 1.0;
    }
    // Short words ("&", "n", "of") would tie everything to everything.
    let words = |names: &[String]| -> HashSet<String> {
        names
            .iter()
            .flat_map(|name| name.split([' ', '-']))
            .filter(|word| word.chars().count() >= 3)
            .map(str::to_owned)
            .collect()
    };
    if words(seed).is_disjoint(&words(other)) {
        0.0
    } else {
        0.5
    }
}

/// 1 within `YEAR_NEAR` years, falling to 0 at `YEAR_FAR`; 0 where either year is unknown.
fn year_closeness(seed: Option<i32>, other: Option<i32>) -> f64 {
    let (Some(seed), Some(other)) = (seed, other) else {
        return 0.0;
    };
    let apart = f64::from((seed - other).abs());
    (1.0 - (apart - YEAR_NEAR) / (YEAR_FAR - YEAR_NEAR)).clamp(0.0, 1.0)
}

/// 0 to 1: how much this user has shown to like the album.
fn taste(album: &Album) -> f64 {
    let seconds = album.heard + if album.liked { FAVORITE_SECONDS } else { 0.0 };
    if seconds < MIN_SECONDS_FOR_TASTE {
        return 0.0;
    }
    (seconds / FULL_TASTE_SECONDS).min(1.0)
}

fn affinity(anchor: &Anchor, album: &Album) -> f64 {
    GENRE_WEIGHT * genre_similarity(&anchor.genres, &genre_names(album.genre.as_deref()))
        + YEAR_WEIGHT * year_closeness(anchor.year, album.year)
        + if album.by_seed_artist {
            ARTIST_WEIGHT
        } else {
            0.0
        }
        + TASTE_WEIGHT * taste(album)
}

/// The albums to take tracks from, closest first: those that fit an artist's share, enough for a
/// track each and some to spare, then as many again of what the share held back — the reserve a
/// small or one-sided library needs to fill the list at all.
fn pick_albums(anchor: &Anchor, albums: Vec<Album>, limit: usize, chance: &mut Chance) -> Vec<i32> {
    let mut scored: Vec<(f64, Album)> = albums
        .into_iter()
        .map(|album| {
            (
                affinity(anchor, &album) + CHANCE_WEIGHT * chance.draw(),
                album,
            )
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    let wanted = limit + 20;
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut fitting = 0;
    let mut reserve = 0;
    let mut picked = Vec::new();
    for (_, album) in scored {
        if fitting >= wanted {
            break;
        }
        let share = if album.by_seed_artist {
            anchor.seed_artist_share
        } else {
            anchor.artist_share
        };
        let within = match &album.artist {
            Some(artist) => {
                let count = per_artist.entry(artist.clone()).or_default();
                *count += 1;
                *count <= share
            }
            None => true,
        };
        if within {
            fitting += 1;
        } else if reserve < limit {
            reserve += 1;
        } else {
            continue;
        }
        picked.push(album.id);
    }
    picked
}

/// One track per album per round, in the albums' order, until `limit` — first within every
/// artist's share, then, if the list is still short, without it. Never more than
/// `MAX_PER_ALBUM` of one album.
fn deal(anchor: &Anchor, mut albums: Vec<VecDeque<Candidate>>, limit: usize) -> Vec<Candidate> {
    let mut dealt = Vec::new();
    let mut taken = vec![0usize; albums.len()];
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    for shared in [true, false] {
        for _round in 0..MAX_PER_ALBUM {
            for (index, album) in albums.iter_mut().enumerate() {
                if dealt.len() >= limit {
                    return dealt;
                }
                if taken[index] >= MAX_PER_ALBUM {
                    continue;
                }
                let fits = |track: &Candidate| {
                    let group = track.group();
                    let share = if anchor.artist.as_deref() == Some(group.as_str()) {
                        anchor.seed_artist_share
                    } else {
                        anchor.artist_share
                    };
                    per_artist.get(&group).copied().unwrap_or(0) < share
                };
                let next = if shared {
                    album.iter().position(fits)
                } else {
                    (!album.is_empty()).then_some(0)
                };
                if let Some(track) = next.and_then(|at| album.remove(at)) {
                    *per_artist.entry(track.group()).or_default() += 1;
                    taken[index] += 1;
                    dealt.push(track);
                }
            }
        }
    }
    dealt
}

/// Moves tracks back so that no artist plays three times in a row, wherever another artist is
/// left to put in between. A list that is one artist's alone stays as it is: a rule of taste is
/// no reason to hand out less music.
fn spread(tracks: Vec<Candidate>) -> Vec<Candidate> {
    let mut rest: VecDeque<Candidate> = tracks.into();
    let mut out: Vec<Candidate> = Vec::with_capacity(rest.len());
    while !rest.is_empty() {
        let run = match out.as_slice() {
            [.., a, b] if a.group() == b.group() => Some(b.group()),
            _ => None,
        };
        let at = run
            .and_then(|run| rest.iter().position(|track| track.group() != run))
            .unwrap_or(0);
        out.extend(rest.remove(at));
    }
    out
}

/// The middle year of the albums that have one.
fn median_year(years: impl Iterator<Item = i32>) -> Option<i32> {
    let mut years: Vec<i32> = years.collect();
    years.sort_unstable();
    years.get(years.len() / 2).copied()
}

/// What the seed's own artist may take: sparingly beside a track or an album (two at least, an
/// eighth of the list), a third when the radio is about that artist. Everybody else gets the
/// sparing share, so no one artist with many albums in a genre fills the list.
fn shares(limit: usize, about_artist: bool) -> (usize, usize) {
    let sparing = (limit / 8).max(2);
    let seed = if about_artist {
        (limit / 3).max(3)
    } else {
        sparing
    };
    (seed, sparing)
}

async fn present_albums(
    db: &PgPool,
    user_id: i32,
    seed_artist: Option<&str>,
) -> Result<Vec<Album>, sqlx::Error> {
    sqlx::query_as(
        "SELECT i.id, lower(coalesce(i.album_artist, i.artist)) AS artist, i.genre, i.year,
                coalesce(l.heard, 0)::double precision AS heard,
                (f.item_id IS NOT NULL OR ft.item_id IS NOT NULL) AS liked,
                coalesce(lower(i.artist) = lower($2) OR lower(i.album_artist) = lower($2)
                         OR s.item_id IS NOT NULL, false) AS by_seed_artist
         FROM items i
         LEFT JOIN (SELECT item_id, sum(seconds) AS heard FROM listen_events
                    WHERE user_id = $1 GROUP BY item_id) l ON l.item_id = i.id
         LEFT JOIN favorites f ON f.user_id = $1 AND f.item_id = i.id
         LEFT JOIN (SELECT DISTINCT t.item_id FROM favorites tf
                    JOIN tracks t ON t.id = tf.track_id
                    WHERE tf.user_id = $1) ft ON ft.item_id = i.id
         LEFT JOIN (SELECT DISTINCT item_id FROM tracks
                    WHERE lower(artist) = lower($2)) s ON s.item_id = i.id
         WHERE i.kind = 'album' AND i.missing_since IS NULL AND i.duplicate_of IS NULL",
    )
    .bind(user_id)
    .bind(seed_artist)
    .fetch_all(db)
    .await
}

async fn tracks_of(
    db: &PgPool,
    user_id: i32,
    albums: &[i32],
    excluded: &[i32],
) -> Result<Vec<Candidate>, sqlx::Error> {
    sqlx::query_as(
        "SELECT t.id, t.title, t.artist, t.duration, t.item_id,
                i.title AS item_title, i.kind AS item_kind, t.has_lyrics,
                t.gain_db, t.album_gain_db, t.peak,
                lower(coalesce(t.artist, i.album_artist, i.artist)) AS artist_key,
                f.track_id IS NOT NULL AS liked
         FROM tracks t JOIN items i ON i.id = t.item_id
         LEFT JOIN favorites f ON f.user_id = $2 AND f.track_id = t.id
         WHERE t.item_id = ANY($1) AND t.id <> ALL($3)
         ORDER BY t.id",
    )
    .bind(albums)
    .bind(user_id)
    .bind(excluded)
    .fetch_all(db)
    .await
}

/// Up to `limit` tracks that go with `seed`: music only, from present albums, none of
/// `excluded`, not the seed track, none of the seed album, none twice.
pub async fn radio(
    db: &PgPool,
    user_id: i32,
    seed: &RadioSeed,
    limit: usize,
    excluded: &[i32],
    chance: &mut Chance,
) -> Result<Vec<TrackHit>, RadioError> {
    let mut excluded = excluded.to_vec();
    let mut without_album = None;
    // The seed first: an unknown one is an error even where the library has nothing to offer.
    let (seed_artist, described) = match seed {
        RadioSeed::Track(id) => {
            let track = queries::track(db, *id)
                .await?
                .ok_or(RadioError::UnknownTrack)?;
            let item = queries::item(db, track.item_id)
                .await?
                .ok_or(RadioError::UnknownTrack)?;
            if item.kind != "album" {
                return Err(RadioError::NotMusic);
            }
            excluded.push(track.id);
            let artist = track
                .artist
                .or_else(|| item.album_artist.clone())
                .or_else(|| item.artist.clone());
            (artist, Some((item.genre, item.year)))
        }
        RadioSeed::Album(id) => {
            let item = queries::item(db, *id)
                .await?
                .ok_or(RadioError::UnknownItem)?;
            if item.kind != "album" {
                return Err(RadioError::NotMusic);
            }
            without_album = Some(item.id);
            let artist = item.album_artist.clone().or_else(|| item.artist.clone());
            (artist, Some((item.genre, item.year)))
        }
        RadioSeed::Artist(name) => (Some(name.clone()), None),
    };
    let mut albums = present_albums(db, user_id, seed_artist.as_deref()).await?;
    let (genres, year) = match described {
        Some((genre, year)) => (genre_names(genre.as_deref()), year),
        // An artist is what their albums are: every genre those carry, and their middle year.
        None => {
            let own: Vec<&Album> = albums.iter().filter(|a| a.by_seed_artist).collect();
            if own.is_empty() {
                return Err(RadioError::UnknownArtist);
            }
            let mut genres: Vec<String> = own
                .iter()
                .flat_map(|a| genre_names(a.genre.as_deref()))
                .collect();
            genres.sort();
            genres.dedup();
            (genres, median_year(own.iter().filter_map(|a| a.year)))
        }
    };
    let (seed_artist_share, artist_share) = shares(limit, matches!(seed, RadioSeed::Artist(_)));
    let anchor = Anchor {
        genres,
        year,
        artist: seed_artist.map(|name| name.to_lowercase()),
        seed_artist_share,
        artist_share,
    };
    albums.retain(|album| Some(album.id) != without_album);
    let order = pick_albums(&anchor, albums, limit, chance);
    let mut by_album: HashMap<i32, Vec<(f64, Candidate)>> = HashMap::new();
    for track in tracks_of(db, user_id, &order, &excluded).await? {
        let key = chance.draw() + if track.liked { LIKED_TRACK_BONUS } else { 0.0 };
        by_album
            .entry(track.item_id)
            .or_default()
            .push((key, track));
    }
    let queues = order
        .iter()
        .filter_map(|album| by_album.remove(album))
        .map(|mut tracks| {
            tracks.sort_by(|a, b| b.0.total_cmp(&a.0));
            tracks.into_iter().map(|(_, track)| track).collect()
        })
        .collect();
    let dealt = spread(deal(&anchor, queues, limit));
    Ok(dealt.into_iter().map(Candidate::into_hit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(raw: &str) -> Vec<String> {
        genre_names(Some(raw))
    }

    #[test]
    fn a_genre_tag_may_name_several() {
        assert_eq!(names("Rock; Pop"), ["rock", "pop"]);
        assert_eq!(names("Hip-Hop/Rap"), ["hip-hop", "rap"]);
        assert_eq!(names(" Jazz "), ["jazz"]);
        assert!(genre_names(None).is_empty());
        assert!(names(" ; ").is_empty());
    }

    #[test]
    fn genres_match_whole_or_by_a_word() {
        assert_eq!(genre_similarity(&names("Rock"), &names("Pop/ROCK")), 1.0);
        assert_eq!(genre_similarity(&names("Indie Rock"), &names("Rock")), 0.5);
        assert_eq!(genre_similarity(&names("Rock"), &names("Jazz")), 0.0);
        // Nothing known is no match, not a match with everything else unknown.
        assert_eq!(genre_similarity(&[], &[]), 0.0);
        assert_eq!(
            genre_similarity(&names("R & B"), &names("Drum & Bass")),
            0.0
        );
    }

    #[test]
    fn years_count_when_close() {
        assert_eq!(year_closeness(Some(1980), Some(1982)), 1.0);
        assert_eq!(year_closeness(Some(1980), Some(1989)), 0.5);
        assert_eq!(year_closeness(Some(1980), Some(2010)), 0.0);
        assert_eq!(year_closeness(None, Some(2010)), 0.0);
    }

    fn album(id: i32, artist: &str, genre: &str, year: i32) -> Album {
        Album {
            id,
            artist: Some(artist.to_owned()),
            genre: Some(genre.to_owned()),
            year: Some(year),
            heard: 0.0,
            liked: false,
            by_seed_artist: false,
        }
    }

    #[test]
    fn taste_needs_real_listening_or_a_favourite() {
        let mut a = album(1, "a", "rock", 1990);
        a.heard = MIN_SECONDS_FOR_TASTE - 1.0;
        assert_eq!(taste(&a), 0.0);
        a.heard = 1800.0;
        assert_eq!(taste(&a), 0.5);
        a.heard = 0.0;
        a.liked = true;
        assert_eq!(taste(&a), 1.0);
    }

    fn anchor(limit: usize) -> Anchor {
        let (seed_artist_share, artist_share) = shares(limit, false);
        Anchor {
            genres: names("Rock"),
            year: Some(1990),
            artist: Some("seed".into()),
            seed_artist_share,
            artist_share,
        }
    }

    #[test]
    fn close_albums_come_before_distant_ones_whatever_the_chance() {
        let albums = vec![
            album(1, "a", "Jazz", 1950),
            album(2, "b", "Rock", 1991),
            album(3, "c", "Klassik", 1900),
            album(4, "d", "Rock", 1989),
        ];
        for seed in 1..50 {
            let order = pick_albums(&anchor(10), albums.clone(), 10, &mut Chance::seeded(seed));
            let mut near = order[..2].to_vec();
            near.sort_unstable();
            assert_eq!(near, [2, 4], "seed {seed}");
        }
    }

    #[test]
    fn chance_changes_the_order() {
        let albums: Vec<Album> = (1..=30)
            .map(|id| album(id, &format!("artist {id}"), "Rock", 1990))
            .collect();
        let first = pick_albums(&anchor(10), albums.clone(), 10, &mut Chance::seeded(1));
        let second = pick_albums(&anchor(10), albums, 10, &mut Chance::seeded(2));
        assert_ne!(first, second);
    }

    #[test]
    fn an_artist_with_many_albums_is_held_back_to_a_reserve() {
        let mut albums: Vec<Album> = (1..=40)
            .map(|id| album(id, "vielschreiber", "Rock", 1990))
            .collect();
        albums.extend((41..=44).map(|id| album(id, &format!("other {id}"), "Jazz", 1950)));
        let order = pick_albums(&anchor(8), albums, 8, &mut Chance::seeded(7));
        // Two within the share, eight held in reserve, and every other artist's album.
        assert_eq!(order.iter().filter(|id| **id <= 40).count(), 10);
        assert_eq!(order.iter().filter(|id| **id > 40).count(), 4);
    }

    fn track(id: i32, album: i32, artist: &str) -> Candidate {
        Candidate {
            id,
            title: None,
            artist: None,
            duration: 100.0,
            item_id: album,
            item_title: String::new(),
            item_kind: "album".into(),
            has_lyrics: false,
            loudness: Default::default(),
            artist_key: Some(artist.to_owned()),
            liked: false,
        }
    }

    fn ids(tracks: &[Candidate]) -> Vec<i32> {
        tracks.iter().map(|t| t.id).collect()
    }

    #[test]
    fn dealing_goes_round_the_albums_and_stops_at_a_few_each() {
        let queues = vec![
            (1..=5).map(|id| track(id, 10, "a")).collect(),
            (6..=7).map(|id| track(id, 20, "b")).collect(),
        ];
        let dealt = deal(&anchor(40), queues, 40);
        assert_eq!(ids(&dealt), [1, 6, 2, 7, 3]);
    }

    #[test]
    fn dealing_keeps_to_the_share_until_the_list_would_stay_short() {
        // Share 2 at this limit: the third album of artist "a" waits for the second pass.
        let queues = vec![
            VecDeque::from([track(1, 10, "a")]),
            VecDeque::from([track(2, 20, "a")]),
            VecDeque::from([track(3, 30, "a")]),
            VecDeque::from([track(4, 40, "b")]),
        ];
        assert_eq!(ids(&deal(&anchor(4), queues.clone(), 4)), [1, 2, 4, 3]);
        assert_eq!(ids(&deal(&anchor(3), queues, 3)), [1, 2, 4]);
    }

    #[test]
    fn no_artist_plays_three_times_in_a_row_while_another_is_left() {
        let tracks = vec![
            track(1, 10, "a"),
            track(2, 11, "a"),
            track(3, 12, "a"),
            track(4, 13, "a"),
            track(5, 20, "b"),
            track(6, 21, "c"),
        ];
        assert_eq!(ids(&spread(tracks)), [1, 2, 5, 3, 4, 6]);
        // One artist only: nothing to put in between, nothing dropped.
        let alone = vec![track(1, 10, "a"), track(2, 11, "a"), track(3, 12, "a")];
        assert_eq!(ids(&spread(alone)), [1, 2, 3]);
    }

    #[test]
    fn tracks_without_any_artist_group_by_album() {
        let mut nameless = track(1, 10, "");
        nameless.artist_key = None;
        assert_eq!(nameless.group(), "#10");
    }

    #[test]
    fn chance_stays_in_range_and_moves() {
        let mut chance = Chance::seeded(0);
        let drawn: Vec<f64> = (0..100).map(|_| chance.draw()).collect();
        assert!(drawn.iter().all(|n| (0.0..1.0).contains(n)));
        assert!(drawn.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn the_middle_year_of_an_artist() {
        assert_eq!(median_year([1990, 1970, 1980].into_iter()), Some(1980));
        assert_eq!(median_year(std::iter::empty()), None);
    }
}
