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

#[derive(Template)]
#[template(path = "pages/library.html")]
struct LibraryPage {
    shell: Shell,
    greeting: &'static str,
    name: String,
}

/// The start page. Slice 1 only renders the shell and the greeting; "Weiter hören" and
/// "Neu hinzugefügt" follow with playback and scanning.
async fn library(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    let shell = Shell::load(&state.db, &user, &uri).await?;
    let page = LibraryPage {
        greeting: greeting(chrono::Local::now().hour()),
        name: capitalize(&shell.user_name),
        shell,
    };
    Ok(Html(page.render()?))
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
    fn capitalizes_the_first_letter_only() {
        assert_eq!(capitalize("oliver"), "Oliver");
        assert_eq!(capitalize("ärger"), "Ärger");
        assert_eq!(capitalize(""), "");
    }
}
