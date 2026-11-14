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
use crate::library::playback::{ContinueEntry, continue_listening, listening_history};
use crate::library::queries::{Item, items_by_ids, recently_added};
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

/// One tile of "Neu im Bestand" or "Zuletzt gehört".
pub struct RecentTile {
    pub id: i32,
    pub accent: &'static str,
    /// Every kind has a page to go to (legacy linked only books and albums).
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
            // An episode has no page of its own; it is listed on its podcast's.
            href: Some(detail_href(&item.kind, item.parent_id.unwrap_or(item.id))),
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
    /// "Zuletzt gehört" (not in legacy); the section is left out while it is empty.
    heard: Vec<RecentTile>,
    recent: Vec<RecentTile>,
}

/// As many tiles as one row of the start page holds.
const TILES: usize = 6;

/// What the user heard last, each item once, newest first (`GET /me/history` as tiles).
async fn recently_heard(state: &AppState, user_id: i32) -> Result<Vec<RecentTile>, sqlx::Error> {
    // An item may come up several times in the history, so ask for more runs than tiles.
    let runs = listening_history(&state.db, user_id, 4 * TILES as i64).await?;
    let mut ids: Vec<i32> = Vec::new();
    for run in &runs {
        if !ids.contains(&run.item_id) {
            ids.push(run.item_id);
        }
    }
    ids.truncate(TILES);
    let items = items_by_ids(&state.db, &ids).await?;
    Ok(ids
        .iter()
        .filter_map(|id| items.iter().find(|item| item.id == *id))
        .map(RecentTile::from)
        .collect())
}

/// The start page: greeting, the last three unfinished items, what was heard last and the six
/// newest.
async fn library(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    let (shell, entries, heard, recent) = tokio::try_join!(
        Shell::load(&state, &user, &uri),
        async { Ok(continue_listening(&state.db, user.id, 3).await?) },
        async { Ok(recently_heard(&state, user.id).await?) },
        async { Ok(recently_added(&state.db, TILES as i64).await?) },
    )?;
    let page = LibraryPage {
        greeting: greeting(chrono::Local::now().hour()),
        name: capitalize(&shell.user_name),
        shell,
        continue_entries: entries.iter().map(ContinueCard::from).collect(),
        heard,
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
        assert_eq!(capitalize("oskar"), "Oskar");
        assert_eq!(capitalize("ärger"), "Ärger");
        assert_eq!(capitalize(""), "");
    }
}
