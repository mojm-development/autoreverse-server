//! `/library/books` and `/library/books/{id}` (legacy `library/books/**`). Playing is wired up by
//! static/js/player through `data-action` buttons, editing (Verwalter) by static/js/metadata.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::format::{chapter_labels, format_hms, format_hours, human_duration, volume};
use super::layout::Shell;
use super::metadata::{BulkEditor, MetadataEditor};
use super::playlists::{PlaylistChoice, playlist_choices};
use crate::auth::WebUser;
use crate::error::{WebError, WebResult};
use crate::library::bookmarks::bookmarks_for_item;
use crate::library::favorites::is_item_favorite;
use crate::library::queries::{
    self, Item, ItemsFilter, ProgressEntry, SortKey, chapters, count_items, item_durations,
    progress, progress_map, series_siblings, tracks,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/books", get(list))
        .route("/library/books/{id}", get(detail))
}

/// How far a user got with a book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Listening,
    Unstarted,
    Finished,
}

impl Status {
    fn of(progress: Option<&ProgressEntry>) -> Status {
        match progress {
            None => Status::Unstarted,
            Some(p) if p.finished => Status::Finished,
            Some(_) => Status::Listening,
        }
    }

    /// The tile's play button: legacy said "fortsetzen" for a book being listened to.
    pub fn play_verb(self) -> &'static str {
        if self == Status::Listening {
            "fortsetzen"
        } else {
            "abspielen"
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Status::Listening => "listening",
            Status::Unstarted => "unstarted",
            Status::Finished => "finished",
        }
    }
}

/// One book in the grid or the list.
pub struct BookRow {
    pub id: i32,
    pub title: String,
    pub author: String,
    pub has_cover: bool,
    pub status: Status,
    /// 0–100.
    pub percent: u32,
    /// "noch 3 Std 12 min" while listening, else empty.
    pub remaining: String,
    /// "Perry Rhodan · Band 3", or empty outside a series.
    pub band: String,
    pub duration: String,
}

impl BookRow {
    fn new(book: &Item, duration: f64, progress: Option<&ProgressEntry>) -> BookRow {
        let status = Status::of(progress);
        let percent = match progress {
            Some(_) if status == Status::Finished && duration > 0.0 => 100,
            Some(p) if duration > 0.0 => ((p.position / duration) * 100.0).round().max(0.0) as u32,
            _ => 0,
        };
        let remaining = match progress {
            Some(p) if status == Status::Listening && duration > 0.0 => {
                format!("noch {}", human_duration((duration - p.position).max(0.0)))
            }
            _ => String::new(),
        };
        let band = match (&book.series, book.series_index) {
            (Some(series), Some(index)) if index != 0.0 => {
                format!("{series} · Band {}", volume(index))
            }
            (Some(series), _) => series.clone(),
            _ => String::new(),
        };
        let duration_label = human_duration(duration);
        BookRow {
            id: book.id,
            title: book.title.clone(),
            author: book.author.clone().unwrap_or_default(),
            has_cover: book.cover_path.is_some(),
            status,
            percent,
            remaining,
            band,
            duration: if duration_label.is_empty() {
                "—".into()
            } else {
                duration_label
            },
        }
    }
}

pub struct Link {
    pub label: String,
    pub href: String,
    pub current: bool,
}

pub struct Filter {
    pub key: &'static str,
    pub label: &'static str,
    pub count: usize,
}

#[derive(Template)]
#[template(path = "pages/library/books.html")]
struct BooksPage {
    shell: Shell,
    view: &'static str,
    q: String,
    series: Option<String>,
    series_author: Option<String>,
    sort: &'static str,
    total: i64,
    books: Vec<BookRow>,
    sortable: bool,
    sort_options: Vec<Link>,
    grid_href: String,
    list_href: String,
    clear_q_href: String,
    clear_series_href: String,
    filters: Vec<Filter>,
    empty: String,
    /// Verwalter pick tiles for the bulk editor.
    pickable: bool,
    bulk: Option<BulkEditor>,
}

/// The current list's parameters, so a link can change one and keep the rest.
struct ListParams {
    q: String,
    series: Option<String>,
    author: Option<String>,
    sort: SortKey,
    view: &'static str,
}

