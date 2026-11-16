//! Working out which series a book belongs to, and which volume it is (legacy
//! `scanner/series.ts`).
//!
//! Nothing in an audiobook file is authoritative here: some rippers write the series into ID3
//! movement/grouping frames (the Plex and Audiobookshelf convention), most people encode it in the
//! folder name, and the rest rely on an Autor/Serie/Buch tree. All three are read, in that order
//! of confidence.

use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesGuess {
    pub series: Option<String>,
    pub index: Option<f64>,
    /// The name with the series part removed — the plain book title.
    pub rest: Option<String>,
}

impl SeriesGuess {
    const NONE: SeriesGuess = SeriesGuess {
        series: None,
        index: None,
        rest: None,
    };
}

/// `_` too: rippers write it where a file name may not hold a colon ("Folge 237_ Titel").
const SEPARATOR: &str = "[-–—:·_]";
/// "Band", "Teil", "Folge", "Fall", "Vol." … the words that introduce a volume number.
const VOLUME_WORD: &str = r"(?:band|teil|folge|fall|nr\.?|no\.?|vol\.?|volume|book|buch)";
const NUMBER: &str = r"([0-9]{1,4}(?:[.,][0-9]{1,2})?)";

struct Pattern {
    re: Regex,
    /// Without a "Band"/"Folge" in front of it, a four-digit number is a year.
    reject_years: bool,
}

static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    let pattern = |re: String, reject_years: bool| Pattern {
        re: Regex::new(&re).expect("series pattern"),
        reject_years,
    };
    vec![
        // "Perry Rhodan, Band 3 - Titel" / "Perry Rhodan - Teil 3: Titel" / "Serie Vol. 3"
        pattern(
            format!(
                r"(?i)^(.+?)[,\s]*\s*{SEPARATOR}?\s*\b{VOLUME_WORD}\s*{NUMBER}(?:\b|_)\s*{SEPARATOR}?\s*(.*)$"
            ),
            false,
        ),
        // "Perry Rhodan #3 - Titel"
        pattern(
            format!(r"^(.+?)\s*#\s*{NUMBER}\s*{SEPARATOR}?\s*(.*)$"),
            false,
        ),
        // "Perry Rhodan (3) - Titel"
        pattern(
            format!(r"^(.+?)\s*\(\s*{NUMBER}\s*\)\s*{SEPARATOR}?\s*(.*)$"),
            false,
        ),
        // "Der Herr der Ringe 01 - Die Gefährten": a number between name and title, which only
        // counts when a separator follows — otherwise "Album 2000" would match.
        pattern(format!(r"^(.+?)\s+{NUMBER}\s*{SEPARATOR}\s*(.+)$"), true),
    ]
});

/// A leading volume number: "03 - Der Fluch", "3. Der Fluch".
static LEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^{NUMBER}\s*(?:{SEPARATOR}|\.)\s*(.+)$")).expect("leading pattern")
});

/// A series tag that is nothing but a volume word ("Folge") names no series — some rippers write
/// it into the movement frame of every episode.
static BARE_VOLUME_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)^\s*{VOLUME_WORD}\s*$")).expect("bare volume word pattern")
});

/// The volume in what follows a series name: "Folge 237_ Titel", "131_Titel",
/// "Die neuen Fälle, Fall 60_ Titel". A volume word may come first, and words before it; without
/// one the number has to lead.
static VOLUME_AND_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^(?:.*?\b{VOLUME_WORD}\s*)?{NUMBER}(?:\s*(?:{SEPARATOR}|\.)\s*(.*)|\s*)$"
    ))
    .expect("volume pattern")
});

static CLEAN_LEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^\s*{SEPARATOR}+\s*")).unwrap());
static CLEAN_TRAILING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\s*{SEPARATOR}+\s*$")).unwrap());
static CLEAN_COMMAS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s,]+$").unwrap());
static CLEAN_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

