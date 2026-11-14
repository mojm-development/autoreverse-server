//! A station's logo from its own web page, for when the directory has none to offer.
//!
//! The directory's `favicon` field is filled in by volunteers: many stations have none, and of
//! those that have one a good part points at a file that is long gone. Nearly every entry names
//! a homepage, though, and a homepage names its own icons. So where the directory's logo is
//! missing or cannot be had, the page is fetched and its icon links are tried in the order of
//! how well they do as a logo: an `apple-touch-icon` (square, 180 pixels or so), then
//! `rel="icon"` by its declared size, then the two addresses browsers try unasked
//! (`/apple-touch-icon.png`, `/favicon.ico`), then tiny icons, and `og:image` last — that one
//! is often a photo of the studio rather than a logo.
//!
//! The page and everything it names are somebody else's word, exactly like a stream address:
//! each request goes through `probe::client_for`, so a page cannot point the server at the
//! local network, neither by a link nor by a redirect. The work per page is bounded in
//! requests, bytes and time; what the pictures must be is decided by `logos::normalize` as for
//! every other logo (no SVG).
//!
//! The HTML is read by a small tag reader of its own rather than a parser: all that is wanted
//! are the attributes of `<link>`, `<meta>` and `<base>`, wherever in a broken page they stand,
//! and nothing else in the project reads HTML (feeds are XML and go through `feed-rs`).

use std::time::{Duration, Instant};

use reqwest::Url;
use reqwest::header::CONTENT_TYPE;

use super::logos::{self, Picture};
use super::probe::{self, USER_AGENT};
use crate::state::AppState;

/// The icon links are in the head; a page whose head is longer than this is not read further.
pub const MAX_PAGE_BYTES: usize = 512 * 1024;
/// Requests per page, the page itself included.
pub const MAX_REQUESTS: usize = 5;
/// A picture with a shorter edge than this is a browser-tab icon; blown up to a tile it is a
/// blur, so it is only taken when nothing else can be had.
pub const MIN_EDGE: u32 = 32;
/// What an icon link without `sizes` is taken to be when the links are put in order: an
/// `apple-touch-icon` is 180 pixels unless it says otherwise, a plain icon is usually an
/// `.ico` holding up to 48.
const UNDECLARED_TOUCH: u32 = 180;
const UNDECLARED_ICON: u32 = 48;

/// A start tag: its attributes with their names in lower case, in the order they stand.
struct Tag {
    name: String,
    attributes: Vec<(String, String)>,
}

impl Tag {
    /// The first attribute of this name, as browsers have it.
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.trim())
    }
}

/// The handful of entities that turn up in addresses; `&amp;` between query parameters is the
/// one that matters.
fn decoded(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#38;", "&")
        .replace("&#x26;", "&")
        .replace("&amp;", "&")
}

/// The attributes from `from` up to the end of the tag, and where the tag ends. Tolerant the
/// way browsers are: quoted or bare values, attributes without one, stray slashes, a tag that
/// never closes.
fn attributes(html: &str, from: usize) -> (Vec<(String, String)>, usize) {
    let bytes = html.as_bytes();
    let mut found = Vec::new();
    let mut at = from;
    loop {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        if at >= bytes.len() {
            return (found, bytes.len());
        }
        if bytes[at] == b'>' {
            return (found, at + 1);
        }
        let start = at;
        while at < bytes.len()
            && !bytes[at].is_ascii_whitespace()
            && !matches!(bytes[at], b'=' | b'>' | b'/')
        {
            at += 1;
        }
        if at == start {
            // A stray `=`.
            at += 1;
            continue;
        }
        // Every index used to cut is that of an ASCII byte, so the cuts are on characters.
        let name = html[start..at].to_ascii_lowercase();
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = "";
        if bytes.get(at) == Some(&b'=') {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            match bytes.get(at) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let start = at + 1;
                    let end = bytes[start..]
                        .iter()
                        .position(|byte| *byte == quote)
                        .map_or(bytes.len(), |length| start + length);
                    value = &html[start..end];
                    at = (end + 1).min(bytes.len());
                }
                _ => {
                    let start = at;
                    while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>'
                    {
                        at += 1;
                    }
                    value = &html[start..at];
                }
            }
        }
        found.push((name, decoded(value)));
    }
}

