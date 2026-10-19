//! Wire format (legacy `api/serialize.ts`): rows in, snake_case API objects out, timestamps
//! through [`to_iso`]. Routes never hand-roll row → JSON mapping.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

use crate::library::queries::{Chapter, Item, Track, TrackHit};

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
            year: row.year,
            missing_since: row.missing_since,
            published_at: row.published_at,
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