fn to_index(raw: &str) -> Option<f64> {
    raw.replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn clean(value: &str) -> String {
    let value = CLEAN_LEADING.replace(value, "");
    let value = CLEAN_TRAILING.replace(&value, "");
    let value = CLEAN_COMMAS.replace(&value, "");
    let value = CLEAN_SPACES.replace_all(&value, " ");
    value.trim().to_owned()
}

/// Reads a series and volume out of one name (a folder or an album tag). All `None` when the name
/// carries no series at all.
pub fn parse_series_name(name: &str) -> SeriesGuess {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return SeriesGuess::NONE;
    }
    for pattern in PATTERNS.iter() {
        let Some(caps) = pattern.re.captures(trimmed) else {
            continue;
        };
        let series = clean(&caps[1]);
        let index = to_index(&caps[2]);
        let rest = clean(caps.get(3).map_or("", |m| m.as_str()));
        // A series name of one or two characters is far more likely to be a stray number than a
        // real series.
        let Some(index) = index else { continue };
        if series.chars().count() < 3 {
            continue;
        }
        // A bare 1984 between a name and a title is a year, not volume 1984.
        if pattern.reject_years && index.fract() == 0.0 && (1500.0..=2299.0).contains(&index) {
            continue;
        }
        return SeriesGuess {
            series: Some(series),
            index: Some(index),
            rest: (!rest.is_empty()).then_some(rest),
        };
    }
    SeriesGuess::NONE
}

/// A volume number without a series name: the folder sits inside the series folder.
pub fn parse_leading_index(name: &str) -> (Option<f64>, Option<String>) {
    let Some(caps) = LEADING.captures(name.trim()) else {
        return (None, None);
    };
    match to_index(&caps[1]) {
        None => (None, None),
        Some(index) => {
            let rest = clean(&caps[2]);
            (Some(index), (!rest.is_empty()).then_some(rest))
        }
    }
}

/// The volume and the title after it, out of the text that follows a series name.
fn volume_and_title(text: &str) -> (Option<f64>, Option<String>) {
    let Some(caps) = VOLUME_AND_TITLE.captures(text.trim()) else {
        return (None, None);
    };
    let rest = clean(caps.get(2).map_or("", |m| m.as_str()));
    (to_index(&caps[1]), (!rest.is_empty()).then_some(rest))
}

/// What follows `author` at the start of `name` — "TKKG - 131_Titel" with author "TKKG" gives
/// "131_Titel". `None` unless the name starts with the author and a separator.
fn after_author<'n>(name: &'n str, author: &str) -> Option<&'n str> {
    let author = author.trim();
    if author.is_empty() {
        return None;
    }
    let head = name.trim_start().get(..author.len())?;
    if head.to_lowercase() != author.to_lowercase() {
        return None;
    }
    let rest = &name.trim_start()[author.len()..];
    let after = CLEAN_LEADING.replace(rest, "");
    // Only a separator turns the author into a prefix: "TKKG Junior" is a series of its own.
    (after.len() < rest.len()).then(|| &rest[rest.len() - after.len()..])
}

#[derive(Debug, Clone, Default)]
pub struct SeriesSource<'a> {
    /// Series written into the file's tags, if the ripper bothered.
    pub tag_series: Option<&'a str>,
    pub tag_series_index: Option<f64>,
    /// The folder holding the audio files.
    pub folder_name: &'a str,
    /// The folder above it — the series folder in an Autor/Serie/Buch tree.
    pub parent_name: Option<&'a str>,
    /// True when the tree is deep enough for the parent to be a series.
    pub parent_is_series: bool,
    /// The author folder of an Autor/Buch tree (two levels, no series folder between).
    pub author_folder: Option<&'a str>,
    /// The title the scanner would use otherwise (an album tag, usually).
    pub title: &'a str,
    /// Whether that title came from a tag rather than from the folder name.
    pub title_from_tag: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesResolution {
    pub series: Option<String>,
    pub series_index: Option<f64>,
    /// The title to store — stripped of the series prefix when it was in the folder name.
    pub title: String,
}

