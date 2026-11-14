//! Playlists as files: writing Extended M3U and reading it back in (`GET /playlists/{id}/export`,
//! `POST /playlists/import`).
//!
//! Reading is the hard direction. A file written elsewhere names paths of another machine
//! (`/Volumes/Musik/…`, `C:\Musik\…`, `file://` addresses) and titles as another tagger spelled
//! them, so every line is matched in two steps: by its path — the longest run of folders and
//! file name it shares with a track here, counted from the end — and only then by what its
//! `#EXTINF` says, artist and title, with the length as the tie-breaker. A wrong song in a
//! playlist is worse than a missing one, so a match on the bare file name alone has to be
//! unambiguous, and backed by the `#EXTINF` where there is one.
//!
//! The collection is read once per import ([`TrackIndex::load`]) and every line is answered from
//! memory: one statement however long the file is.

use std::collections::HashMap;

use sqlx::PgPool;

use super::playlists::ExportTrack;
use crate::ai::musicbrainz;

/// The longest text an import takes.
pub const MAX_IMPORT_BYTES: usize = 2 * 1024 * 1024;

/// How far a track's length may be off the `#EXTINF`'s and still be the same recording.
const DURATION_TOLERANCE: f64 = 3.0;

/// One line of a playlist file that names a track.
#[derive(Debug, Clone, PartialEq)]
pub struct M3uEntry {
    /// The path or address as written; `None` for an `#EXTINF` nothing followed.
    pub path: Option<String>,
    /// What the `#EXTINF` displays, usually "Artist - Title".
    pub display: Option<String>,
    /// Seconds from the `#EXTINF`, where it claims a real length (`-1` and `0` mean "unknown").
    pub duration: Option<f64>,
}

impl M3uEntry {
    /// How the entry reads to a person, for the list of what was not found.
    pub fn label(&self) -> String {
        self.display
            .clone()
            .or_else(|| self.path.clone())
            .unwrap_or_default()
    }
}

/// The entries of an M3U or M3U8 text, in order. Plain M3U (paths only) and Extended M3U both
/// read; directives other than `#EXTINF` are skipped.
pub fn parse(content: &str) -> Vec<M3uEntry> {
    let mut entries = Vec::new();
    let mut pending: Option<(Option<String>, Option<f64>)> = None;
    for line in content.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = strip_prefix_ignore_case(line, "#EXTINF:") {
            // Two in a row: the first one named a track without saying where it is.
            if let Some((display, duration)) = pending.take()
                && display.is_some()
            {
                entries.push(M3uEntry {
                    path: None,
                    display,
                    duration,
                });
            }
            pending = Some(parse_extinf(info));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let (display, duration) = pending.take().unwrap_or((None, None));
        entries.push(M3uEntry {
            path: Some(line.to_owned()),
            display,
            duration,
        });
    }
    if let Some((display, duration)) = pending
        && display.is_some()
    {
        entries.push(M3uEntry {
            path: None,
            display,
            duration,
        });
    }
    entries
}

fn strip_prefix_ignore_case<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let head = line.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &line[prefix.len()..])
}

/// `<seconds>[ attributes],<display>` → the display and the length.
fn parse_extinf(info: &str) -> (Option<String>, Option<f64>) {
    let (head, display) = info.split_once(',').unwrap_or((info, ""));
    let duration = head
        .split_whitespace()
        .next()
        .and_then(|raw| raw.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0);
    let display = display.trim();
    ((!display.is_empty()).then(|| display.to_owned()), duration)
}

/// Lower case, and the accents taken off: "Björk", "BJORK" and a decomposed "Bjo\u{308}rk"
/// (as macOS writes file names) all become `bjork`.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            // Combining marks: what is left of an accent once the letter is decomposed.
            '\u{0300}'..='\u{036f}' => {}
            'à'..='å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => out.push('c'),
            'ď' | 'đ' | 'ð' => out.push('d'),
            'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => out.push('e'),
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
            'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => out.push('i'),
            'ł' | 'ĺ' | 'ļ' | 'ľ' => out.push('l'),
            'ñ' | 'ń' | 'ņ' | 'ň' => out.push('n'),
            'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => out.push('o'),
            'ŕ' | 'ŗ' | 'ř' => out.push('r'),
            'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => out.push('s'),
            'ţ' | 'ť' | 'ț' => out.push('t'),
            'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            other => out.push(other),
        }
    }
    out
}