/// Where the element `name`, whose start tag ended at `from`, is closed.
fn closing(html: &str, from: usize, name: &str) -> usize {
    let bytes = html.as_bytes();
    let mut at = from;
    while let Some(found) = html[at..].find("</") {
        at += found + 2;
        let end = at + name.len();
        if bytes
            .get(at..end)
            .is_some_and(|tag| tag.eq_ignore_ascii_case(name.as_bytes()))
        {
            return end;
        }
    }
    bytes.len()
}

/// The `<link>`, `<meta>` and `<base>` tags of a page, wherever they stand. Comments, scripts
/// and styles are stepped over: a script that writes a `<link>` into the page is no link.
fn tags(html: &str) -> Vec<Tag> {
    let bytes = html.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(open) = html[at..].find('<') {
        at += open + 1;
        if html[at..].starts_with("!--") {
            at = html[at..]
                .find("-->")
                .map_or(bytes.len(), |end| at + end + 3);
            continue;
        }
        let name_end = at
            + bytes[at..]
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric())
                .count();
        if name_end == at {
            // An end tag, a doctype, or a `<` in the text.
            continue;
        }
        let name = html[at..name_end].to_ascii_lowercase();
        let (attributes, end) = attributes(html, name_end);
        at = end;
        match name.as_str() {
            "script" | "style" => at = closing(html, at, &name),
            "link" | "meta" | "base" => found.push(Tag { name, attributes }),
            _ => {}
        }
    }
    found
}

/// `href` as an `http(s)` address, relative ones resolved against `base`.
fn resolved(base: &Url, href: &str) -> Option<Url> {
    if href.is_empty() {
        return None;
    }
    let mut url = base.join(href).ok()?;
    url.set_fragment(None);
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()).then_some(url)
}

/// The edge a `sizes` attribute declares: the largest of its entries, each by its shorter side.
/// `None` without one, and for `any` (which goes with vector icons).
fn declared_edge(sizes: Option<&str>) -> Option<u32> {
    sizes?
        .split_ascii_whitespace()
        .filter_map(|size| {
            let (width, height) = size.split_once(['x', 'X'])?;
            Some(width.parse::<u32>().ok()?.min(height.parse().ok()?))
        })
        .max()
}

/// Whether a link names a vector icon: those are never taken (`logos`), so asking for one
/// would only use up a request.
fn is_svg(tag: &Tag, url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    tag.attribute("type")
        .is_some_and(|kind| kind.to_ascii_lowercase().contains("svg"))
        || path.ends_with(".svg")
        || path.ends_with(".svgz")
}