impl ListParams {
    fn href(&self, overrides: &[(&str, &str)]) -> String {
        let mut params: Vec<(&str, String)> = Vec::new();
        if !self.q.is_empty() {
            params.push(("q", self.q.clone()));
        }
        if let Some(series) = &self.series {
            params.push(("series", series.clone()));
        }
        // The author scopes the series: two writers can use the same series name.
        if let Some(author) = &self.author {
            params.push(("author", author.clone()));
        }
        if self.sort != SortKey::Series {
            params.push(("sort", self.sort.key().into()));
        }
        if self.view != "grid" {
            params.push(("view", self.view.into()));
        }
        // `URLSearchParams.set`: a present key keeps its place, a new one goes last.
        for (key, value) in overrides {
            if value.is_empty() {
                params.retain(|(k, _)| k != key);
            } else if let Some(entry) = params.iter_mut().find(|(k, _)| k == key) {
                entry.1 = (*value).to_owned();
            } else {
                params.push((key, (*value).to_owned()));
            }
        }
        let query = serde_urlencoded::to_string(&params).unwrap_or_default();
        if query.is_empty() {
            "/library/books".into()
        } else {
            format!("/library/books?{query}")
        }
    }
}

async fn list(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> WebResult<Html<String>> {
    let shell = Shell::load(&state.db, &user, &uri).await?;
    let get = |key: &str| query.get(key).filter(|v| !v.is_empty()).cloned();
    let params = ListParams {
        q: get("q").unwrap_or_default(),
        series: get("series"),
        author: get("author"),
        sort: get("sort")
            .and_then(|s| SortKey::parse(&s))
            .unwrap_or(SortKey::Series),
        view: if get("view").as_deref() == Some("list") {
            "list"
        } else {
            "grid"
        },
    };

    let (rows, total) = match &params.series {
        Some(series) => {
            let rows = series_siblings(&state.db, series, params.author.as_deref()).await?;
            let total = rows.len() as i64;
            (rows, total)
        }
        None => {
            let filter = ItemsFilter {
                kind: Some("book".into()),
                q: (!params.q.is_empty()).then(|| params.q.clone()),
                missing: Some(false),
                sort: Some(params.sort),
                limit: 200,
                offset: 0,
                played_by: Some(user.id),
                ..Default::default()
            };
            tokio::try_join!(
                queries::items(&state.db, &filter),
                // The whole shelf, not the search hits: the heading counts the library.
                count_items(&state.db, "book", None, None)
            )?
        }
    };
    let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let (durations, progress) = tokio::try_join!(
        item_durations(&state.db, &ids),
        progress_map(&state.db, user.id, &ids)
    )?;
    let books: Vec<BookRow> = rows
        .iter()
        .map(|book| {
            BookRow::new(
                book,
                durations.get(&book.id).copied().unwrap_or(0.0),
                progress.get(&book.id),
            )
        })
        .collect();

    let count = |status: Status| books.iter().filter(|b| b.status == status).count();
    let filters = vec![
        Filter {
            key: "all",
            label: "Alle",
            count: books.len(),
        },
        Filter {
            key: "listening",
            label: "Wird gehört",
            count: count(Status::Listening),
        },
        Filter {
            key: "unstarted",
            label: "Nicht begonnen",
            count: count(Status::Unstarted),
        },
        Filter {
            key: "finished",
            label: "Beendet",
            count: count(Status::Finished),
        },
    ];
    let empty = if !params.q.is_empty() {
        format!("Nichts gefunden für „{}“.", params.q)
    } else {
        "Noch keine Hörbücher in der Bibliothek.".into()
    };
    let sort_options = SortKey::BOOK_SORTS
        .iter()
        .map(|key| Link {
            label: key.label().into(),
            href: params.href(&[("sort", key.key())]),
            current: *key == params.sort,
        })
        .collect();

    let pickable = shell.is_admin;
    let bulk = if pickable {
        Some(
            BulkEditor::new("book", total, None)
                .with_last(&state.db, user.id)
                .await?,
        )
    } else {
        None
    };
    let page = BooksPage {
        bulk,
        pickable,
        shell,
        view: params.view,
        sort: params.sort.key(),
        total,
        books,
        sortable: params.series.is_none(),
        sort_options,
        grid_href: params.href(&[("view", "grid")]),
        list_href: params.href(&[("view", "list")]),
        clear_q_href: params.href(&[("q", "")]),
        clear_series_href: params.href(&[("series", ""), ("author", "")]),
        filters,
        empty,
        q: params.q.clone(),
        series: params.series.clone(),
        series_author: params.author.clone(),
    };
    Ok(Html(page.render()?))
}

/// One chapter row (legacy `ChapterList.svelte`).
pub struct ChapterRow {
    pub number: usize,
    pub title: String,
    pub label: String,
    /// "gehört" once the saved position is past it.
    pub state: &'static str,
    pub start: String,
    /// Seconds, for the player script.
    pub start_secs: f64,
    pub end_secs: f64,
}

impl ChapterRow {
    /// The rows of a chapter list, with "gehört" up to `position`.
    pub fn list(rows: &[queries::Chapter], item_title: &str, position: f64) -> Vec<ChapterRow> {
        let titles: Vec<String> = rows.iter().map(|c| c.title.clone()).collect();
        let labels = chapter_labels(&titles, Some(item_title));
        rows.iter()
            .enumerate()
            .map(|(i, c)| ChapterRow {
                number: i + 1,
                title: c.title.clone(),
                label: labels.get(i).cloned().unwrap_or_else(|| c.title.clone()),
                state: if position >= c.end { "gehört" } else { "" },
                start: format_hms(c.start),
                start_secs: c.start,
                end_secs: c.end,
            })
            .collect()
    }
}

pub struct BookmarkRow {
    pub title: String,
    pub position: String,
    pub position_secs: f64,
}

pub struct SeriesRow {
    pub id: i32,
    pub title: String,
}

#[derive(Template)]
#[template(path = "pages/library/book.html")]
struct BookPage {
    shell: Shell,
    book: Item,
    subline: String,
    chapters: Vec<ChapterRow>,
    chapter_count: usize,
    /// "Weiter ab 1:02:03" with saved progress, else "Abspielen".
    play_label: String,
    total: f64,
    current_chapter: String,
    progress_meta: String,
    percent: u32,
    /// Chapter starts as percentages of the whole book.
    ticks: Vec<String>,
    bookmarks: Vec<BookmarkRow>,
    series_books: Vec<SeriesRow>,
    favorite: bool,
    playlists: Vec<PlaylistChoice>,
    /// The saved position: where "Lesezeichen setzen" marks while the book is not playing.
    bookmark_at: f64,
    /// For Verwalter.
    editor: Option<MetadataEditor>,
}

async fn detail(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Path(raw_id): Path<String>,
) -> WebResult<Html<String>> {
    let not_found = || WebError::not_found("Unbekanntes Hörbuch");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let book = queries::item(&state.db, id)
        .await?
        .filter(|b| b.kind == "book")
        .ok_or_else(not_found)?;
    let series_books = async {
        match &book.series {
            Some(series) => series_siblings(&state.db, series, None).await,
            None => Ok(Vec::new()),
        }
    };
    let (
        shell,
        track_rows,
        chapter_rows,
        progress_row,
        bookmark_rows,
        series_rows,
        favorite,
        playlists,
    ) = tokio::try_join!(
        async { Shell::load(&state.db, &user, &uri).await },
        async { Ok(tracks(&state.db, book.id).await?) },
        async { Ok(chapters(&state.db, book.id).await?) },
        async { Ok(progress(&state.db, user.id, book.id).await?) },
        async { Ok(bookmarks_for_item(&state.db, user.id, book.id).await?) },
        async { Ok::<_, WebError>(series_books.await?) },
        async { Ok(is_item_favorite(&state.db, user.id, book.id).await?) },
        playlist_choices(&state.db, user.id),
    )?;

    let total: f64 = track_rows.iter().map(|t| t.duration).sum();
    let position = progress_row.map_or(0.0, |p| p.position);
    let current = chapter_rows
        .iter()
        .find(|c| position >= c.start && position < c.end)
        .or(chapter_rows.last());
    let percent = if total > 0.0 {
        ((position / total) * 100.0).round().clamp(0.0, 100.0) as u32
    } else {
        0
    };
    let chapters = ChapterRow::list(&chapter_rows, &book.title, position);
    let ticks = chapter_rows
        .iter()
        .map(|c| {
            let at = if total > 0.0 {
                c.start / total * 100.0
            } else {
                0.0
            };
            format!("{at:.3}")
        })
        .collect();

    let mut subline = book.author.clone().unwrap_or_default();
    if let Some(narrator) = &book.narrator {
        subline.push_str(&format!(" · gelesen von {narrator}"));
    }
    subline.push_str(&format!(
        " · {} Kapitel · {}",
        chapter_rows.len(),
        format_hours(total)
    ));

    let editor = shell
        .is_admin
        .then(|| MetadataEditor::new(&book, &track_rows, "Dateien"));
    let page = BookPage {
        editor,
        shell,
        subline,
        chapter_count: chapters.len(),
        chapters,
        play_label: match progress_row {
            Some(p) => format!("Weiter ab {}", format_hms(p.position)),
            None => "Abspielen".into(),
        },
        total,
        current_chapter: current.map(|c| c.title.clone()).unwrap_or_default(),
        progress_meta: format!(
            "{} übrig · {percent} %",
            format_hours((total - position).max(0.0))
        ),
        percent,
        ticks,
        bookmarks: bookmark_rows
            .iter()
            .map(|b| BookmarkRow {
                title: b.title.clone(),
                position: format_hms(b.position),
                position_secs: b.position,
            })
            .collect(),
        series_books: series_rows
            .iter()
            .map(|b| SeriesRow {
                id: b.id,
                title: b.title.clone(),
            })
            .collect(),
        favorite,
        playlists,
        bookmark_at: position,
        book,
    };
    Ok(Html(page.render()?))
}
