//! Wire format (legacy `api/serialize.ts`): rows in, snake_case API objects out, timestamps
//! through [`to_iso`]. Routes never hand-roll row → JSON mapping.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

use crate::library::queries::{BookEntry, Chapter, Item, Loudness, NewEpisode, Track, TrackHit};
use crate::library::top::{Review, TopAlbum, TopArtist, TopGenre, TopTrack};

/// ISO-8601 in UTC with whole seconds and a `Z`: `2026-10-14T18:22:57Z`. Legacy cut the
/// milliseconds off `toISOString()` because strict ISO-8601 clients (the iOS app) reject them.
pub fn to_iso(d: DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// `serde(serialize_with)` adapter for `Option<DateTime<Utc>>` fields.
pub fn iso_opt<S: serde::Serializer>(d: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match d {
        Some(d) => s.serialize_str(&to_iso(*d)),
        None => s.serialize_none(),
    }
}

/// `serde(serialize_with)` adapter for `DateTime<Utc>` fields.
pub fn iso<S: serde::Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_iso(*d))
}

/// A number the way `JSON.stringify` writes it: whole values without a fraction (`120`, not
/// `120.0`), so the wire format matches legacy byte for byte. NaN and infinities become `null`.
pub fn js_number<S: serde::Serializer>(n: &f64, s: S) -> Result<S::Ok, S::Error> {
    const MAX_SAFE: f64 = 9_007_199_254_740_991.0;
    if !n.is_finite() {
        s.serialize_none()
    } else if n.fract() == 0.0 && n.abs() <= MAX_SAFE {
        s.serialize_i64(*n as i64)
    } else {
        s.serialize_f64(*n)
    }
}

/// [`js_number`] for an optional number; `None` is `null`.
pub fn js_number_opt<S: serde::Serializer>(n: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match n {
        Some(n) => js_number(n, s),
        None => s.serialize_none(),
    }
}

/// [`js_number`] for every element of a list.
pub fn js_numbers<S: serde::Serializer>(numbers: &[f64], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    struct Js(f64);
    impl Serialize for Js {
        fn serialize<S2: serde::Serializer>(&self, s: S2) -> Result<S2::Ok, S2::Error> {
            js_number(&self.0, s)
        }
    }
    let mut out = s.serialize_seq(Some(numbers.len()))?;
    for n in numbers {
        out.serialize_element(&Js(*n))?;
    }
    out.end()
}

/// [`js_number`] for every value of a map.
pub fn js_number_map<K, S>(map: &std::collections::HashMap<K, f64>, s: S) -> Result<S::Ok, S::Error>
where
    K: Serialize,
    S: serde::Serializer,
{
    js_number_entries(map.len(), map.iter(), s)
}

/// [`js_number_map`] for an ordered map.
pub fn js_number_map_btree<K, S>(
    map: &std::collections::BTreeMap<K, f64>,
    s: S,
) -> Result<S::Ok, S::Error>
where
    K: Serialize,
    S: serde::Serializer,
{
    js_number_entries(map.len(), map.iter(), s)
}

fn js_number_entries<'a, K, S>(
    len: usize,
    entries: impl Iterator<Item = (&'a K, &'a f64)>,
    s: S,
) -> Result<S::Ok, S::Error>
where
    K: Serialize + 'a,
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    struct Js(f64);
    impl Serialize for Js {
        fn serialize<S2: serde::Serializer>(&self, s: S2) -> Result<S2::Ok, S2::Error> {
            js_number(&self.0, s)
        }
    }
    let mut out = s.serialize_map(Some(len))?;
    for (k, v) in entries {
        out.serialize_entry(k, &Js(*v))?;
    }
    out.end()
}

/// An item in a list. No `cover_path` (a server path), only whether there is one; no
/// description (blurbs run long, a list would carry megabytes of prose).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemSummary {
    pub id: i32,
    pub kind: String,
    pub title: String,
    pub has_cover: bool,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub series: Option<String>,
    /// The volume number within `series`, where the tags or a person named one. A list sorted
    /// by series is already in this order; the number is there to be shown.
    #[serde(serialize_with = "js_number_opt")]
    pub series_index: Option<f64>,
    pub year: Option<i32>,
    #[serde(serialize_with = "iso_opt")]
    pub missing_since: Option<DateTime<Utc>>,
    #[serde(serialize_with = "iso_opt")]
    pub published_at: Option<DateTime<Utc>>,
}

