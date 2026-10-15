//! `/library/search` (legacy `library/search`): one query over books, albums, tracks, podcasts
//! and the people behind them.

use std::collections::HashMap;
use std::time::Instant;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::favorites::kind_accent;
use super::format::{encode_component, format_hours};
use super::layout::Shell;
use crate::auth::WebUser;
use crate::error::WebResult;
use crate::library::queries::{
    Item, ProgressEntry, progress_map, search_artists, search_items, search_tracks, total_items,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/library/search", get(page))
}

/// Hits per group, as in legacy.
const LIMIT: i64 = 20;

pub struct Hit {
    pub href: String,
    pub accent: &'static str,
    pub title: String,
    pub subtitle: String,
    pub status: String,
}

pub struct TrackLine {
    pub id: i32,
    pub item_id: i32,
    pub title: String,
    pub item_title: String,
    pub duration: String,
}

pub struct NameLine {
    pub href: String,
    pub accent: &'static str,
    pub name: String,
    pub role: &'static str,
    pub work_count: i32,
}

#[derive(Template)]
#[template(path = "pages/library/search.html")]
struct SearchPage {
    shell: Shell,
    q: String,
    total: i64,
    elapsed_ms: u128,
    total_hits: usize,
    books: Vec<Hit>,
    albums: Vec<Hit>,
    podcasts: Vec<Hit>,
    tracks: Vec<TrackLine>,
    artists: Vec<NameLine>,
}

/// "beendet", "42 % gehört", or nothing for an item never started.
pub fn status(progress: Option<&ProgressEntry>) -> String {
    match progress {
        None => String::new(),
        Some(p) if p.finished => "beendet".into(),
        Some(p) => {
            let percent = if p.duration > 0.0 {
                (p.position / p.duration * 100.0).round() as i64
            } else {
                0
            };
            format!("{percent} % gehört")
        }
    }
}

/// Author and series for a book, the artist for anything else.
pub fn subtitle(item: &Item) -> String {
    if item.kind == "book" {
        [item.author.as_deref(), item.series.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        item.artist.clone().unwrap_or_default()
    }
}

async fn page(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> WebResult<Html<String>> {
    let q = query.get("q").cloned().unwrap_or_default();
    let started = Instant::now();
    let (shell, total) = tokio::try_join!(Shell::load(&state.db, &user, &uri), async {
        Ok(total_items(&state.db).await?)
    })?;
    let mut page = SearchPage {
        shell,
        q,
        total,
        elapsed_ms: 0,
        total_hits: 0,
        books: Vec::new(),
        albums: Vec::new(),
        podcasts: Vec::new(),
        tracks: Vec::new(),
        artists: Vec::new(),
    };
    if page.q.is_empty() {
        return Ok(Html(page.render()?));
    }
    let q = page.q.as_str();
    let (books, albums, podcasts, tracks, artists) = tokio::try_join!(
        search_items(&state.db, q, &["book"], LIMIT),
        search_items(&state.db, q, &["album"], LIMIT),
        search_items(&state.db, q, &["podcast", "episode"], LIMIT),
        search_tracks(&state.db, q, LIMIT),
        search_artists(&state.db, q, LIMIT)
    )?;
    let ids: Vec<i32> = books
        .iter()
        .chain(&albums)
        .chain(&podcasts)
        .map(|i| i.id)
        .collect();
    let progress = progress_map(&state.db, user.id, &ids).await?;
    let hits = |rows: &[Item], segment: &str| -> Vec<Hit> {
        rows.iter()
            .map(|row| Hit {
                href: format!("/library/{segment}/{}", row.id),
                accent: kind_accent(&row.kind),
                title: row.title.clone(),
                subtitle: subtitle(row),
                status: status(progress.get(&row.id)),
            })
            .collect()
    };
    page.books = hits(&books, "books");
    page.albums = hits(&albums, "albums");
    page.podcasts = hits(&podcasts, "podcasts");
    page.tracks = tracks
        .into_iter()
        .map(|t| TrackLine {
            id: t.id,
            item_id: t.item_id,
            title: t.title.unwrap_or_default(),
            item_title: t.item_title,
            duration: format_hours(t.duration),
        })
        .collect();
    page.artists = artists
        .into_iter()
        .map(|a| {
            let author = a.role == "author";
            NameLine {
                // Legacy sent authors to the album filter too, which never lists a book.
                href: if author {
                    format!("/library/books?q={}", encode_component(&a.name))
                } else {
                    format!("/library/albums?artist={}", encode_component(&a.name))
                },
                accent: if author { "book" } else { "music" },
                role: if author { "Autor:in" } else { "Interpret:in" },
                work_count: a.work_count,
                name: a.name,
            }
        })
        .collect();
    page.total_hits =
        page.books.len() + page.albums.len() + page.podcasts.len() + page.tracks.len();
    page.elapsed_ms = started.elapsed().as_millis();
    Ok(Html(page.render()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(position: f64, finished: bool, duration: f64) -> ProgressEntry {
        ProgressEntry {
            position,
            finished,
            duration,
        }
    }

    #[test]
    fn status_reads_like_legacy() {
        assert_eq!(status(None), "");
        assert_eq!(status(Some(&entry(10.0, true, 100.0))), "beendet");
        assert_eq!(status(Some(&entry(42.4, false, 100.0))), "42 % gehört");
        assert_eq!(status(Some(&entry(42.0, false, 0.0))), "0 % gehört");
    }
}