/// The addresses to try for the logo of the page `html` that was served from `page`, the most
/// promising first (see the module text for the order). A page that says nothing — or no page
/// at all, `html` empty — still yields the two addresses browsers try unasked.
pub fn candidates(html: &str, page: &Url) -> Vec<Url> {
    let tags = tags(html);
    // Relative addresses count from `<base href>` where the page has one (the first, as in a
    // browser), else from where the page really came from, after every redirect.
    let base = tags
        .iter()
        .filter(|tag| tag.name == "base")
        .find_map(|tag| resolved(page, tag.attribute("href")?))
        .unwrap_or_else(|| page.clone());

    let mut touch: Vec<(u32, Url)> = Vec::new();
    let mut icons: Vec<(u32, Url)> = Vec::new();
    let mut social: Vec<Url> = Vec::new();
    for tag in &tags {
        match tag.name.as_str() {
            "link" => {
                let rel = tag.attribute("rel").unwrap_or("").to_ascii_lowercase();
                let is =
                    |wanted: &[&str]| rel.split_ascii_whitespace().any(|r| wanted.contains(&r));
                let is_touch = is(&["apple-touch-icon", "apple-touch-icon-precomposed"]);
                // `shortcut icon` is the token `icon` with an old word before it; `mask-icon`
                // and the like are tokens of their own and not meant.
                if !is_touch && !is(&["icon"]) {
                    continue;
                }
                let Some(url) = resolved(&base, tag.attribute("href").unwrap_or("")) else {
                    continue;
                };
                if is_svg(tag, &url) {
                    continue;
                }
                let edge = declared_edge(tag.attribute("sizes"));
                if is_touch {
                    touch.push((edge.unwrap_or(UNDECLARED_TOUCH), url));
                } else {
                    icons.push((edge.unwrap_or(UNDECLARED_ICON), url));
                }
            }
            "meta" => {
                let key = tag.attribute("property").or_else(|| tag.attribute("name"));
                let is_image = key.is_some_and(|key| {
                    ["og:image", "og:image:url", "og:image:secure_url"]
                        .iter()
                        .any(|wanted| key.eq_ignore_ascii_case(wanted))
                });
                if is_image
                    && let Some(url) = resolved(&base, tag.attribute("content").unwrap_or(""))
                {
                    social.push(url);
                }
            }
            _ => {}
        }
    }
    // Stable sorts: among equals the page's own order stands.
    touch.sort_by_key(|(edge, _)| std::cmp::Reverse(*edge));
    icons.sort_by_key(|(edge, _)| std::cmp::Reverse(*edge));
    let (icons, tiny): (Vec<_>, Vec<_>) =
        icons.into_iter().partition(|(edge, _)| *edge >= MIN_EDGE);
    // From the root of the page's host, whatever `<base>` says: that is where browsers look.
    let unasked = ["/apple-touch-icon.png", "/favicon.ico"]
        .into_iter()
        .filter_map(|path| page.join(path).ok());

    let mut ordered: Vec<Url> = Vec::new();
    let all = touch
        .into_iter()
        .chain(icons)
        .map(|(_, url)| url)
        .chain(unasked)
        .chain(tiny.into_iter().map(|(_, url)| url))
        .chain(social);
    for url in all {
        if !ordered.contains(&url) {
            ordered.push(url);
        }
    }
    ordered
}

/// The page at `url`: where it was really served from, and its HTML — empty when the server
/// answered with an error or with something that is no page, which still leaves the addresses
/// browsers try unasked. `None` when nothing answered: then nothing else there will either.
async fn page(state: &AppState, url: Url, timeout: Duration) -> Option<(Url, String)> {
    let client = probe::client_for(state, &url).ok()?;
    let mut response = client
        .get(url)
        // The whole answer: a page that trickles is read as far as it got.
        .timeout(timeout)
        .header("user-agent", USER_AGENT)
        .header("accept", "text/html,application/xhtml+xml")
        .send()
        .await
        .ok()?;
    let served_from = response.url().clone();
    // A homepage field that names the stream itself must not be read by the megabyte.
    let is_page = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|kind| {
            let kind = kind.trim().to_ascii_lowercase();
            kind.starts_with("text/html") || kind.starts_with("application/xhtml+xml")
        });
    if !response.status().is_success() || !is_page {
        return Some((served_from, String::new()));
    }
    let mut body = Vec::new();
    while body.len() < MAX_PAGE_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            _ => break,
        }
    }
    body.truncate(MAX_PAGE_BYTES);
    // Tags and addresses are ASCII in every encoding in use; what else is in there is not read.
    Some((served_from, String::from_utf8_lossy(&body).into_owned()))
}

