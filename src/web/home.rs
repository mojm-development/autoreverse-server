//! `/` and the start page `/library`.

use askama::Template;
use axum::extract::State;
use axum::http::Uri;
use axum::response::{Html, Redirect};
use axum::{Router, routing::get};
use chrono::Timelike;

use super::layout::Shell;
use crate::auth::{Session, WebUser};
use crate::error::WebResult;
use crate::library::playback::{ContinueEntry, continue_listening};
use crate::library::queries::{Item, recently_added};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(root))
        .route("/library", get(library))
}

async fn root(session: Session) -> Redirect {
    Redirect::to(if session.user_id.is_some() {
        "/library"
    } else {
        "/login"
    })
}

/// One "Weiter hören" card (legacy `ContinueCard.svelte`).
pub struct ContinueCard {
    pub id: i32,
    pub kind: String,
    pub accent: &'static str,
    pub href: String,
    pub has_cover: bool,
    pub title: String,
    pub subtitle: String,
    /// Width of the progress bar, `0`–`100` with three decimals.
    pub percent: String,
    pub duration: String,
}

impl From<&ContinueEntry> for ContinueCard {
    fn from(e: &ContinueEntry) -> Self {
        let percent = if e.duration > 0.0 {
            (e.position / e.duration * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let subtitle = if e.kind == "book" {
            &e.author
        } else {
            &e.artist
        };
        ContinueCard {
            id: e.id,
            kind: e.kind.clone(),
            accent: accent(&e.kind),
            href: detail_href(&e.kind, e.id),
            has_cover: e.cover_path.is_some(),
            title: e.title.clone(),
            subtitle: subtitle.clone().unwrap_or_default(),
            percent: format!("{percent:.3}"),
            duration: format_duration(e.duration),
        }
    }
}

/// One "Neu im Bestand" tile.
pub struct RecentTile {
    pub id: i32,
    pub accent: &'static str,
    /// Books and albums have a page to go to; podcasts get theirs in a later slice.
    pub href: Option<String>,
    pub has_cover: bool,
    pub title: String,
    pub subtitle: String,
}

impl From<&Item> for RecentTile {
    fn from(item: &Item) -> Self {
        RecentTile {
            id: item.id,
            accent: accent(&item.kind),
            href: matches!(item.kind.as_str(), "book" | "album")
                .then(|| detail_href(&item.kind, item.id)),
            has_cover: item.cover_path.is_some(),
            title: item.title.clone(),
            subtitle: item
                .artist
                .clone()
                .or_else(|| item.author.clone())
                .unwrap_or_default(),
        }
    }
}

#[derive(Template)]
#[template(path = "pages/library.html")]
struct LibraryPage {
    shell: Shell,
    greeting: &'static str,
    name: String,
    continue_entries: Vec<ContinueCard>,
    recent: Vec<RecentTile>,
}

/// The start page: greeting, the last three unfinished items and the six newest.
async fn library(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    let (shell, entries, recent) = tokio::try_join!(
        Shell::load(&state.db, &user, &uri),
        async { Ok(continue_listening(&state.db, user.id, 3).await?) },
        async { Ok(recently_added(&state.db, 6).await?) },
    )?;
    let page = LibraryPage {
        greeting: greeting(chrono::Local::now().hour()),
        name: capitalize(&shell.user_name),
        shell,
        continue_entries: entries.iter().map(ContinueCard::from).collect(),
        recent: recent.iter().map(RecentTile::from).collect(),
    };
    Ok(Html(page.render()?))
}

/// The accent colour token of an item kind.
pub fn accent(kind: &str) -> &'static str {
    match kind {
        "book" => "book",
        "album" => "music",
        _ => "podcast",
    }
}

/// Where a card leads (legacy `detailHref`).
fn detail_href(kind: &str, id: i32) -> String {
    let segment = match kind {
        "book" => "books",
        "album" => "albums",
        _ => "podcasts",
    };
    format!("/library/{segment}/{id}")
}

/// "1.5 h" from an hour on, "48 min" below (the start page's own formatter).
fn format_duration(seconds: f64) -> String {
    let hours = seconds / 3600.0;
    if hours >= 1.0 {
        format!("{hours:.1} h")
    } else {
        format!("{} min", (seconds / 60.0).round() as i64)
    }
}

fn greeting(hour: u32) -> &'static str {
    match hour {
        0..12 => "Guten Morgen",
        12..18 => "Guten Tag",
        _ => "Guten Abend",
    }
}

fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_by_hour() {
        assert_eq!(greeting(0), "Guten Morgen");
        assert_eq!(greeting(11), "Guten Morgen");
        assert_eq!(greeting(12), "Guten Tag");
        assert_eq!(greeting(17), "Guten Tag");
        assert_eq!(greeting(18), "Guten Abend");
    }

    #[test]
    fn durations_and_links_like_the_start_page() {
        assert_eq!(format_duration(5400.0), "1.5 h");
        assert_eq!(format_duration(3600.0), "1.0 h");
        assert_eq!(format_duration(1500.0), "25 min");
        assert_eq!(format_duration(0.0), "0 min");
        assert_eq!(detail_href("book", 3), "/library/books/3");
        assert_eq!(detail_href("album", 4), "/library/albums/4");
        assert_eq!(detail_href("episode", 5), "/library/podcasts/5");
    }

    #[test]
    fn capitalizes_the_first_letter_only() {
        assert_eq!(capitalize("oliver"), "Oliver");
        assert_eq!(capitalize("ärger"), "Ärger");
        assert_eq!(capitalize(""), "");
    }
}
