//! Reading an audio file's tags and duration (legacy `scanner/tags.ts`, there with
//! music-metadata, here with lofty). Never fails: an unreadable file gives [`TrackTags::UNREADABLE`].

use std::fs::File;
use std::path::Path;
use std::sync::LazyLock;

use lofty::config::ParseOptions;
use lofty::file::FileType;
use lofty::id3::v2::{Frame, Id3v2Tag};
use lofty::mp4::{AtomData, AtomIdent};
use lofty::prelude::*;
use lofty::tag::Tag;
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    /// How the album title and its artist are to be sorted (TSOA/soal, TSO2/soaa or TSOP/soar).
    pub album_sort: Option<String>,
    pub artist_sort: Option<String>,
    /// Marked as a compilation (ID3 TCMP, MP4 cpil, Vorbis COMPILATION): a various-artists album.
    pub compilation: bool,
    /// Series name from the tags — MVNM/grouping, or a TXXX:SERIES frame.
    pub series: Option<String>,
    /// Volume number from MVIN or TXXX:SERIES-PART.
    pub series_index: Option<f64>,
    /// Blurb from the comment tag — where audiobook tools put the description.
    pub description: Option<String>,
    /// Seconds.
    pub duration: f64,
    pub readable: bool,
}

impl TrackTags {
    pub const UNREADABLE: TrackTags = TrackTags {
        title: None,
        artist: None,
        album: None,
        album_artist: None,
        track: None,
        disc: None,
        year: None,
        genre: None,
        album_sort: None,
        artist_sort: None,
        compilation: false,
        series: None,
        series_index: None,
        description: None,
        duration: 0.0,
        readable: false,
    };
}

/// Series live in different places depending on who wrote the file: ID3 movement frames
/// (MVNM/MVIN, what Plex and Audiobookshelf write), the grouping frame, or a free-form
/// TXXX:SERIES. All are read, first hit wins.
const SERIES_KEYS: &[&str] = &["series", "book series", "series-name", "mvnm", "show"];
const SERIES_PART_KEYS: &[&str] = &["series-part", "series_index", "part", "mvin", "movement"];
/// Blurbs hide in as many places: besides the standard comment frames, plenty of files carry the
/// text in a free-form `TXXX:comment` — ffmpeg writes exactly that for `-metadata comment=…`.
const DESCRIPTION_KEYS: &[&str] = &["comment", "comm", "description", "desc"];