impl From<&Item> for ItemSummary {
    fn from(row: &Item) -> Self {
        ItemSummary {
            id: row.id,
            kind: row.kind.clone(),
            title: row.title.clone(),
            has_cover: row.cover_path.is_some(),
            author: row.author.clone(),
            artist: row.artist.clone(),
            series: row.series.clone(),
            series_index: row.series_index,
            year: row.year,
            missing_since: row.missing_since,
            published_at: row.published_at,
        }
    }
}

/// A book in a row of the "Entdecken" feed: everything an [`ItemSummary`] has, so a client reads
/// it with the model it already uses for `/items`, plus what a tile there wants to show.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BookSummary {
    #[serde(flatten)]
    pub summary: ItemSummary,
    pub narrator: Option<String>,
    #[serde(serialize_with = "js_number")]
    pub duration: f64,
    #[serde(serialize_with = "js_number")]
    pub position: f64,
}

impl From<&BookEntry> for BookSummary {
    fn from(row: &BookEntry) -> Self {
        BookSummary {
            summary: (&row.item).into(),
            narrator: row.item.narrator.clone(),
            duration: row.duration,
            position: row.position,
        }
    }
}

/// An unheard episode with its podcast, as `/podcasts/new` and the "Entdecken" feed list it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewEpisodeOut {
    pub id: i32,
    pub title: String,
    #[serde(serialize_with = "iso_opt")]
    pub published_at: Option<DateTime<Utc>>,
    pub podcast_id: i32,
    pub podcast_title: String,
    /// Its own artwork or its podcast's; `/items/{id}/cover` serves either.
    pub has_cover: bool,
    pub downloaded: bool,
    #[serde(serialize_with = "js_number")]
    pub duration: f64,
    #[serde(serialize_with = "js_number")]
    pub position: f64,
}

impl From<NewEpisode> for NewEpisodeOut {
    fn from(e: NewEpisode) -> Self {
        NewEpisodeOut {
            id: e.id,
            has_cover: e.cover_path.is_some() || e.podcast_has_cover,
            title: e.title,
            published_at: e.published_at,
            podcast_id: e.podcast_id,
            podcast_title: e.podcast_title,
            downloaded: e.downloaded,
            duration: e.duration,
            position: e.position,
        }
    }
}

/// A single item: the summary plus its description and genre (`genre` is new in Rust).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemDetail {
    #[serde(flatten)]
    pub summary: ItemSummary,
    pub description: Option<String>,
    pub genre: Option<String>,
}

