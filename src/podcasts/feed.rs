//! Feed parsing (legacy `podcasts/feed.ts`, which used rss-parser).
//!
//! feed-rs is the parser of record: it decides whether a document is a feed at all (RSS 0.9–2,
//! Atom, JSON Feed), and it reads titles, descriptions and dates. What it does not model — the raw
//! `<guid>`/`<link>`, the enclosure URL as written, `<itunes:duration>`, `<itunes:summary>`,
//! `<itunes:image>`, Podlove `<psc:chapters>` and Podcasting 2.0 `<podcast:chapters>` — comes from a
//! second pass over the same RSS document with roxmltree, matched to the entries by position.

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedChapter {
    pub start: f64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEpisode {
    pub guid: String,
    pub title: String,
    pub published_at: Option<DateTime<Utc>>,
    pub media_url: Option<String>,
    pub description: Option<String>,
    pub duration_seconds: Option<f64>,
    /// Podlove Simple Chapters, carried inline in the feed.
    pub chapters: Vec<ParsedChapter>,
    /// Podcasting 2.0 points at a JSON file instead; fetched at download time.
    pub chapters_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFeed {
    pub title: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub episodes: Vec<ParsedEpisode>,
}

/// The document is not a feed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct InvalidFeed(pub String);

pub const NOT_A_FEED: &str = "Kein gültiger RSS- oder Atom-Feed";

static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static PSC_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{1,3}(:\d{1,2}){0,2}([.,]\d{1,3})?$").unwrap());

fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Tags out, the common entities decoded, "Ohne Titel" if nothing is left.
pub fn clean_title(raw: Option<&str>) -> String {
    let stripped = TAG.replace_all(raw.unwrap_or(""), "");
    let decoded = decode_entities(&stripped);
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        "Ohne Titel".into()
    } else {
        trimmed.to_owned()
    }
}

/// Tags out (as spaces), entities decoded, whitespace collapsed; `None` if nothing is left.
pub fn clean_text(raw: Option<&str>) -> Option<String> {
    let raw = raw.filter(|r| !r.is_empty())?;
    let stripped = TAG.replace_all(raw, " ");
    let decoded = decode_entities(&stripped);
    let collapsed = SPACE.replace_all(&decoded, " ");
    let trimmed = collapsed.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// `<itunes:duration>`: `SS`, `MM:SS` or `H:MM:SS`, digits only.
pub fn duration_to_seconds(raw: Option<&str>) -> Option<f64> {
    let raw = raw?.trim();
    let mut total = 0.0;
    for part in raw.split(':') {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    total.is_finite().then_some(total)
}

/// Podlove timestamps: "HH:MM:SS.mmm", "MM:SS", or "SS" — normal play time as defined by the PSC
/// spec, so the hour part is optional and the fraction is too.
pub fn psc_time_to_seconds(raw: Option<&str>) -> Option<f64> {
    let trimmed = raw?.trim();
    if !PSC_TIME.is_match(trimmed) {
        return None;
    }
    let mut halves = trimmed.split(['.', ',']);
    let whole = halves.next().unwrap_or("");
    let fraction = halves.next();
    let seconds = whole
        .split(':')
        .try_fold(0.0, |total, part| {
            part.parse::<f64>().map(|n| total * 60.0 + n)
        })
        .ok()?;
    let millis = fraction
        .map_or(Ok(0.0), |f| format!("0.{f}").parse::<f64>())
        .ok()?;
    Some(seconds + millis)
}

/// What the second pass reads out of one RSS `<item>`.
#[derive(Debug, Default)]
struct ItemExtras {
    raw_title: Option<String>,
    guid: Option<String>,
    link: Option<String>,
    enclosure: Option<String>,
    itunes_duration: Option<String>,
    itunes_summary: Option<String>,
    chapters: Vec<ParsedChapter>,
    chapters_url: Option<String>,
}

#[derive(Debug, Default)]
struct ChannelExtras {
    itunes_image: Option<String>,
    image_url: Option<String>,
    items: Vec<ItemExtras>,
}

fn is_ns(node: &roxmltree::Node, needle: &str) -> bool {
    node.tag_name()
        .namespace()
        .is_some_and(|ns| ns.contains(needle))
}

fn plain(node: &roxmltree::Node, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace().is_none() && node.tag_name().name() == name
}

fn text_of(node: &roxmltree::Node) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect()
}

/// Inline `<psc:chapters><psc:chapter start=… title=…/></psc:chapters>`, sorted, without a second
/// chapter at the same start (it would break the chapter list's unique position index).
fn psc_chapters(container: &roxmltree::Node) -> Vec<ParsedChapter> {
    let mut chapters: Vec<ParsedChapter> = container
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "chapter")
        .filter_map(|n| {
            let start = psc_time_to_seconds(n.attribute("start"))?;
            Some(ParsedChapter {
                start,
                title: clean_title(n.attribute("title")),
            })
        })
        .collect();
    chapters.sort_by(|a, b| a.start.total_cmp(&b.start));
    chapters.dedup_by(|later, earlier| later.start == earlier.start);
    chapters
}

fn item_extras(item: &roxmltree::Node) -> ItemExtras {
    let mut extras = ItemExtras::default();
    for child in item.children().filter(|n| n.is_element()) {
        let name = child.tag_name().name();
        if plain(&child, "title") {
            extras.raw_title = Some(text_of(&child));
        } else if plain(&child, "guid") {
            extras.guid = Some(text_of(&child).trim().to_owned()).filter(|g| !g.is_empty());
        } else if plain(&child, "link") {
            extras.link = Some(text_of(&child).trim().to_owned()).filter(|l| !l.is_empty());
        } else if plain(&child, "enclosure") && extras.enclosure.is_none() {
            extras.enclosure = child.attribute("url").map(str::to_owned);
        } else if is_ns(&child, "itunes.com") && name == "duration" {
            extras.itunes_duration = Some(text_of(&child));
        } else if is_ns(&child, "itunes.com") && name == "summary" {
            extras.itunes_summary = Some(text_of(&child));
        } else if is_ns(&child, "podlove.org") && name == "chapters" {
            extras.chapters = psc_chapters(&child);
        } else if is_ns(&child, "podcastindex.org") && name == "chapters" {
            extras.chapters_url = child
                .attribute("url")
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .map(str::to_owned);
        }
    }
    extras
}

/// `None` unless the document is well-formed RSS with a `<channel>`.
fn channel_extras(xml: &str) -> Option<ChannelExtras> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(xml, options).ok()?;
    let root = doc.root_element();
    if root.tag_name().name() != "rss" {
        return None;
    }
    let channel = root.children().find(|n| plain(n, "channel"))?;
    let mut extras = ChannelExtras::default();
    for child in channel.children().filter(|n| n.is_element()) {
        if plain(&child, "item") {
            extras.items.push(item_extras(&child));
        } else if is_ns(&child, "itunes.com") && child.tag_name().name() == "image" {
            extras.itunes_image = child.attribute("href").map(str::to_owned);
        } else if plain(&child, "image") {
            extras.image_url = child
                .children()
                .find(|n| plain(n, "url"))
                .map(|n| text_of(&n).trim().to_owned())
                .filter(|u| !u.is_empty());
        }
    }
    Some(extras)
}

