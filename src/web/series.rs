//! `/library/series` (legacy `library/series/**`): one shelf per series, grouped by author.

use askama::Template;
use axum::extract::State;
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::layout::Shell;
use crate::auth::WebUser;
use crate::error::WebResult;
use crate::library::queries::book_series;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/library/series", get(page))
}

pub struct Shelf {
    pub series: String,
    pub href: String,
    /// Up to three item ids with artwork.
    pub covers: Vec<i32>,
    /// "4 Bände · 1 beendet".
    pub count_line: String,
    /// Lower-cased series and author, for the page's own filter.
    pub search: String,
}

pub struct AuthorGroup {
    pub author: String,
    pub shelves: Vec<Shelf>,
}

#[derive(Template)]
#[template(path = "pages/library/series.html")]
struct SeriesPage {
    shell: Shell,
    total: usize,
    groups: Vec<AuthorGroup>,
}

fn series_href(series: &str, author: Option<&str>) -> String {
    let mut params = vec![("series", series)];
    if let Some(author) = author {
        params.push(("author", author));
    }
    format!(
        "/library/books?{}",
        serde_urlencoded::to_string(&params).unwrap_or_default()
    )
}

async fn page(State(state): State<AppState>, user: WebUser, uri: Uri) -> WebResult<Html<String>> {
    let shell = Shell::load(&state.db, &user, &uri).await?;
    let rows = book_series(&state.db, user.id).await?;
    let total = rows.len();
    // Rows come ordered by author, so consecutive runs are the groups.
    let mut groups: Vec<(Option<String>, Vec<Shelf>)> = Vec::new();
    for row in rows {
        let mut count_line = format!(
            "{} {}",
            row.count,
            if row.count == 1 { "Band" } else { "Bände" }
        );
        if row.finished_count > 0 {
            count_line.push_str(&format!(" · {} beendet", row.finished_count));
        }
        let shelf = Shelf {
            href: series_href(&row.series, row.author.as_deref()),
            covers: row.covers.unwrap_or_default().into_iter().take(3).collect(),
            count_line,
            search: format!(
                "{}\n{}",
                row.series.to_lowercase(),
                row.author.as_deref().unwrap_or("").to_lowercase()
            ),
            series: row.series,
        };
        match groups.last_mut() {
            Some((author, shelves)) if *author == row.author => shelves.push(shelf),
            _ => groups.push((row.author, vec![shelf])),
        }
    }
    let page = SeriesPage {
        shell,
        total,
        groups: groups
            .into_iter()
            .map(|(author, shelves)| AuthorGroup {
                author: author.unwrap_or_else(|| "Ohne Autor".into()),
                shelves,
            })
            .collect(),
    };
    Ok(Html(page.render()?))
}
