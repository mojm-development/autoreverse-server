//! Lyrics out of the user's own collection: a `.lrc` or `.txt` beside the audio file, else what
//! is embedded in the file's tags. Nothing is fetched from anywhere and nothing is stored — the
//! text is read when somebody asks for it; only the flag `tracks.has_lyrics` is kept, by the scan
//! (`scanner::lyrics`).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use lofty::config::ParseOptions;
use lofty::file::FileType;
use lofty::id3::v2::{Frame, SynchronizedTextFrame, TimestampFormat};
use lofty::prelude::*;
use regex::Regex;
use serde::Serialize;

use crate::serialize::js_number_opt;

/// More than this is not a song's text but some other file that happens to share the name (a
/// log, a cue sheet, a book's blurb). Applies to a file beside the track and to embedded text.
pub const MAX_LYRICS_BYTES: u64 = 200 * 1024;

/// The endings a file beside the track may have, in the order they are tried: timed text first.
const SIDECAR_EXTENSIONS: [&str; 4] = ["lrc", "LRC", "txt", "TXT"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LyricLine {
    /// Seconds from the start of the track; `null` in text without timing.
    #[serde(serialize_with = "js_number_opt")]
    pub t: Option<f64>,
    pub text: String,
}

/// Where a text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    File,
    Tags,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lyrics {
    pub synced: bool,
    pub lines: Vec<LyricLine>,
    pub source: Source,
}

/// Text and timing without the source, as the parsers hand it on.
#[derive(Debug, Clone, PartialEq)]
struct Parsed {
    synced: bool,
    lines: Vec<LyricLine>,
}

impl Parsed {
    fn from(self, source: Source) -> Lyrics {
        Lyrics {
            synced: self.synced,
            lines: self.lines,
            source,
        }
    }
}

/// Bytes of a text file as text: UTF-8 or UTF-16 by their byte order mark, else UTF-8 if it is
/// valid, else Latin-1 — which is what an old `.lrc` from a Windows machine is, and reading it
/// that way never fails.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
        // An odd byte at the end is half a character and dropped.
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| unit(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// `[mm:ss]`, `[mm:ss.x]`…`[mm:ss.xxx]`, also with `:` or `,` before the fraction, as some of
/// the many tools that write LRC do. No hours: `[00:01:25]` is 1.25 s in those files, and a
/// track of more than an hour simply counts its minutes past 59.
static STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[(\d{1,3}):(\d{1,2})(?:[.:,](\d{1,3}))?\]").unwrap());
/// A header line: `[ar:Artist]`, `[ti:…]`, `[length:…]`, `[offset:+250]`.
static HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([A-Za-z#][A-Za-z_]*):([^\]]*)\]\s*$").unwrap());
/// Word timing of "enhanced" LRC inside a line: `<00:12.34>`.
static WORD_STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<\d{1,3}:\d{1,2}(?:[.:,]\d{1,3})?>").unwrap());

fn stamp_seconds(caps: &regex::Captures<'_>) -> f64 {
    let number = |i: usize| {
        caps.get(i)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    // ".5" is half a second, ".05" a twentieth: the digits are a decimal fraction.
    let fraction = caps
        .get(3)
        .and_then(|m| format!("0.{}", m.as_str()).parse::<f64>().ok())
        .unwrap_or(0.0);
    number(1) * 60.0 + number(2) + fraction
}

/// Text as LRC, or `None` if not one line of it carries a time stamp. A line with several stamps
/// (a chorus) becomes one line per stamp; header lines are dropped; `[offset:±ms]` shifts every
/// line (positive: earlier, as the format defines it); the result is in playing order.
fn parse_lrc(text: &str) -> Option<Vec<LyricLine>> {
    let mut offset = 0.0;
    let mut lines: Vec<LyricLine> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim();
        if let Some(header) = HEADER.captures(raw) {
            if header[1].eq_ignore_ascii_case("offset")
                && let Ok(ms) = header[2].trim().parse::<f64>()
                && ms.is_finite()
            {
                offset = ms / 1000.0;
            }
            continue;
        }
        let mut rest = raw;
        let mut stamps = Vec::new();
        while let Some(caps) = STAMP.captures(rest) {
            stamps.push(stamp_seconds(&caps));
            rest = rest[caps[0].len()..].trim_start();
        }
        let words = WORD_STAMP.replace_all(rest, "");
        let words = words.trim();
        if stamps.is_empty() {
            // Text without a stamp inside a timed file continues the line before it; before the
            // first stamp it is a title or a credit and nothing that is sung.
            if let (false, Some(previous)) = (words.is_empty(), lines.last()) {
                lines.push(LyricLine {
                    t: previous.t,
                    text: words.to_owned(),
                });
            }
            continue;
        }
        for t in stamps {
            lines.push(LyricLine {
                t: Some(t),
                text: words.to_owned(),
            });
        }
    }
    if lines.is_empty() {
        return None;
    }
    for line in &mut lines {
        line.t = line.t.map(|t| (t - offset).max(0.0));
    }
    // Stable: lines that share a moment keep the order they were written in.
    lines.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    Some(lines)
}

/// Plain text, a line per entry. Blank lines inside stay — they are the gaps between verses —
/// the ones around the text go.
fn parse_plain(text: &str) -> Vec<LyricLine> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let Some(first) = lines.iter().position(|l| !l.trim().is_empty()) else {
        return Vec::new();
    };
    let last = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .unwrap_or(first);
    lines[first..=last]
        .iter()
        .map(|line| LyricLine {
            t: None,
            text: line.trim().to_owned(),
        })
        .collect()
}

/// Any text: timed if it carries LRC stamps, plain otherwise. `None` when it says nothing.
fn parse_text(text: &str) -> Option<Parsed> {
    if text.len() as u64 > MAX_LYRICS_BYTES {
        return None;
    }
    // A lone `\r` ends a line in files from old Macs and in some tags.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if let Some(lines) = parse_lrc(&text) {
        // Only stamps and no word at all is an empty template, not a text.
        return lines.iter().any(|l| !l.text.is_empty()).then_some(Parsed {
            synced: true,
            lines,
        });
    }
    let lines = parse_plain(&text);
    (!lines.is_empty()).then_some(Parsed {
        synced: false,
        lines,
    })
}

/// The files that may hold `audio`'s lyrics: same folder, same name, another ending. Built from
/// the track's own path and nothing a request says, so it cannot name any other place.
fn sidecar_candidates(audio: &Path) -> Vec<PathBuf> {
    SIDECAR_EXTENSIONS
        .iter()
        .map(|extension| audio.with_extension(extension))
        .filter(|candidate| candidate != audio)
        .collect()
}

/// A lyrics file beside `audio`, read and parsed. A candidate that is a link out of the track's
/// folder is passed over: the file beside the track is all this may ever read.
fn from_sidecar(audio: &Path) -> Option<Parsed> {
    let folder = audio.parent()?.canonicalize().ok()?;
    sidecar_candidates(audio).into_iter().find_map(|candidate| {
        let real = candidate.canonicalize().ok()?;
        if real.parent()? != folder {
            return None;
        }
        let meta = std::fs::metadata(&real).ok()?;
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_LYRICS_BYTES {
            return None;
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        // `take`: the file may have grown since the size was read.
        File::open(&real)
            .ok()?
            .take(MAX_LYRICS_BYTES)
            .read_to_end(&mut bytes)
            .ok()?;
        parse_text(&decode(&bytes))
    })
}

/// An ID3v2 SYLT frame's lines. Only millisecond stamps can be placed; a frame that counts in
/// MPEG frames still gives its words, as text without timing.
fn from_sylt(frame: &SynchronizedTextFrame<'_>) -> Option<Parsed> {
    let synced = frame.timestamp_format == TimestampFormat::MS;
    let mut lines: Vec<LyricLine> = frame
        .content
        .iter()
        .map(|(at, text)| LyricLine {
            t: synced.then_some(f64::from(*at) / 1000.0),
            // Writers put the line break in front of each line rather than behind it.
            text: text.trim().to_owned(),
        })
        .collect();
    if synced {
        lines.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    }
    let size: usize = lines.iter().map(|l| l.text.len()).sum();
    (size as u64 <= MAX_LYRICS_BYTES && lines.iter().any(|l| !l.text.is_empty()))
        .then_some(Parsed { synced, lines })
}

/// The descriptions under which a free-form ID3v2 frame (TXXX) holds lyrics: what taggers
/// write that follow the Vorbis names, and ffmpeg's `lyrics-<language>`.
fn is_lyrics_description(description: &str) -> bool {
    let description = description.to_lowercase();
    matches!(
        description.as_str(),
        "lyrics" | "unsyncedlyrics" | "syncedlyrics"
    ) || description.starts_with("lyrics-")
}

/// Lyrics that only the format's own tag knows: ID3v2 SYLT, which lofty keeps as a binary frame,
/// free-form ID3v2 frames, and the Vorbis comment `SYNCEDLYRICS`, which lofty's generic tag
/// has no key for. In file order.
fn native_lyrics(path: &Path, file_type: FileType) -> Vec<Parsed> {
    let read = || -> Option<Vec<Parsed>> {
        let mut file = File::open(path).ok()?;
        let options = ParseOptions::new().read_properties(false);
        let vorbis = |comments: &lofty::ogg::tag::VorbisComments| {
            comments
                .get_all("SYNCEDLYRICS")
                .filter_map(parse_text)
                .collect()
        };
        Some(match file_type {
            FileType::Mpeg => {
                let parsed = lofty::mpeg::MpegFile::read_from(&mut file, options).ok()?;
                parsed
                    .id3v2()?
                    .into_iter()
                    .filter_map(|frame| match frame {
                        Frame::Binary(binary) if frame.id().as_str() == "SYLT" => {
                            let sylt =
                                SynchronizedTextFrame::parse(&binary.data, frame.flags()).ok()?;
                            from_sylt(&sylt)
                        }
                        Frame::UserText(text) if is_lyrics_description(&text.description) => {
                            parse_text(&text.content)
                        }
                        _ => None,
                    })
                    .collect()
            }
            FileType::Flac => {
                let parsed = lofty::flac::FlacFile::read_from(&mut file, options).ok()?;
                vorbis(parsed.vorbis_comments()?)
            }
            FileType::Vorbis => {
                let parsed = lofty::ogg::VorbisFile::read_from(&mut file, options).ok()?;
                vorbis(parsed.vorbis_comments())
            }
            FileType::Opus => {
                let parsed = lofty::ogg::OpusFile::read_from(&mut file, options).ok()?;
                vorbis(parsed.vorbis_comments())
            }
            _ => Vec::new(),
        })
    };
    read().unwrap_or_default()
}

/// What the file's tags hold: ID3v2 USLT and SYLT, the Vorbis comments `LYRICS`,
/// `UNSYNCEDLYRICS` and `SYNCEDLYRICS`, MP4 `©lyr`. Timed text wins over plain text; a plain
/// field whose text is LRC counts as timed.
fn from_tags(path: &Path) -> Option<Parsed> {
    let tagged = lofty::probe::Probe::open(path)
        .ok()?
        .options(ParseOptions::new().read_properties(false))
        .read()
        .ok()?;
    // lofty maps USLT, ©lyr, LYRICS and UNSYNCEDLYRICS onto these two keys, whatever the
    // container, and decodes UTF-16 and Latin-1 frames on the way.
    let mut found: Vec<Parsed> = tagged
        .tags()
        .iter()
        .flat_map(|tag| {
            tag.get_strings(ItemKey::Lyrics)
                .chain(tag.get_strings(ItemKey::UnsyncLyrics))
        })
        .filter_map(parse_text)
        .collect();
    if !found.iter().any(|parsed| parsed.synced) {
        found.extend(native_lyrics(path, tagged.file_type()));
    }
    let timed = found.iter().position(|parsed| parsed.synced).unwrap_or(0);
    (!found.is_empty()).then(|| found.swap_remove(timed))
}

/// The lyrics of the audio file at `path`: the file beside it, else its tags. Blocking — call
/// it from `spawn_blocking`.
pub fn read(path: &Path) -> Option<Lyrics> {
    from_sidecar(path)
        .map(|parsed| parsed.from(Source::File))
        .or_else(|| from_tags(path).map(|parsed| parsed.from(Source::Tags)))
}

/// Whether a file beside `path` holds lyrics (the scan's check on every run).
pub fn has_sidecar(path: &Path) -> bool {
    from_sidecar(path).is_some()
}

/// Whether the file's own tags hold lyrics (the scan's check once per version of a file).
pub fn has_embedded(path: &Path) -> bool {
    from_tags(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: Option<f64>, text: &str) -> LyricLine {
        LyricLine {
            t,
            text: text.into(),
        }
    }

    #[test]
    fn lrc_lines_come_out_in_playing_order() {
        let text = "[ar:Die Band]\n[ti:Lied]\n[00:12.50]Zweite Zeile\n[00:05]Erste Zeile\n\
                    [01:02.345]Dritte\n";
        let parsed = parse_text(text).unwrap();
        assert!(parsed.synced);
        assert_eq!(
            parsed.lines,
            [
                line(Some(5.0), "Erste Zeile"),
                line(Some(12.5), "Zweite Zeile"),
                line(Some(62.345), "Dritte"),
            ]
        );
    }

    #[test]
    fn a_line_with_several_stamps_is_one_line_per_stamp() {
        let parsed = parse_text("[00:10.00][00:40.00]Refrain\n[00:20.00]Strophe").unwrap();
        assert_eq!(
            parsed.lines,
            [
                line(Some(10.0), "Refrain"),
                line(Some(20.0), "Strophe"),
                line(Some(40.0), "Refrain"),
            ]
        );
    }

    #[test]
    fn the_offset_shifts_every_line_and_never_below_zero() {
        let parsed = parse_text("[offset:+500]\n[00:00.20]A\n[00:10.00]B").unwrap();
        assert_eq!(parsed.lines, [line(Some(0.0), "A"), line(Some(9.5), "B")]);
        let parsed = parse_text("[00:10.00]B\n[offset: -1500]").unwrap();
        assert_eq!(parsed.lines, [line(Some(11.5), "B")]);
    }

    #[test]
    fn empty_timed_lines_stay_and_word_stamps_go() {
        let parsed = parse_text("[00:01.00]<00:01.00>Eins <00:01.50>zwei\n[00:05.00]\n").unwrap();
        assert_eq!(
            parsed.lines,
            [line(Some(1.0), "Eins zwei"), line(Some(5.0), "")]
        );
    }

    #[test]
    fn fractions_are_decimal_and_minutes_run_past_an_hour() {
        let parsed = parse_text("[00:01.5]a\n[00:01:25]b\n[60:02]c").unwrap();
        assert_eq!(
            parsed.lines,
            [
                line(Some(1.25), "b"),
                line(Some(1.5), "a"),
                line(Some(3602.0), "c"),
            ]
        );
    }

    #[test]
    fn text_without_a_stamp_follows_the_line_before_it() {
        let parsed = parse_text("Vorspann\n[00:03.00]Zeile\n(leise)\n").unwrap();
        assert_eq!(
            parsed.lines,
            [line(Some(3.0), "Zeile"), line(Some(3.0), "(leise)")]
        );
    }

    #[test]
    fn plain_text_keeps_the_gaps_between_verses() {
        let parsed = parse_text("\n\nErste\r\nZweite  \r\n\r\nNeue Strophe\n\n").unwrap();
        assert!(!parsed.synced);
        assert_eq!(
            parsed.lines,
            [
                line(None, "Erste"),
                line(None, "Zweite"),
                line(None, ""),
                line(None, "Neue Strophe"),
            ]
        );
    }

    #[test]
    fn nothing_to_read_is_no_lyrics() {
        assert_eq!(parse_text(""), None);
        assert_eq!(parse_text(" \n\t\n"), None);
        assert_eq!(parse_text("[00:01.00]\n[00:02.00]\n"), None);
        let huge = "la ".repeat(MAX_LYRICS_BYTES as usize);
        assert_eq!(parse_text(&huge), None);
    }

    #[test]
    fn a_bracket_that_is_no_stamp_is_text() {
        let parsed = parse_text("[Refrain]\nLa la\n").unwrap();
        assert!(!parsed.synced);
        assert_eq!(parsed.lines[0], line(None, "[Refrain]"));
    }

    #[test]
    fn decodes_what_text_files_come_in() {
        assert_eq!(decode("Grüße".as_bytes()), "Grüße");
        assert_eq!(decode(b"\xEF\xBB\xBFGr\xC3\xBC\xC3\x9Fe"), "Grüße");
        assert_eq!(decode(b"Gr\xFC\xDFe"), "Grüße");
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Grüße".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode(&le), "Grüße");
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("Grüße".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode(&be), "Grüße");
    }

    #[test]
    fn the_file_beside_the_track_wins_and_lrc_goes_before_txt() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("01 Lied.mp3");
        std::fs::write(&audio, b"no audio").unwrap();
        assert_eq!(read(&audio), None);
        std::fs::write(dir.path().join("01 Lied.txt"), "Nur Text").unwrap();
        let plain = read(&audio).unwrap();
        assert_eq!(plain.source, Source::File);
        assert!(!plain.synced);
        std::fs::write(dir.path().join("01 Lied.lrc"), "[00:01.00]Mit Zeit").unwrap();
        let timed = read(&audio).unwrap();
        assert!(timed.synced);
        assert_eq!(timed.lines, [line(Some(1.0), "Mit Zeit")]);
        assert!(has_sidecar(&audio));
        assert!(!has_embedded(&audio));
    }

    #[test]
    fn a_text_file_that_is_too_large_or_empty_is_not_lyrics() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("a.flac");
        std::fs::write(&audio, b"x").unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        assert_eq!(read(&audio), None);
        std::fs::write(
            dir.path().join("a.txt"),
            "x\n".repeat(MAX_LYRICS_BYTES as usize),
        )
        .unwrap();
        assert_eq!(read(&audio), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_is_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("geheim.txt");
        std::fs::write(&secret, "nicht für fremde Augen").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("a.mp3");
        std::fs::write(&audio, b"x").unwrap();
        std::os::unix::fs::symlink(&secret, dir.path().join("a.txt")).unwrap();
        assert_eq!(read(&audio), None);
        // A link that stays inside the folder is an ordinary way to share one text.
        std::fs::write(dir.path().join("text.txt"), "Gemeinsam").unwrap();
        std::fs::remove_file(dir.path().join("a.txt")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("text.txt"), dir.path().join("a.txt")).unwrap();
        assert_eq!(read(&audio).unwrap().lines, [line(None, "Gemeinsam")]);
    }
}