/// The logo the page at `homepage` offers: the first of its [`candidates`] that is a picture
/// (`logos::normalize`) of at least [`MIN_EDGE`] pixels, else the largest smaller one. At most
/// [`MAX_REQUESTS`] requests, `each` no longer than that, all within `total`.
pub async fn fetch(
    state: &AppState,
    homepage: &str,
    each: Duration,
    total: Duration,
) -> Option<Picture> {
    let started = Instant::now();
    let left = || {
        total
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
            .map(|left| left.min(each))
    };
    let url = probe::parse_url(homepage).ok()?;
    let (served_from, html) = page(state, url, left()?).await?;
    let html = tokio::task::spawn_blocking(move || candidates(&html, &served_from));
    let candidates = html.await.ok()?;

    let mut small: Option<(u32, Picture)> = None;
    for candidate in candidates.iter().take(MAX_REQUESTS - 1) {
        let Some(timeout) = left() else { break };
        let Some(picture) = probe::fetch_logo(state, candidate.as_str(), timeout).await else {
            continue;
        };
        let edge = logos::shorter_edge(&picture);
        if edge >= MIN_EDGE {
            return Some(picture);
        }
        if small.as_ref().is_none_or(|(kept, _)| edge > *kept) {
            small = Some((edge, picture));
        }
    }
    small.map(|(_, picture)| picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(html: &str, page: &str) -> Vec<String> {
        candidates(html, &Url::parse(page).unwrap())
            .into_iter()
            .map(String::from)
            .collect()
    }

    const UNASKED: [&str; 2] = [
        "https://radio.example/apple-touch-icon.png",
        "https://radio.example/favicon.ico",
    ];

    #[test]
    fn the_touch_icon_comes_before_icons_and_the_social_picture_last() {
        let html = r#"<!doctype html><html><head>
            <meta property="og:image" content="https://cdn.example/studio.jpg">
            <link rel="icon" href="/icon.png">
            <link rel="apple-touch-icon" href="/touch.png">
            </head><body></body></html>"#;
        assert_eq!(
            found(html, "https://radio.example/"),
            [
                "https://radio.example/touch.png",
                "https://radio.example/icon.png",
                UNASKED[0],
                UNASKED[1],
                "https://cdn.example/studio.jpg",
            ]
        );
    }

    #[test]
    fn larger_declared_sizes_come_first_and_tiny_ones_after_the_unasked() {
        let html = r#"
            <link rel="icon" sizes="16x16" href="/16.png">
            <link rel="icon" type="image/png" sizes="32x32" href="/32.png">
            <link rel="shortcut icon" href="/plain.ico">
            <link rel="icon" sizes="192x192" href="/192.png">
            <link rel="ICON" sizes="16x16 64X64 48x48" href="/multi.ico">
            <link rel="apple-touch-icon-precomposed" sizes="120x120" href="/t120.png">
            <link rel="apple-touch-icon" href="/t.png">
            <link rel="apple-touch-icon" sizes="152x152" href="/t152.png">"#;
        let paths: Vec<String> = found(html, "https://radio.example/")
            .into_iter()
            .map(|url| url.trim_start_matches("https://radio.example").to_owned())
            .collect();
        assert_eq!(
            paths,
            [
                // Without `sizes` a touch icon is 180 pixels, a plain icon 48.
                "/t.png",
                "/t152.png",
                "/t120.png",
                "/192.png",
                "/multi.ico",
                "/plain.ico",
                "/32.png",
                "/apple-touch-icon.png",
                "/favicon.ico",
                "/16.png",
            ]
        );
    }

    #[test]
    fn relative_addresses_count_from_where_the_page_was_served() {
        let html = r#"<link rel="apple-touch-icon" href="img/touch.png?v=2&amp;x=1#top">
            <link rel="icon" href="../icon.png">
            <link rel="icon" sizes="96x96" href="//static.example/96.png">
            <meta name="og:image" content="/social.jpg">"#;
        assert_eq!(
            found(html, "http://radio.example:8080/de/start/index.html"),
            [
                "http://radio.example:8080/de/start/img/touch.png?v=2&x=1",
                "http://static.example/96.png",
                "http://radio.example:8080/de/icon.png",
                "http://radio.example:8080/apple-touch-icon.png",
                "http://radio.example:8080/favicon.ico",
                "http://radio.example:8080/social.jpg",
            ]
        );
    }

    #[test]
    fn a_base_href_moves_the_links_but_not_the_unasked_addresses() {
        let html = r#"<head><base target="_blank"><base href="https://cdn.example/assets/">
            <base href="https://other.example/">
            <link rel="icon" href="icon.png"><link rel="apple-touch-icon" href="/touch.png">"#;
        assert_eq!(
            found(html, "https://radio.example/start"),
            [
                // A path from the root is one from the root of the base's host.
                "https://cdn.example/touch.png",
                "https://cdn.example/assets/icon.png",
                UNASKED[0],
                UNASKED[1],
            ]
        );
        // A relative base counts from the page.
        let html = r#"<base href="/static/"><link rel="icon" href="icon.png">"#;
        assert_eq!(
            found(html, "https://radio.example/a/b")[0],
            "https://radio.example/static/icon.png"
        );
    }

    #[test]
    fn broken_html_is_read_as_far_as_it_makes_sense() {
        let html = "<html><head><title>Radio</title\n\
            <!-- <link rel=\"apple-touch-icon\" href=\"/commented.png\"> -->\n\
            <script>document.write('<link rel=\"icon\" href=\"/written.png\">');</SCRIPT>\n\
            <style>a::before { content: '<link rel=\"icon\" href=\"/styled.png\">' }</style>\n\
            <LINK REL=icon HREF=/bare.png>\n\
            <link rel = 'apple-touch-icon'   href = '/single.png' / >\n\
            <link href=\"/first.png\" href=\"/second.png\" rel=\"icon\" sizes=\"64x64\" crossorigin>\n\
            <link rel=\"stylesheet\" href=\"/style.css\"><link rel=\"mask-icon\" href=\"/mask.png\">\n\
            <link rel=\"icon\" href=\"\"><link rel=\"icon\"><link = rel=icon>\n\
            <meta property=\"og:image\"><meta content=\"/nothing.jpg\">\n\
            < link rel=\"icon\" href=\"/spaced.png\">\n\
            <p>1 < 2 <b>fett\n\
            <link rel=\"icon\" href=\"/in-body.png\" sizes=\"gross\">\n\
            <link rel=\"icon\" href=\"/unclosed.png";
        let paths: Vec<String> = found(html, "https://radio.example/")
            .into_iter()
            .map(|url| url.trim_start_matches("https://radio.example").to_owned())
            .collect();
        assert_eq!(
            paths,
            [
                "/single.png",
                "/first.png",
                "/bare.png",
                "/in-body.png",
                "/unclosed.png",
                "/apple-touch-icon.png",
                "/favicon.ico",
            ]
        );
    }

    #[test]
    fn a_page_that_says_nothing_leaves_the_addresses_browsers_try_unasked() {
        for html in [
            "",
            "<html><head><title>Radio</title></head></html>",
            "\u{0}\u{1}<<<>>>&&& \"'",
            "<",
            "<link",
            "<link rel=\"icon\" href=\"",
            "Schön, daß Sie da sind — <ÿ> <é",
        ] {
            assert_eq!(found(html, "https://radio.example/de/"), UNASKED, "{html}");
        }
    }

    #[test]
    fn what_cannot_be_a_logo_is_not_asked_for() {
        let html = r#"<link rel="icon" type="image/svg+xml" href="/icon">
            <link rel="icon" href="/logo.SVG" sizes="any">
            <link rel="apple-touch-icon" href="data:image/png;base64,iVBORw0KGgo=">
            <link rel="icon" href="javascript:alert(1)">
            <link rel="icon" href="file:///etc/passwd">
            <link rel="icon" href="ftp://radio.example/icon.png">
            <meta property="og:image" content="mailto:radio@radio.example">
            <link rel="icon" href="/favicon.ico"><link rel="icon" href="/favicon.ico?v=3">"#;
        assert_eq!(
            found(html, "https://radio.example/"),
            [
                // Named by the page and tried unasked: once.
                UNASKED[1],
                "https://radio.example/favicon.ico?v=3",
                UNASKED[0],
            ]
        );
    }

    #[test]
    fn an_address_in_the_local_network_is_a_candidate_like_any_other() {
        // Reading the page decides nothing about reach: that is `probe::client_for`'s job with
        // every single request, and the integration tests hold it to that.
        let html = r#"<link rel="apple-touch-icon" href="http://192.168.1.1/admin.png">"#;
        assert_eq!(
            found(html, "https://radio.example/")[0],
            "http://192.168.1.1/admin.png"
        );
    }
}