/// Tags first, then the folder name, then the shape of the tree. Every source can fill in what an
/// earlier one left open: an Autor/Serie/Buch tree that names its book folder "03 - Titel" gives
/// the series from the path and the volume from the folder.
pub fn resolve_series(source: &SeriesSource<'_>) -> SeriesResolution {
    let from_folder = parse_series_name(source.folder_name);
    let (leading_index, leading_rest) = parse_leading_index(source.folder_name);
    let from_title = if source.title_from_tag {
        parse_series_name(source.title)
    } else {
        from_folder.clone()
    };

    let mut series = source
        .tag_series
        .map(str::trim)
        .filter(|s| !s.is_empty() && !BARE_VOLUME_WORD.is_match(s))
        .map(str::to_owned);
    let mut index = source.tag_series_index;
    let mut title = source.title.to_owned();

    if series.is_none()
        && source.parent_is_series
        && let Some(parent) = source.parent_name.filter(|p| !p.is_empty())
    {
        series = Some(parent.to_owned());
        if index.is_none() {
            index = leading_index.or(from_folder.index);
        }
        // "03 - Der Fluch" inside a series folder: the number is not part of the title.
        if !source.title_from_tag
            && let Some(rest) = &leading_rest
        {
            title = rest.clone();
        }
    }

    // An Autor/Buch tree whose book folders (or album tags) repeat the author in front:
    // "TKKG/TKKG - 131_Titel", "Sherlock Holmes/Sherlock Holmes - Die neuen Fälle, Fall 60_ Titel".
    // The author folder is the series — whatever the rest calls it, and however it is spelt from
    // one rip to the next; reading the series out of the rest split one shelf into several.
    if series.is_none()
        && !source.parent_is_series
        && let Some(author) = source.author_folder.filter(|a| !a.trim().is_empty())
    {
        let in_folder = after_author(source.folder_name, author);
        let in_title = source
            .title_from_tag
            .then(|| after_author(source.title, author))
            .flatten();
        if in_folder.is_some() || in_title.is_some() {
            series = Some(author.trim().to_owned());
            let (folder_index, folder_rest) = in_folder.map(volume_and_title).unwrap_or_default();
            let (title_index, _) = in_title.map(volume_and_title).unwrap_or_default();
            if index.is_none() {
                index = folder_index
                    .or(from_folder.index)
                    .or(leading_index)
                    .or(title_index);
            }
            if !source.title_from_tag
                && let Some(rest) = folder_rest
            {
                title = rest;
            }
        }
    }

    if series.is_none()
        && let Some(found) = &from_folder.series
    {
        series = Some(found.clone());
        if index.is_none() {
            index = from_folder.index;
        }
        if !source.title_from_tag
            && let Some(rest) = &from_folder.rest
        {
            title = rest.clone();
        }
    }

    if series.is_none()
        && source.title_from_tag
        && let Some(found) = &from_title.series
    {
        series = Some(found.clone());
        if index.is_none() {
            index = from_title.index;
        }
        if let Some(rest) = &from_title.rest {
            title = rest.clone();
        }
    }

    if series.is_some() && index.is_none() {
        index = leading_index
            .or(from_folder.index)
            .or(if source.title_from_tag {
                from_title.index
            } else {
                None
            });
    }

    SeriesResolution {
        series,
        series_index: index,
        title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(series: &str, index: f64, rest: &str) -> SeriesGuess {
        SeriesGuess {
            series: Some(series.into()),
            index: Some(index),
            rest: Some(rest.into()),
        }
    }

    #[test]
    fn reads_the_german_shelf_conventions() {
        assert_eq!(
            parse_series_name("Perry Rhodan, Band 3 - Der Fluch"),
            guess("Perry Rhodan", 3.0, "Der Fluch")
        );
        assert_eq!(
            parse_series_name("Die drei ??? - Folge 42: Der Karpatenhund"),
            guess("Die drei ???", 42.0, "Der Karpatenhund")
        );
        assert_eq!(
            parse_series_name("Der Herr der Ringe 01 - Die Gefährten"),
            guess("Der Herr der Ringe", 1.0, "Die Gefährten")
        );
        assert_eq!(
            parse_series_name("Discworld #5 - Sourcery"),
            guess("Discworld", 5.0, "Sourcery")
        );
        assert_eq!(
            parse_series_name("Bobiverse (2) We Are Legion"),
            guess("Bobiverse", 2.0, "We Are Legion")
        );
    }

    #[test]
    fn keeps_half_volumes_which_is_why_the_column_is_not_an_integer() {
        assert_eq!(
            parse_series_name("Perry Rhodan, Band 3.5 - Zwischenspiel").index,
            Some(3.5)
        );
        assert_eq!(
            parse_series_name("Perry Rhodan, Band 3,5 - Zwischenspiel").index,
            Some(3.5)
        );
    }

    #[test]
    fn leaves_a_plain_title_alone() {
        assert_eq!(parse_series_name("Das Parfum"), SeriesGuess::NONE);
        // A bare number after a name is a year or an edition, not a volume.
        assert_eq!(parse_series_name("Nevermind 1991"), SeriesGuess::NONE);
        // Two characters cannot be a series name.
        assert_eq!(parse_series_name("X 3 - Etwas"), SeriesGuess::NONE);
        // A bare year reads as a year; with "Band" in front of it, it is a volume again.
        assert_eq!(
            parse_series_name("Das Jahr 1984 - Roman"),
            SeriesGuess::NONE
        );
        assert_eq!(
            parse_series_name("Perry Rhodan, Band 1984 - Titel"),
            guess("Perry Rhodan", 1984.0, "Titel")
        );
    }

    #[test]
    fn picks_the_volume_off_a_folder_that_sits_inside_its_series() {
        assert_eq!(
            parse_leading_index("03 - Der Fluch"),
            (Some(3.0), Some("Der Fluch".into()))
        );
        assert_eq!(
            parse_leading_index("7. Der Fluch"),
            (Some(7.0), Some("Der Fluch".into()))
        );
        assert_eq!(parse_leading_index("Der Fluch"), (None, None));
    }

    fn resolved(series: Option<&str>, index: Option<f64>, title: &str) -> SeriesResolution {
        SeriesResolution {
            series: series.map(Into::into),
            series_index: index,
            title: title.into(),
        }
    }

    #[test]
    fn believes_the_tags_before_anything_else() {
        let source = SeriesSource {
            tag_series: Some("Perry Rhodan"),
            tag_series_index: Some(12.0),
            folder_name: "Irgendein Ordner",
            title: "Der Fluch",
            title_from_tag: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_series(&source),
            resolved(Some("Perry Rhodan"), Some(12.0), "Der Fluch")
        );
    }

    #[test]
    fn takes_the_series_from_the_tree_and_the_volume_from_the_folder() {
        let source = SeriesSource {
            folder_name: "03 - Der Fluch",
            parent_name: Some("Die drei ???"),
            parent_is_series: true,
            title: "03 - Der Fluch",
            ..Default::default()
        };
        assert_eq!(
            resolve_series(&source),
            resolved(Some("Die drei ???"), Some(3.0), "Der Fluch")
        );
    }

    #[test]
    fn reads_both_out_of_the_folder_name_when_the_tree_is_flat() {
        let source = SeriesSource {
            folder_name: "Der Herr der Ringe 01 - Die Gefährten",
            title: "Der Herr der Ringe 01 - Die Gefährten",
            ..Default::default()
        };
        assert_eq!(
            resolve_series(&source),
            resolved(Some("Der Herr der Ringe"), Some(1.0), "Die Gefährten")
        );
    }

    #[test]
    fn does_not_rewrite_a_title_that_came_from_a_tag() {
        let source = SeriesSource {
            folder_name: "Der Herr der Ringe 01 - Die Gefährten",
            title: "Die Gefährten",
            title_from_tag: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_series(&source),
            resolved(Some("Der Herr der Ringe"), Some(1.0), "Die Gefährten")
        );
    }

    fn in_author_folder<'a>(folder: &'a str, title: &'a str, from_tag: bool) -> SeriesSource<'a> {
        SeriesSource {
            folder_name: folder,
            title,
            title_from_tag: from_tag,
            author_folder: Some(folder.split('/').next().unwrap_or("")),
            ..Default::default()
        }
    }

    /// The shelves of one TKKG rip, spelt four ways; all of them are one series.
    #[test]
    fn the_author_folder_is_the_series_when_the_book_folders_repeat_it() {
        let tkkg = |folder: &'static str,
                    title: &'static str,
                    from_tag: bool,
                    tag: Option<&'static str>| {
            resolve_series(&SeriesSource {
                tag_series: tag,
                author_folder: Some("TKKG"),
                ..in_author_folder(folder, title, from_tag)
            })
        };
        assert_eq!(
            tkkg(
                "TKKG - 131_Stundenlohn für flotte Gangster",
                "TKKG - 131_Stundenlohn für flotte Gangster",
                false,
                None
            ),
            resolved(Some("TKKG"), Some(131.0), "Stundenlohn für flotte Gangster")
        );
        assert_eq!(
            tkkg(
                "TKKG - 100_Fieser Trick mit Nr. 100",
                "100/Fieser Trick mit Nr. 100",
                true,
                None
            ),
            resolved(Some("TKKG"), Some(100.0), "100/Fieser Trick mit Nr. 100")
        );
        // The ripper wrote "Folge" into the series tag: a volume word is no series.
        assert_eq!(
            tkkg(
                "TKKG - Folge 237_ K.I. Kriminelle Illusion",
                "K.I. Kriminelle Illusion",
                true,
                Some("Folge")
            ),
            resolved(Some("TKKG"), Some(237.0), "K.I. Kriminelle Illusion")
        );
    }

    #[test]
    fn the_author_in_the_album_tag_is_enough_when_the_folder_leaves_it_out() {
        let source = SeriesSource {
            author_folder: Some("Sherlock Holmes"),
            ..in_author_folder(
                "Die neuen Fälle - Fall 07 - Der eisige Tod",
                "Sherlock Holmes - Die neuen Fälle, Fall ",
                true,
            )
        };
        assert_eq!(
            resolve_series(&source)
                .series_index
                .zip(resolve_series(&source).series),
            Some((7.0, "Sherlock Holmes".to_owned()))
        );
        let source = SeriesSource {
            author_folder: Some("Sherlock Holmes"),
            ..in_author_folder(
                "Sherlock Holmes - Die neuen Fälle, Fall 60_ Nervus Rerum",
                "Sherlock Holmes - Die neuen Fälle, Fall 60_ Nervus Rerum",
                false,
            )
        };
        assert_eq!(
            resolve_series(&source),
            resolved(Some("Sherlock Holmes"), Some(60.0), "Nervus Rerum")
        );
    }

    #[test]
    fn the_author_folder_is_no_series_without_the_prefix() {
        // "Tolkien/Der Herr der Ringe 01 - …" keeps the series its folder names.
        let source = SeriesSource {
            author_folder: Some("Tolkien"),
            ..in_author_folder(
                "Der Herr der Ringe 01 - Die Gefährten",
                "Der Herr der Ringe 01 - Die Gefährten",
                false,
            )
        };
        assert_eq!(
            resolve_series(&source).series.as_deref(),
            Some("Der Herr der Ringe")
        );
        // A longer name that merely starts like the author is another series.
        let source = SeriesSource {
            author_folder: Some("TKKG"),
            ..in_author_folder("TKKG Junior 03 - Titel", "TKKG Junior 03 - Titel", false)
        };
        assert_eq!(
            resolve_series(&source).series.as_deref(),
            Some("TKKG Junior")
        );
        // A real series tag still wins.
        let source = SeriesSource {
            tag_series: Some("Perry Rhodan"),
            author_folder: Some("TKKG"),
            ..in_author_folder("TKKG - 03 - Titel", "Titel", true)
        };
        assert_eq!(
            resolve_series(&source).series.as_deref(),
            Some("Perry Rhodan")
        );
    }

    #[test]
    fn reads_fall_and_underscores_like_folge_and_dashes() {
        assert_eq!(
            parse_series_name("Die neuen Fälle, Fall 60_ Nervus Rerum"),
            guess("Die neuen Fälle", 60.0, "Nervus Rerum")
        );
        assert_eq!(
            parse_series_name("Die neuen Fälle - Fall 07 - Der eisige Tod"),
            guess("Die neuen Fälle", 7.0, "Der eisige Tod")
        );
    }

    #[test]
    fn leaves_a_standalone_book_without_a_series() {
        let source = SeriesSource {
            folder_name: "Das Parfum",
            title: "Das Parfum",
            ..Default::default()
        };
        assert_eq!(resolve_series(&source), resolved(None, None, "Das Parfum"));
    }
}