impl From<&Item> for ItemDetail {
    fn from(row: &Item) -> Self {
        ItemDetail {
            summary: row.into(),
            description: row.description.clone(),
            genre: row.genre.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackOut {
    pub id: i32,
    pub position: i32,
    pub title: Option<String>,
    #[serde(serialize_with = "js_number")]
    pub duration: f64,
    /// New in Rust, so a client can head discs and name a sampler's artists without asking
    /// `/tracks/{id}` for every track.
    pub artist: Option<String>,
    pub disc: Option<i32>,
    /// Whether `GET /tracks/{id}/lyrics` has a text for it, so a client shows its lyrics button
    /// only where there is one. Kept by the scan.
    pub has_lyrics: bool,
    /// `gain_db`, `album_gain_db`, `peak` for levelling the volume; each `null` while unknown.
    #[serde(flatten)]
    pub loudness: Loudness,
}

impl From<&Track> for TrackOut {
    fn from(row: &Track) -> Self {
        TrackOut {
            id: row.id,
            position: row.position,
            title: row.title.clone(),
            duration: row.duration,
            artist: row.artist.clone(),
            disc: row.disc,
            has_lyrics: row.has_lyrics,
            loudness: row.loudness,
        }
    }
}

/// A single track (`GET /tracks/{id}`): the list shape (which carries the disc) plus its item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackDetail {
    #[serde(flatten)]
    pub track: TrackOut,
    pub item_id: i32,
}

impl From<&Track> for TrackDetail {
    fn from(row: &Track) -> Self {
        TrackDetail {
            track: row.into(),
            item_id: row.item_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChapterOut {
    pub title: String,
    #[serde(serialize_with = "js_number")]
    pub start: f64,
    #[serde(serialize_with = "js_number")]
    pub end: f64,
}

impl From<&Chapter> for ChapterOut {
    fn from(row: &Chapter) -> Self {
        ChapterOut {
            title: row.title.clone(),
            start: row.start,
            end: row.end,
        }
    }
}

/// A track in a cross-item list (favourites, search) with the item it belongs to
/// (`toTrackSummary`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackSummary {
    pub id: i32,
    pub title: Option<String>,
    /// The track's own artist (new in Rust).
    pub artist: Option<String>,
    #[serde(serialize_with = "js_number")]
    pub duration: f64,
    pub item_id: i32,
    pub item_title: String,
    pub item_kind: String,
    pub has_lyrics: bool,
    /// As in [`TrackOut`].
    #[serde(flatten)]
    pub loudness: Loudness,
}

impl TrackSummary {
    pub fn new(track: &Track, item: &Item) -> Self {
        TrackSummary {
            id: track.id,
            title: track.title.clone(),
            artist: track.artist.clone(),
            duration: track.duration,
            item_id: item.id,
            item_title: item.title.clone(),
            item_kind: item.kind.clone(),
            has_lyrics: track.has_lyrics,
            loudness: track.loudness,
        }
    }
}

impl From<&TrackHit> for TrackSummary {
    fn from(hit: &TrackHit) -> Self {
        TrackSummary {
            id: hit.id,
            title: hit.title.clone(),
            artist: hit.artist.clone(),
            duration: hit.duration,
            item_id: hit.item_id,
            item_title: hit.item_title.clone(),
            item_kind: hit.item_kind.clone(),
            has_lyrics: hit.has_lyrics,
            loudness: hit.loudness,
        }
    }
}

/// One entry of `GET /me/history`: a stretch of listening to one item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryEntry {
    /// When the latest part of it was heard.
    #[serde(serialize_with = "iso")]
    pub listened_at: DateTime<Utc>,
    #[serde(serialize_with = "js_number")]
    pub seconds: f64,
    pub item: ItemSummary,
}

/// One entry of `GET /me/top?kind=tracks`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopTrackOut {
    pub track: TrackSummary,
    #[serde(serialize_with = "js_number")]
    pub seconds: f64,
    /// Listens of at least 30 seconds.
    pub plays: i32,
}

impl From<&TopTrack> for TopTrackOut {
    fn from(top: &TopTrack) -> Self {
        TopTrackOut {
            track: (&top.track).into(),
            seconds: top.seconds,
            plays: top.plays,
        }
    }
}

/// One entry of `GET /me/top?kind=albums`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopAlbumOut {
    pub item: ItemSummary,
    #[serde(serialize_with = "js_number")]
    pub seconds: f64,
    pub plays: i32,
}

impl From<&TopAlbum> for TopAlbumOut {
    fn from(top: &TopAlbum) -> Self {
        TopAlbumOut {
            item: (&top.item).into(),
            seconds: top.seconds,
            plays: top.plays,
        }
    }
}

/// One entry of `GET /me/top?kind=artists`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopArtistOut {
    pub artist: String,
    #[serde(serialize_with = "js_number")]
    pub seconds: f64,
    pub plays: i32,
    /// Whether `GET /artists/{artist}/image` answers with a picture.
    pub has_image: bool,
}

impl From<&TopArtist> for TopArtistOut {
    fn from(top: &TopArtist) -> Self {
        TopArtistOut {
            artist: top.artist.clone(),
            seconds: top.seconds,
            plays: top.plays,
            has_image: top.has_image,
        }
    }
}

/// One entry of `GET /me/top?kind=genres`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopGenreOut {
    pub genre: String,
    #[serde(serialize_with = "js_number")]
    pub seconds: f64,
}