/// The key two spellings of a title or an artist share: [`fold`]ed, then what
/// `musicbrainz::normalize` does to an album title — letters and digits only, a trailing
/// "(Remastered 2011)" or "[Live]" cut off.
pub fn match_key(text: &str) -> String {
    musicbrainz::normalize(&fold(text))
}

/// `%41` → `A`; anything that is no valid escape, or no UTF-8 afterwards, stays as written.
fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escape = (bytes[at] == b'%')
            .then(|| text.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escape {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8(out)
        .ok()
        .filter(|decoded| decoded != text)
}

/// The folded parts of a path, whatever machine wrote it: `file://` and a drive letter dropped,
/// backslashes read as separators. Empty for something that is no path (a web address).
fn components(path: &str) -> Vec<String> {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Vec::new();
    }
    let path = path.replace('\\', "/");
    path.split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        // "C:" — and "file:" and "localhost", which cost nothing to keep: they are in front,
        // and the comparison runs from the end.
        .map(fold)
        .collect()
}

/// The ways a written path may be meant, most literal first: as it stands, and with its URL
/// escapes resolved (always for a `file://` address, else only as a second guess — a file may
/// really be called `100%25.mp3`).
fn path_readings(path: &str) -> Vec<Vec<String>> {
    let is_url = path
        .get(..7)
        .is_some_and(|head| head.eq_ignore_ascii_case("file://"));
    let literal = if is_url { &path[7..] } else { path };
    let decoded = percent_decode(literal);
    let mut readings = Vec::new();
    if !is_url || decoded.is_none() {
        readings.push(components(literal));
    }
    if let Some(decoded) = decoded {
        readings.push(components(&decoded));
    }
    readings.retain(|parts| !parts.is_empty());
    readings
}

/// How many parts two paths share, counted from the file name backwards.
fn common_suffix(a: &[String], b: &[String]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// A track of the collection, as far as matching needs it.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct LibraryTrack {
    pub id: i32,
    pub path: String,
    pub title: Option<String>,
    /// The track's own artist, where its tags name one.
    pub artist: Option<String>,
    /// The album's artist, or a book's author.
    pub item_artist: Option<String>,
    pub duration: f64,
}

impl LibraryTrack {
    fn by(&self, artist_key: &str) -> bool {
        [&self.artist, &self.item_artist]
            .into_iter()
            .flatten()
            .any(|name| match_key(name) == artist_key)
    }

    fn lasts(&self, duration: Option<f64>) -> bool {
        duration.is_some_and(|seconds| (self.duration - seconds).abs() <= DURATION_TOLERANCE)
    }
}

/// The collection, ready to be asked for a line at a time.
pub struct TrackIndex {
    tracks: Vec<LibraryTrack>,
    /// Folded file name → positions in `tracks`.
    by_file: HashMap<String, Vec<usize>>,
    /// [`match_key`] of the title → positions in `tracks`.
    by_title: HashMap<String, Vec<usize>>,
}

impl TrackIndex {
    /// Every track of a present item, originals before their duplicates, so that "the first of
    /// several equal ones" is the album the library shows.
    pub async fn load(db: &PgPool) -> Result<TrackIndex, sqlx::Error> {
        let tracks: Vec<LibraryTrack> = sqlx::query_as(
            "SELECT track.id, track.path, track.title, track.artist,
                    coalesce(item.artist, item.album_artist, item.author) AS item_artist,
                    track.duration
             FROM tracks AS track
             JOIN items AS item ON item.id = track.item_id
             WHERE item.missing_since IS NULL
             ORDER BY (item.duplicate_of IS NOT NULL), track.item_id, track.position",
        )
        .fetch_all(db)
        .await?;
        Ok(TrackIndex::new(tracks))
    }

    pub fn new(tracks: Vec<LibraryTrack>) -> TrackIndex {
        let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
        for (at, track) in tracks.iter().enumerate() {
            if let Some(name) = track.path.rsplit('/').next() {
                by_file.entry(fold(name)).or_default().push(at);
            }
            if let Some(key) = track.title.as_deref().map(match_key)
                && !key.is_empty()
            {
                by_title.entry(key).or_default().push(at);
            }
        }
        TrackIndex {
            tracks,
            by_file,
            by_title,
        }
    }

    /// The track an entry means, or `None`.
    pub fn find(&self, entry: &M3uEntry) -> Option<i32> {
        let named = entry.display.as_deref().map(named_like);
        entry
            .path
            .as_deref()
            .and_then(|path| self.by_path(path, named.as_deref(), entry.duration))
            .or_else(|| self.by_name(named.as_deref()?, entry.duration))
            .map(|at| self.tracks[at].id)
    }