fn non_empty(value: Option<impl AsRef<str>>) -> Option<String> {
    value
        .map(|v| v.as_ref().trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// "3", "3/12", "3.5" → the leading number.
fn to_number(value: Option<&str>) -> Option<f64> {
    static LEADING_NUMBER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^([0-9]{1,4}(?:[.,][0-9]{1,2})?)").unwrap());
    let caps = LEADING_NUMBER.captures(value?)?;
    caps[1]
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// The format's own key/value pairs, in file order, keys lower-cased and reduced to what follows
/// the last `:` (`TXXX:SERIES` and `----:com.apple.iTunes:SERIES` both become `series`). These
/// are the frames lofty's generic tag drops because it has no key for them.
fn native_pairs(path: &Path, file_type: FileType) -> Vec<(String, String)> {
    let read = || -> Option<Vec<(String, String)>> {
        let mut file = File::open(path).ok()?;
        let options = ParseOptions::new().read_properties(false);
        let mut pairs = Vec::new();
        let mut vorbis = |items: &mut dyn Iterator<Item = (&str, &str)>| {
            for (key, value) in items {
                pairs.push((key.to_lowercase(), value.to_owned()));
            }
        };
        match file_type {
            FileType::Mpeg => {
                let parsed = lofty::mpeg::MpegFile::read_from(&mut file, options).ok()?;
                return Some(id3v2_pairs(parsed.id3v2()?));
            }
            FileType::Flac => {
                let parsed = lofty::flac::FlacFile::read_from(&mut file, options).ok()?;
                vorbis(&mut parsed.vorbis_comments()?.items());
            }
            FileType::Vorbis => {
                let parsed = lofty::ogg::VorbisFile::read_from(&mut file, options).ok()?;
                vorbis(&mut parsed.vorbis_comments().items());
            }
            FileType::Opus => {
                let parsed = lofty::ogg::OpusFile::read_from(&mut file, options).ok()?;
                vorbis(&mut parsed.vorbis_comments().items());
            }
            FileType::Mp4 => {
                let parsed = lofty::mp4::Mp4File::read_from(&mut file, options).ok()?;
                for atom in parsed.ilst()? {
                    let key = match atom.ident() {
                        AtomIdent::Freeform { name, .. } => name.to_lowercase(),
                        AtomIdent::Fourcc(code) => String::from_utf8_lossy(code).to_lowercase(),
                    };
                    let text = atom.data().find_map(|data| match data {
                        AtomData::UTF8(s) | AtomData::UTF16(s) => Some(s.clone()),
                        _ => None,
                    });
                    if let Some(text) = text {
                        pairs.push((key, text));
                    }
                }
            }
            _ => {}
        }
        Some(pairs)
    };
    read().unwrap_or_default()
}

fn id3v2_pairs(tag: &Id3v2Tag) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for frame in tag {
        match frame {
            Frame::UserText(f) => pairs.push((f.description.to_lowercase(), f.content.to_string())),
            Frame::Comment(f) => pairs.push(("comm".into(), f.content.to_string())),
            Frame::Text(f) => pairs.push((frame.id().as_str().to_lowercase(), f.value.to_string())),
            _ => {}
        }
    }
    pairs
}

/// The first native value under one of `wanted`, in file order.
fn native_tag(pairs: &[(String, String)], wanted: &[&str]) -> Option<String> {
    pairs
        .iter()
        .filter(|(key, _)| {
            let key = key.rsplit(':').next().unwrap_or(key);
            wanted.contains(&key)
        })
        .find_map(|(_, value)| non_empty(Some(value)))
}

fn year(tag: &Tag) -> Option<i32> {
    if let Some(date) = tag.date() {
        return Some(i32::from(date.year));
    }
    let raw = tag.get_string(ItemKey::Year)?;
    raw.get(..4)?.parse().ok()
}

/// The genre as text. ID3 files may still carry the v1 number (`17`, `(17)`, `(17)Rock`) instead
/// of a name; those become the name from the standard list.
fn genre(tag: &Tag) -> Option<String> {
    genre_name(&non_empty(tag.genre())?)
}

fn genre_name(raw: &str) -> Option<String> {
    static NUMBERED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:\(([0-9]{1,3})\)(.*)|([0-9]{1,3}))$").unwrap());
    let Some(caps) = NUMBERED.captures(raw) else {
        return Some(raw.to_owned());
    };
    if let Some(rest) = caps
        .get(2)
        .map(|m| m.as_str().trim())
        .filter(|r| !r.is_empty())
    {
        return Some(rest.to_owned());
    }
    let number = caps.get(1).or_else(|| caps.get(3))?.as_str();
    number
        .parse::<usize>()
        .ok()
        .and_then(|n| lofty::id3::v1::GENRES.get(n))
        .map(|name| (*name).to_owned())
}

pub fn read_tags(path: &Path) -> TrackTags {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return TrackTags::UNREADABLE;
    };
    let duration = tagged.properties().duration().as_secs_f64();
    if duration <= 0.0 {
        return TrackTags::UNREADABLE;
    }
    let empty = Tag::new(tagged.primary_tag_type());
    let tag = tagged
        .primary_tag()
        .or_else(|| tagged.first_tag())
        .unwrap_or(&empty);
    let natives = native_pairs(path, tagged.file_type());

    let series = non_empty(tag.get_string(ItemKey::Movement))
        .or_else(|| native_tag(&natives, SERIES_KEYS))
        .or_else(|| non_empty(tag.get_string(ItemKey::ContentGroup)));
    let series_index = to_number(tag.get_string(ItemKey::MovementNumber))
        .or_else(|| to_number(native_tag(&natives, SERIES_PART_KEYS).as_deref()));
    let description = tag
        .get_strings(ItemKey::Comment)
        .find_map(|c| non_empty(Some(c)))
        .or_else(|| non_empty(tag.get_string(ItemKey::Description)))
        .or_else(|| native_tag(&natives, DESCRIPTION_KEYS));

    TrackTags {
        title: non_empty(tag.title()),
        artist: non_empty(tag.artist()),
        album: non_empty(tag.album()),
        album_artist: non_empty(tag.get_string(ItemKey::AlbumArtist)),
        track: tag.track(),
        disc: tag.disk(),
        year: year(tag),
        genre: genre(tag),
        album_sort: non_empty(tag.get_string(ItemKey::AlbumTitleSortOrder)),
        artist_sort: non_empty(tag.get_string(ItemKey::AlbumArtistSortOrder))
            .or_else(|| non_empty(tag.get_string(ItemKey::TrackArtistSortOrder))),
        compilation: tag
            .get_string(ItemKey::FlagCompilation)
            .is_some_and(|v| matches!(v.trim(), "1" | "true" | "True" | "TRUE")),
        series,
        series_index,
        description,
        duration,
        readable: true,
    }
}

/// The first embedded picture of a file, if any.
pub fn embedded_picture(path: &Path) -> Option<Vec<u8>> {
    let tagged = lofty::read_from_path(path).ok()?;
    tagged
        .tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .map(|picture| picture.data().to_vec())
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_number_reads_the_leading_number() {
        assert_eq!(to_number(Some("3")), Some(3.0));
        assert_eq!(to_number(Some("3/12")), Some(3.0));
        assert_eq!(to_number(Some("3,5")), Some(3.5));
        assert_eq!(to_number(Some("x3")), None);
        assert_eq!(to_number(None), None);
    }

    #[test]
    fn native_tag_matches_the_part_after_the_last_colon_in_file_order() {
        let pairs = vec![
            ("title".to_owned(), "x".to_owned()),
            ("com.apple.itunes:series".to_owned(), "  ".to_owned()),
            ("book series".to_owned(), "Perry Rhodan".to_owned()),
            ("series".to_owned(), "Später".to_owned()),
        ];
        assert_eq!(
            native_tag(&pairs, SERIES_KEYS).as_deref(),
            Some("Perry Rhodan")
        );
        assert_eq!(native_tag(&pairs, DESCRIPTION_KEYS), None);
    }

    #[test]
    fn genre_numbers_become_names() {
        assert_eq!(genre_name("17").as_deref(), Some("Rock"));
        assert_eq!(genre_name("(17)").as_deref(), Some("Rock"));
        assert_eq!(genre_name("(17)Hard Rock").as_deref(), Some("Hard Rock"));
        assert_eq!(genre_name("80s Pop").as_deref(), Some("80s Pop"));
        assert_eq!(genre_name("Jazz").as_deref(), Some("Jazz"));
        assert_eq!(genre_name("999"), None);
    }
}
