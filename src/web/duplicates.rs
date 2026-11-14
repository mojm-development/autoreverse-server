//! Einstellungen → Doppelte Alben: albums with the same artist and title in more than one
//! folder. A Verwalter picks the copy to keep; the others are hidden, their files untouched.

use std::collections::HashMap;
use std::path::PathBuf;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::Uri;
use axum::response::{Html, Redirect};
use axum::{Router, routing::get};

use super::format::{human_duration, numeric_date};
use super::layout::Shell;
use super::settings::{Section, settings_shell, web_user};
use crate::auth::WebAdmin;
use crate::error::{WebError, WebResult};
use crate::library::duplicates::{
    Copy, DeleteRefused, Group, delete_all_hidden, delete_hidden_copy, groups, keep,
    keep_all_recommended, unhide,
};
use crate::library::paths::get_library_paths;
use crate::state::AppState;

const PAGE: &str = "/settings/duplicates";

pub fn router() -> Router<AppState> {
    Router::new().route(PAGE, get(page).post(action))
}

pub struct CopyView {
    pub id: i32,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    /// The folder, below the music root when it lies there.
    pub folder: String,
    pub has_cover: bool,
    /// "12 Titel · 48 min".
    pub tracks_line: String,
    pub added: String,
    pub recommended: bool,
    pub hidden: bool,
}

pub struct GroupView {
    pub anchor: String,
    /// Where the page jumps after a decision here: the next open group, since this one leaves
    /// the list.
    pub next_anchor: String,
    pub copies: Vec<CopyView>,
}

pub struct HiddenView {
    pub copy: CopyView,
    /// The title and folder of the copy that is kept instead.
    pub kept: String,
}

#[derive(Template)]
#[template(path = "pages/settings/duplicates.html")]
struct DuplicatesPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    open: Vec<GroupView>,
    hidden: Vec<HiddenView>,
    /// After deleting: "3 Kopien gelöscht."
    notice: Option<String>,
    error: Option<&'static str>,
}

/// What the last deletion left in the address: `geloescht=<n>`, `fehler=<code>`.
#[derive(serde::Deserialize, Default)]
struct Outcome {
    geloescht: Option<usize>,
    fehler: Option<String>,
}

fn view(copy: &Copy, music_root: Option<&str>, recommended: bool) -> CopyView {
    let path = copy.source_path.clone().unwrap_or_default();
    let folder = music_root
        .and_then(|root| path.strip_prefix(root.trim_end_matches('/')))
        .map_or(path.clone(), |rest| rest.trim_start_matches('/').to_owned());
    let duration = human_duration(copy.duration);
    let mut tracks_line = format!("{} Titel", copy.track_count);
    if !duration.is_empty() {
        tracks_line.push_str(" · ");
        tracks_line.push_str(&duration);
    }
    CopyView {
        id: copy.id,
        title: copy.title.clone(),
        artist: copy.artist.clone().unwrap_or_default(),
        year: copy.year,
        folder,
        has_cover: copy.has_cover,
        tracks_line,
        added: numeric_date(Some(copy.added_at)),
        recommended,
        hidden: copy.duplicate_of.is_some(),
    }
}

fn open_views(groups: &[Group], music_root: Option<&str>) -> Vec<GroupView> {
    let open: Vec<&Group> = groups.iter().filter(|g| g.is_open()).collect();
    let anchor = |g: &Group| format!("gruppe-{}", g.recommended().id);
    open.iter()
        .enumerate()
        .map(|(i, group)| GroupView {
            anchor: anchor(group),
            next_anchor: open.get(i + 1).map(|g| anchor(g)).unwrap_or_default(),
            copies: group
                .copies
                .iter()
                .enumerate()
                .map(|(j, c)| view(c, music_root, j == 0))
                .collect(),
        })
        .collect()
}

fn hidden_views(groups: &[Group], music_root: Option<&str>) -> Vec<HiddenView> {
    let by_id: HashMap<i32, &Copy> = groups
        .iter()
        .flat_map(|g| g.copies.iter())
        .map(|c| (c.id, c))
        .collect();
    groups
        .iter()
        .flat_map(|g| g.copies.iter())
        .filter_map(|copy| {
            let kept = by_id.get(&copy.duplicate_of?)?;
            let kept = view(kept, music_root, false);
            Some(HiddenView {
                copy: view(copy, music_root, false),
                kept: format!("{} ({})", kept.title, kept.folder),
            })
        })
        .collect()
}

async fn page(
    State(state): State<AppState>,
    admin: WebAdmin,
    uri: Uri,
    RawQuery(query): RawQuery,
) -> WebResult<Html<String>> {
    // A hand-edited address shows no message rather than an error.
    let outcome: Outcome =
        serde_urlencoded::from_str(query.as_deref().unwrap_or("")).unwrap_or_default();
    let (shell, sections) = settings_shell(&state, &web_user(&admin), &uri).await?;
    let music_root = get_library_paths(&state.db).await?.music_dir;
    let groups = groups(&state.db).await?;
    let page = DuplicatesPage {
        shell,
        sections,
        open: open_views(&groups, music_root.as_deref()),
        hidden: hidden_views(&groups, music_root.as_deref()),
        notice: outcome.geloescht.map(|n| match n {
            1 => "1 Kopie gelöscht.".to_owned(),
            n => format!("{n} Kopien gelöscht."),
        }),
        error: outcome.fehler.as_deref().and_then(DeleteRefused::message),
    };
    Ok(Html(page.render()?))
}

/// `action=keep|unhide|keep-all|delete|delete-hidden`, `id`, and `next` (the anchor to land
/// on). Answers with a redirect back to the page, so a reload does not send the form again; a
/// deletion reports its outcome in the address.
async fn action(
    State(state): State<AppState>,
    _admin: WebAdmin,
    body: Bytes,
) -> WebResult<Redirect> {
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let id = || form.get("id").and_then(|v| v.parse::<i32>().ok());
    let not_found = || WebError::not_found("Album nicht gefunden");
    let music_root = || async {
        Ok::<_, WebError>(
            get_library_paths(&state.db)
                .await?
                .music_dir
                .map(PathBuf::from),
        )
    };
    let deleted = |count: usize, refused: Option<DeleteRefused>| {
        let mut to = format!("{PAGE}?geloescht={count}");
        if let Some(reason) = refused {
            to.push_str("&fehler=");
            to.push_str(reason.code());
        }
        Ok(Redirect::to(&to))
    };
    match form.get("action").map(String::as_str) {
        Some("keep") => {
            keep(&state.db, id().ok_or_else(not_found)?)
                .await?
                .ok_or_else(not_found)?;
        }
        Some("unhide") => {
            unhide(&state.db, id().ok_or_else(not_found)?).await?;
        }
        Some("keep-all") => {
            keep_all_recommended(&state.db).await?;
        }
        Some("delete") => {
            let root = music_root().await?;
            let outcome =
                delete_hidden_copy(&state.db, id().ok_or_else(not_found)?, root.as_deref()).await?;
            return match outcome {
                Ok(()) => deleted(1, None),
                Err(reason) => deleted(0, Some(reason)),
            };
        }
        Some("delete-hidden") => {
            let root = music_root().await?;
            let (count, refused) = delete_all_hidden(&state.db, root.as_deref()).await?;
            return deleted(count, refused);
        }
        _ => return Err(WebError::not_found("Unbekannte Aktion")),
    }
    let next = form
        .get("next")
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    Ok(Redirect::to(&match next {
        Some(anchor) => format!("{PAGE}#{anchor}"),
        None => PAGE.to_owned(),
    }))
}