    /// Whether what the `#EXTINF` says fits this track: its title — and its artist, where the
    /// reading names one — or failing that its length.
    fn fits(&self, at: usize, named: Option<&[Named]>, duration: Option<f64>) -> bool {
        let track = &self.tracks[at];
        let title = track.title.as_deref().map(match_key);
        let called = named.is_some_and(|readings| {
            readings.iter().any(|reading| {
                Some(&reading.title) == title.as_ref()
                    && reading.artist.as_deref().is_none_or(|a| track.by(a))
            })
        });
        called || track.lasts(duration)
    }

    fn by_path(&self, path: &str, named: Option<&[Named]>, duration: Option<f64>) -> Option<usize> {
        for wanted in path_readings(path) {
            let Some(candidates) = wanted.last().and_then(|file| self.by_file.get(file)) else {
                continue;
            };
            let shared: Vec<(usize, usize)> = candidates
                .iter()
                .map(|&at| {
                    (
                        at,
                        common_suffix(&wanted, &components(&self.tracks[at].path)),
                    )
                })
                .collect();
            let longest = shared.iter().map(|(_, len)| *len).max().unwrap_or(0);
            let mut best: Vec<usize> = shared
                .iter()
                .filter(|(_, len)| *len == longest)
                .map(|(at, _)| *at)
                .collect();
            let described = named.is_some() || duration.is_some();
            if best.len() > 1 && described {
                let fitting: Vec<usize> = best
                    .iter()
                    .copied()
                    .filter(|&at| self.fits(at, named, duration))
                    .collect();
                if !fitting.is_empty() {
                    best = fitting;
                }
            }
            // Only the file name agrees, although the line named folders too: "01 Intro.mp3"
            // is in a hundred albums. One candidate that the #EXTINF does not contradict is
            // taken; anything less certain is left to the names.
            let name_only = longest == 1 && wanted.len() > 1;
            let found = match best.as_slice() {
                [only] => {
                    (!name_only || !described || self.fits(*only, named, duration)).then_some(*only)
                }
                // The same folder and file twice (an album and its duplicate): the first.
                [first, ..] if longest >= 2 => Some(*first),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    fn by_name(&self, named: &[Named], duration: Option<f64>) -> Option<usize> {
        for reading in named {
            let Some(titled) = self.by_title.get(&reading.title) else {
                continue;
            };
            let mut hits: Vec<usize> = match &reading.artist {
                Some(artist) => titled
                    .iter()
                    .copied()
                    .filter(|&at| self.tracks[at].by(artist))
                    .collect(),
                None => titled.clone(),
            };
            if hits.len() > 1 {
                let timed: Vec<usize> = hits
                    .iter()
                    .copied()
                    .filter(|&at| self.tracks[at].lasts(duration))
                    .collect();
                if !timed.is_empty() {
                    hits = timed;
                }
            }
            match (hits.as_slice(), &reading.artist) {
                // The same song on two albums: either is right, the first is stable.
                ([first, ..], Some(_)) => return Some(*first),
                // A title alone says too little to pick one of several.
                ([only], None) => return Some(*only),
                _ => {}
            }
        }
        None
    }
}

/// One way to read an `#EXTINF` display.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Named {
    artist: Option<String>,
    title: String,
}

/// The ways to read "A - B - C": the artist is some of the parts from the front, the title some
/// that follow (what trails it is an addition like "Remastered 2011"). Fuller titles first; the
/// whole text as a bare title last.
fn named_like(display: &str) -> Vec<Named> {
    let parts: Vec<&str> = display.split(" - ").map(str::trim).collect();
    let mut readings = Vec::new();
    // More parts than this is prose, not "Artist - Title".
    if (2..=5).contains(&parts.len()) {
        for split in 1..parts.len() {
            let artist = match_key(&parts[..split].join(" "));
            for end in (split + 1..=parts.len()).rev() {
                let title = match_key(&parts[split..end].join(" "));
                if !artist.is_empty() && !title.is_empty() {
                    readings.push(Named {
                        artist: Some(artist.clone()),
                        title,
                    });
                }
            }
        }
    }
    let whole = match_key(display);
    if !whole.is_empty() {
        readings.push(Named {
            artist: None,
            title: whole,
        });
    }
    readings
}

/// A name without what would break a line of the file.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Extended M3U for a playlist: `#EXTINF:<seconds>,<artist> - <title>` and below it the path.
/// `tracks` carry the paths as they are to leave the server — relative already.
pub fn write(name: &str, tracks: &[ExportTrack]) -> String {
    let mut out = format!("#EXTM3U\n#PLAYLIST:{}\n", one_line(name));
    for track in tracks {
        let title = one_line(&track.title);
        let line = match track.artist.as_deref().map(one_line) {
            Some(artist) if !artist.is_empty() => format!("{artist} - {title}"),
            _ => title,
        };
        out.push_str(&format!(
            "#EXTINF:{},{line}\n{}\n",
            track.duration.round() as i64,
            one_line(&track.path)
        ));
    }
    out
}

/// A playlist's name as a file name every system takes: no separators, no characters Windows
/// refuses, no control characters, no dots or blanks at the ends. "Playlist" if nothing is left.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "Playlist".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i32, path: &str, artist: &str, title: &str, duration: f64) -> LibraryTrack {
        LibraryTrack {
            id,
            path: path.into(),
            title: Some(title.into()),
            artist: None,
            item_artist: Some(artist.into()),
            duration,
        }
    }