/// An id for an item that has neither `<guid>` nor `<link>`: stable for the same position and
/// title.
fn generated_guid(index: usize, title: &str) -> String {
    let digest = Sha256::digest(format!("{index}:{title}").as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("generated:{hex}")
}

fn enclosure_of(entry: &feed_rs::model::Entry) -> Option<String> {
    entry
        .media
        .iter()
        .flat_map(|m| m.content.iter())
        .find_map(|c| c.url.as_ref().map(|u| u.to_string()))
        .or_else(|| {
            entry
                .links
                .iter()
                .find(|l| l.rel.as_deref() == Some("enclosure"))
                .map(|l| l.href.clone())
        })
}

pub fn parse_feed(xml: &str) -> Result<ParsedFeed, InvalidFeed> {
    let feed =
        feed_rs::parser::parse(xml.as_bytes()).map_err(|_| InvalidFeed(NOT_A_FEED.into()))?;
    // Only trusted when it saw the same items feed-rs did.
    let extras = channel_extras(xml).filter(|e| e.items.len() == feed.entries.len());

    let episodes = feed
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let extra = extras.as_ref().map(|e| &e.items[index]);
            let title_text = entry.title.as_ref().map(|t| t.content.as_str());
            let raw_title = extra
                .and_then(|x| x.raw_title.as_deref())
                .or(title_text)
                .unwrap_or("");
            let guid = match extra {
                Some(x) => x.guid.clone().or_else(|| x.link.clone()),
                None => Some(entry.id.clone())
                    .filter(|id| !id.is_empty())
                    .or_else(|| entry.links.first().map(|l| l.href.clone())),
            }
            .unwrap_or_else(|| generated_guid(index, raw_title));
            let description = clean_text(entry.summary.as_ref().map(|s| s.content.as_str()))
                .or_else(|| clean_text(extra.and_then(|x| x.itunes_summary.as_deref())))
                .or_else(|| clean_text(entry.content.as_ref().and_then(|c| c.body.as_deref())));
            ParsedEpisode {
                guid,
                title: clean_title(title_text),
                published_at: entry.published.or(match extras {
                    Some(_) => None,
                    None => entry.updated,
                }),
                media_url: match extra {
                    Some(x) => x.enclosure.clone(),
                    None => enclosure_of(entry),
                },
                description,
                duration_seconds: duration_to_seconds(
                    extra.and_then(|x| x.itunes_duration.as_deref()),
                ),
                chapters: extra.map(|x| x.chapters.clone()).unwrap_or_default(),
                chapters_url: extra.and_then(|x| x.chapters_url.clone()),
            }
        })
        .collect();

    let image_url = extras
        .as_ref()
        .and_then(|e| e.itunes_image.clone().or_else(|| e.image_url.clone()))
        .or_else(|| feed.logo.as_ref().map(|i| i.uri.clone()))
        .or_else(|| feed.icon.as_ref().map(|i| i.uri.clone()));
    Ok(ParsedFeed {
        title: clean_title(feed.title.as_ref().map(|t| t.content.as_str())),
        description: clean_text(feed.description.as_ref().map(|d| d.content.as_str())),
        image_url,
        episodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const SAMPLE_RSS: &str = r#"<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" version="2.0"><channel>
<title>Maschinenraum</title>
<description>&lt;p&gt;Ein Podcast   über  Technik&lt;/p&gt;</description>
<itunes:image href="https://example.com/cover (2).jpg"/>
<item>
  <title>&lt;b&gt;Der lange Weg&lt;/b&gt; zum Netz</title>
  <guid>ep-118</guid>
  <description>Wie das Netz entstand</description>
  <itunes:duration>1:02:03</itunes:duration>
  <pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate>
  <enclosure url="https://example.com/118.mp3" type="audio/mpeg" length="123"/>
</item>
<item>
  <title>Ohne Guid</title>
  <pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate>
</item>
</channel></rss>"#;

    fn sample() -> ParsedFeed {
        parse_feed(SAMPLE_RSS).unwrap()
    }

    #[test]
    fn strips_html_tags_and_unescapes_entities_from_titles() {
        assert_eq!(sample().episodes[0].title, "Der lange Weg zum Netz");
    }

    #[test]
    fn uses_the_guid_when_present() {
        assert_eq!(sample().episodes[0].guid, "ep-118");
    }

    #[test]
    fn falls_back_to_a_generated_sha256_guid() {
        let guid = &sample().episodes[1].guid;
        let re = Regex::new(r"^generated:[a-f0-9]{64}$").unwrap();
        assert!(re.is_match(guid), "{guid}");
        // Stable across parses.
        assert_eq!(&sample().episodes[1].guid, guid);
    }

    #[test]
    fn falls_back_to_the_link_without_a_guid() {
        let xml = SAMPLE_RSS.replace(
            "<title>Ohne Guid</title>",
            "<title>Ohne Guid</title><link>https://example.com/ep</link>",
        );
        assert_eq!(
            parse_feed(&xml).unwrap().episodes[1].guid,
            "https://example.com/ep"
        );
    }

    #[test]
    fn reads_the_enclosure_url_none_when_absent() {
        let feed = sample();
        assert_eq!(
            feed.episodes[0].media_url.as_deref(),
            Some("https://example.com/118.mp3")
        );
        assert_eq!(feed.episodes[1].media_url, None);
    }

    #[test]
    fn parses_the_pub_date_to_utc() {
        assert_eq!(
            sample().episodes[0].published_at,
            Some(Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap())
        );
    }

    #[test]
    fn falls_back_to_ohne_titel_for_an_empty_title() {
        let xml = SAMPLE_RSS.replace(
            "<title>&lt;b&gt;Der lange Weg&lt;/b&gt; zum Netz</title>",
            "<title></title>",
        );
        assert_eq!(parse_feed(&xml).unwrap().episodes[0].title, "Ohne Titel");
    }

    #[test]
    fn reads_the_channel_description_stripped_and_collapsed() {
        assert_eq!(
            sample().description.as_deref(),
            Some("Ein Podcast über Technik")
        );
    }

    #[test]
    fn prefers_itunes_image_for_the_artwork() {
        assert_eq!(
            sample().image_url.as_deref(),
            Some("https://example.com/cover (2).jpg")
        );
    }

    #[test]
    fn no_description_and_no_artwork_when_the_feed_has_neither() {
        let bare = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Leer</title></channel></rss>"#;
        let feed = parse_feed(bare).unwrap();
        assert_eq!(feed.description, None);
        assert_eq!(feed.image_url, None);
        assert_eq!(feed.title, "Leer");
    }

    #[test]
    fn normalises_itunes_duration_to_seconds() {
        assert_eq!(sample().episodes[0].duration_seconds, Some(3723.0));
        let seconds = SAMPLE_RSS.replace("<itunes:duration>1:02:03", "<itunes:duration>930");
        assert_eq!(
            parse_feed(&seconds).unwrap().episodes[0].duration_seconds,
            Some(930.0)
        );
        let bogus = SAMPLE_RSS.replace("<itunes:duration>1:02:03", "<itunes:duration>eine Stunde");
        assert_eq!(
            parse_feed(&bogus).unwrap().episodes[0].duration_seconds,
            None
        );
    }

    #[test]
    fn reads_an_episode_description_none_when_absent() {
        let feed = sample();
        assert_eq!(
            feed.episodes[0].description.as_deref(),
            Some("Wie das Netz entstand")
        );
        assert_eq!(feed.episodes[1].description, None);
    }

    #[test]
    fn refuses_a_document_that_is_no_feed() {
        assert_eq!(
            parse_feed("<html>not a feed</html>"),
            Err(InvalidFeed(NOT_A_FEED.into()))
        );
        assert!(parse_feed("nicht einmal xml").is_err());
    }

    #[test]
    fn reads_an_atom_feed_too() {
        let atom = r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom">
            <title>Atom-Show</title><id>urn:x</id><updated>2026-01-01T00:00:00Z</updated>
            <entry><title>Eins</title><id>urn:e1</id><updated>2026-01-02T00:00:00Z</updated>
              <link rel="enclosure" href="https://x/1.mp3"/></entry></feed>"#;
        let feed = parse_feed(atom).unwrap();
        assert_eq!(feed.title, "Atom-Show");
        assert_eq!(feed.episodes[0].guid, "urn:e1");
        assert_eq!(
            feed.episodes[0].media_url.as_deref(),
            Some("https://x/1.mp3")
        );
        assert!(feed.episodes[0].published_at.is_some());
    }

    // podcast-chapters.test.ts

    #[test]
    fn psc_time_reads_normal_play_time_in_all_its_shapes() {
        assert_eq!(psc_time_to_seconds(Some("00:00:00.000")), Some(0.0));
        assert_eq!(psc_time_to_seconds(Some("00:12:30")), Some(750.0));
        assert_eq!(psc_time_to_seconds(Some("1:02:00.500")), Some(3720.5));
        assert_eq!(psc_time_to_seconds(Some("45")), Some(45.0));
    }

    #[test]
    fn psc_time_refuses_anything_that_is_not_a_timestamp() {
        assert_eq!(psc_time_to_seconds(Some("später")), None);
        assert_eq!(psc_time_to_seconds(Some("")), None);
        assert_eq!(psc_time_to_seconds(None), None);
    }

    const CHAPTER_FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:psc="http://podlove.org/simple-chapters"
     xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
     xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Apfelfunk</title>
    <item>
      <title>Folge 500</title><guid>abc</guid>
      <enclosure url="https://x/500.mp3" type="audio/mpeg" length="1"/>
      <itunes:duration>01:00:00</itunes:duration>
      <psc:chapters version="1.2">
        <psc:chapter start="00:12:30" title="Thema 1"/>
        <psc:chapter start="00:00:00.000" title="Begrüßung"/>
        <psc:chapter start="00:12:30" title="Doppelt"/>
        <psc:chapter start="kaputt" title="Ungültig"/>
      </psc:chapters>
    </item>
    <item>
      <title>Folge 499</title><guid>def</guid>
      <enclosure url="https://x/499.mp3" type="audio/mpeg" length="1"/>
      <podcast:chapters url="https://x/499.json" type="application/json+chapters"/>
    </item>
    <item><title>Folge 498</title><guid>ghi</guid></item>
  </channel>
</rss>"#;

    #[test]
    fn takes_podlove_chapters_sorted_and_deduplicated() {
        let feed = parse_feed(CHAPTER_FEED).unwrap();
        assert_eq!(
            feed.episodes[0].chapters,
            vec![
                ParsedChapter {
                    start: 0.0,
                    title: "Begrüßung".into()
                },
                ParsedChapter {
                    start: 750.0,
                    title: "Thema 1".into()
                }
            ]
        );
        assert_eq!(feed.episodes[0].chapters_url, None);
    }

    #[test]
    fn keeps_the_podcasting_2_chapter_url_for_download_time() {
        let feed = parse_feed(CHAPTER_FEED).unwrap();
        assert_eq!(
            feed.episodes[1].chapters_url.as_deref(),
            Some("https://x/499.json")
        );
        assert!(feed.episodes[1].chapters.is_empty());
        assert!(feed.episodes[2].chapters.is_empty());
        assert_eq!(feed.episodes[2].chapters_url, None);
    }
}