impl From<&TopGenre> for TopGenreOut {
    fn from(top: &TopGenre) -> Self {
        TopGenreOut {
            genre: top.genre.clone(),
            seconds: top.seconds,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewSeconds {
    #[serde(serialize_with = "js_number")]
    pub total: f64,
    #[serde(serialize_with = "js_number")]
    pub music: f64,
    #[serde(serialize_with = "js_number")]
    pub books: f64,
    #[serde(serialize_with = "js_number")]
    pub podcasts: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewDay {
    /// `YYYY-MM-DD`, a UTC day.
    pub date: String,
    #[serde(serialize_with = "js_number")]
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewFirstListen {
    #[serde(serialize_with = "iso")]
    pub at: DateTime<Utc>,
    pub item: ItemSummary,
}

/// `GET /me/review`: a calendar year of listening.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewOut {
    pub year: i32,
    pub seconds: ReviewSeconds,
    pub days_listened: i32,
    pub longest_day: Option<ReviewDay>,
    pub longest_streak_days: i32,
    /// Twelve numbers, January first.
    #[serde(serialize_with = "js_numbers")]
    pub months: Vec<f64>,
    pub top_tracks: Vec<TopTrackOut>,
    pub top_albums: Vec<TopAlbumOut>,
    pub top_artists: Vec<TopArtistOut>,
    pub top_genres: Vec<TopGenreOut>,
    pub books_finished: Vec<ItemSummary>,
    pub new_artists: i32,
    pub first_listen: Option<ReviewFirstListen>,
}

impl From<&Review> for ReviewOut {
    fn from(review: &Review) -> Self {
        let totals = &review.totals;
        ReviewOut {
            year: review.year,
            seconds: ReviewSeconds {
                total: totals.total,
                music: totals.music,
                books: totals.books,
                podcasts: totals.podcasts,
            },
            days_listened: totals.days_listened,
            longest_day: totals.longest_day.map(|(day, seconds)| ReviewDay {
                date: day.format("%Y-%m-%d").to_string(),
                seconds,
            }),
            longest_streak_days: totals.longest_streak_days,
            months: totals.months.to_vec(),
            top_tracks: review.top_tracks.iter().map(TopTrackOut::from).collect(),
            top_albums: review.top_albums.iter().map(TopAlbumOut::from).collect(),
            top_artists: review.top_artists.iter().map(TopArtistOut::from).collect(),
            top_genres: review.top_genres.iter().map(TopGenreOut::from).collect(),
            books_finished: review
                .books_finished
                .iter()
                .map(ItemSummary::from)
                .collect(),
            new_artists: review.new_artists,
            first_listen: review
                .first_listen
                .as_ref()
                .map(|(at, item)| ReviewFirstListen {
                    at: *at,
                    item: item.into(),
                }),
        }
    }
}

/// A radio station of the user's list (`/radio/stations`). The stream's address stays on the
/// server: clients play `/radio/stations/{id}/stream`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StationOut {
    pub id: i32,
    pub name: String,
    pub homepage: Option<String>,
    /// ISO 3166-1 alpha-2.
    pub country: Option<String>,
    pub tags: Vec<String>,
    pub codec: Option<String>,
    /// kbit/s.
    pub bitrate: Option<i32>,
    pub has_logo: bool,
}

impl From<crate::stations::store::Station> for StationOut {
    fn from(station: crate::stations::store::Station) -> Self {
        StationOut {
            id: station.id,
            name: station.name,
            homepage: station.homepage,
            country: station.country,
            tags: station.tags,
            codec: station.codec,
            bitrate: station.bitrate,
            has_logo: station.logo_file.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn drops_fractional_seconds() {
        let d = Utc.timestamp_millis_opt(1_792_002_177_123).unwrap();
        assert_eq!(to_iso(d), "2026-10-14T18:22:57Z");
    }

    #[test]
    fn numbers_serialize_like_json_stringify() {
        #[derive(Serialize)]
        struct N(#[serde(serialize_with = "js_number")] f64);
        let j = |n: f64| serde_json::to_string(&N(n)).unwrap();
        assert_eq!(j(120.0), "120");
        assert_eq!(j(0.0), "0");
        assert_eq!(j(-0.0), "0");
        assert_eq!(j(1.25), "1.25");
        assert_eq!(j(1350.5), "1350.5");
        assert_eq!(j(f64::NAN), "null");
    }

    #[test]
    fn whole_seconds_stay_whole() {
        let d = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(to_iso(d), "2026-01-02T03:04:05Z");
    }
}