    fn library() -> TrackIndex {
        TrackIndex::new(vec![
            track(
                1,
                "/srv/musik/Björk/Debut/01 Human Behaviour.flac",
                "Björk",
                "Human Behaviour",
                252.0,
            ),
            track(
                2,
                "/srv/musik/Björk/Debut/02 Crying.flac",
                "Björk",
                "Crying",
                289.0,
            ),
            track(
                3,
                "/srv/musik/Air/Moon Safari/01 Intro.mp3",
                "Air",
                "La femme d'argent",
                429.0,
            ),
            track(
                4,
                "/srv/musik/Beck/Odelay/01 Intro.mp3",
                "Beck",
                "Devils Haircut",
                194.0,
            ),
            track(
                5,
                "/srv/musik/Queen/Live/07 Under Pressure.mp3",
                "Queen",
                "Under Pressure (Live)",
                221.0,
            ),
            track(
                6,
                "/srv/musik/Queen/Hot Space/11 Under Pressure.mp3",
                "Queen",
                "Under Pressure (Remastered 2011)",
                248.0,
            ),
        ])
    }

    fn path(path: &str) -> M3uEntry {
        M3uEntry {
            path: Some(path.into()),
            display: None,
            duration: None,
        }
    }

    fn info(display: &str, duration: Option<f64>) -> M3uEntry {
        M3uEntry {
            path: None,
            display: Some(display.into()),
            duration,
        }
    }

    #[test]
    fn reads_plain_and_extended_lists() {
        let entries = parse(
            "\u{feff}#EXTM3U\r\n#PLAYLIST:Zug\r\n\r\n#EXTINF:252,Björk - Human Behaviour\r\nBjörk/Debut/01.flac\r\n\
             #EXTALB:Debut\r\nohne/info.mp3\r\n#extinf:-1 tvg-id=\"x\",Nur Titel\r\n#EXTINF:12.4,Ende\r\n",
        );
        assert_eq!(
            entries,
            vec![
                M3uEntry {
                    path: Some("Björk/Debut/01.flac".into()),
                    display: Some("Björk - Human Behaviour".into()),
                    duration: Some(252.0),
                },
                path("ohne/info.mp3"),
                info("Nur Titel", None),
                info("Ende", Some(12.4)),
            ]
        );
        assert_eq!(entries[0].label(), "Björk - Human Behaviour");
        assert_eq!(entries[1].label(), "ohne/info.mp3");
        assert!(parse("").is_empty());
    }

    #[test]
    fn folding_takes_case_and_accents_off() {
        assert_eq!(fold("Björk"), "bjork");
        assert_eq!(fold("Bjo\u{308}rk"), "bjork");
        assert_eq!(fold("Sigur Rós"), "sigur ros");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(
            match_key("Under Pressure (Remastered 2011)"),
            "underpressure"
        );
        assert_eq!(match_key("Beyoncé"), "beyonce");
    }

    #[test]
    fn a_path_matches_relative_to_the_root_and_under_a_foreign_root() {
        let library = library();
        for line in [
            "Björk/Debut/01 Human Behaviour.flac",
            "/Volumes/Musik/Björk/Debut/01 Human Behaviour.flac",
            "C:\\Musik\\Björk\\Debut\\01 Human Behaviour.flac",
            "file:///home/x/Bj%C3%B6rk/Debut/01%20Human%20Behaviour.flac",
            "file://localhost/Volumes/M/Bjo%CC%88rk/Debut/01%20Human%20Behaviour.flac",
            "./BJÖRK/debut/01 human behaviour.FLAC",
            "01 Human Behaviour.flac",
        ] {
            assert_eq!(library.find(&path(line)), Some(1), "{line}");
        }
        assert_eq!(library.find(&path("Nichts/Davon/99 Fehlt.flac")), None);
        assert_eq!(
            library.find(&path("https://example.org/01 Intro.mp3")),
            None
        );
    }

    #[test]
    fn a_bare_file_name_must_be_unambiguous() {
        let library = library();
        // Folder and file agree: no doubt.
        assert_eq!(library.find(&path("x/Odelay/01 Intro.mp3")), Some(4));
        // Two albums have an "01 Intro.mp3".
        assert_eq!(library.find(&path("01 Intro.mp3")), None);
        assert_eq!(library.find(&path("Mix/01 Intro.mp3")), None);
        // The #EXTINF decides between them — by title or by length.
        let mut entry = path("Mix/01 Intro.mp3");
        entry.display = Some("Beck - Devils Haircut".into());
        assert_eq!(library.find(&entry), Some(4));
        let mut entry = path("Mix/01 Intro.mp3");
        entry.duration = Some(430.0);
        assert_eq!(library.find(&entry), Some(3));
        // A lone file of that name which the #EXTINF contradicts is not it.
        let mut entry = path("Sampler/02 Crying.flac");
        entry.display = Some("Roy Orbison - Crying".into());
        entry.duration = Some(166.0);
        assert_eq!(library.find(&entry), None);
    }

    #[test]
    fn artist_and_title_match_through_case_accents_and_additions() {
        let library = library();
        assert_eq!(
            library.find(&info("BJORK - human behaviour", None)),
            Some(1)
        );
        assert_eq!(
            library.find(&info("Bjork - Human Behaviour (2015 Remaster)", None)),
            Some(1)
        );
        assert_eq!(
            library.find(&info("Björk - Human Behaviour - Remastered", None)),
            Some(1)
        );
        assert_eq!(
            library.find(&info("Air - La Femme d’Argent", None)),
            Some(3)
        );
        assert_eq!(library.find(&info("Niemand - Human Behaviour", None)), None);
        // A title alone counts when only one track has it.
        assert_eq!(library.find(&info("Crying", None)), Some(2));
        assert_eq!(library.find(&info("Under Pressure", None)), None);
    }

    #[test]
    fn the_length_decides_between_several_hits() {
        let library = library();
        assert_eq!(
            library.find(&info("Queen - Under Pressure", Some(246.0))),
            Some(6)
        );
        assert_eq!(
            library.find(&info("Queen - Under Pressure", Some(223.9))),
            Some(5)
        );
        // No length, or one that fits neither: the first, rather than nothing.
        assert_eq!(library.find(&info("Queen - Under Pressure", None)), Some(5));
        assert_eq!(
            library.find(&info("Queen - Under Pressure", Some(600.0))),
            Some(5)
        );
        assert_eq!(library.find(&info("Under Pressure", Some(248.0))), Some(6));
    }

    #[test]
    fn a_path_that_fails_falls_back_to_the_names() {
        let library = library();
        let entry = M3uEntry {
            path: Some("/anderswo/track07.ogg".into()),
            display: Some("Beck - Devils Haircut".into()),
            duration: Some(194.0),
        };
        assert_eq!(library.find(&entry), Some(4));
    }

    #[test]
    fn writes_extended_m3u() {
        let tracks = vec![
            ExportTrack {
                title: "Human\nBehaviour".into(),
                artist: Some("Björk".into()),
                album: "Debut".into(),
                duration: 251.6,
                path: "Björk/Debut/01.flac".into(),
            },
            ExportTrack {
                title: "Kapitel 1".into(),
                artist: None,
                album: "Momo".into(),
                duration: 10.2,
                path: "Momo/1.mp3".into(),
            },
        ];
        assert_eq!(
            write("Zug\r\nfahrt", &tracks),
            "#EXTM3U\n#PLAYLIST:Zug  fahrt\n#EXTINF:252,Björk - Human Behaviour\nBjörk/Debut/01.flac\n\
             #EXTINF:10,Kapitel 1\nMomo/1.mp3\n"
        );
    }

    #[test]
    fn file_names_lose_what_a_file_system_refuses() {
        assert_eq!(safe_file_name("AC/DC: Best \"of\"?"), "AC_DC_ Best _of__");
        assert_eq!(safe_file_name("  ..Zug.. "), "Zug");
        assert_eq!(safe_file_name("Für Elise ♫"), "Für Elise ♫");
        assert_eq!(safe_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_file_name("..."), "Playlist");
    }
}
