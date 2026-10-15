//! `/library/playlists` and `/library/playlists/{id}` (legacy `library/playlists/**`), plus the
//! forms behind them: create, rename, delete, remove and move an entry. Each form answers with a
//! redirect back to the page, which htmx follows inside `#page`, so a playing player keeps
//! playing. Drag and drop goes straight to the JSON API (static/js/playlists.js).

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::Uri;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Router, routing::get, routing::post};
use sqlx::PgPool;

use super::format::format_hours;
use super::layout::Shell;
use crate::api::playlists::{UNKNOWN_PLAYLIST, valid_name};
use crate::auth::WebUser;
use crate::error::{WebError, WebResult};
use crate::library::playlists::{
    Playlist, ResolvedEntry, create_playlist, delete_playlist, list_entries, list_playlists,
    move_entry, owned_playlist, playlist_overview, remove_entry, rename_playlist, resolve_entries,
};
use crate::library::queries::item_durations_with_feed_fallback;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/playlists", get(overview).post(create))
        .route("/library/playlists/{id}", get(detail))
        .route("/library/playlists/{id}/rename", post(rename))
        .route("/library/playlists/{id}/delete", post(delete))
        .route(
            "/library/playlists/{id}/entries/{entry_id}/delete",
            post(remove),
        )
        .route(
            "/library/playlists/{id}/entries/{entry_id}/move",
            post(reorder),
        )
}

pub const NAME_ERROR: &str = "Der Name muss 1–100 Zeichen lang sein.";

/// A playlist in the "Zur Playlist hinzufügen" dialog of the album and book pages.
pub struct PlaylistChoice {
    pub id: i32,
    pub name: String,
}

/// The user's playlists for the dialog, by name.
pub async fn playlist_choices(db: &PgPool, user_id: i32) -> WebResult<Vec<PlaylistChoice>> {
    Ok(list_playlists(db, user_id)
        .await?
        .into_iter()
        .map(|p| PlaylistChoice {
            id: p.id,
            name: p.name,
        })
        .collect())
}

/// legacy `formatDuration` on the overview: hours with one decimal (`2.5`).
pub fn tenth_hours(seconds: f64) -> String {
    format!("{:.1}", seconds / 3600.0)
}

/// legacy `formatDuration` on the detail page: `m:ss`, empty without a length.
pub fn minutes_seconds(seconds: Option<f64>) -> String {
    let Some(seconds) = seconds.filter(|s| s.is_finite()) else {
        return String::new();
    };
    let total = seconds.max(0.0).floor() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}

pub struct PlaylistTile {
    pub id: i32,
    pub name: String,
    pub subtitle: String,
}

#[derive(Template)]
#[template(path = "pages/library/playlists.html")]
struct OverviewPage {
    shell: Shell,
    playlists: Vec<PlaylistTile>,
    error: Option<&'static str>,
    name: String,
}

async fn render_overview(
    state: &AppState,
    user: &WebUser,
    error: Option<&'static str>,
    name: String,
) -> WebResult<Html<String>> {
    let uri: Uri = "/library/playlists".parse().expect("static uri");
    let (shell, rows) = tokio::try_join!(Shell::load(&state.db, user, &uri), async {
        Ok(playlist_overview(&state.db, user.id).await?)
    })?;
    let page = OverviewPage {
        shell,
        playlists: rows
            .into_iter()
            .map(|p| PlaylistTile {
                id: p.id,
                subtitle: format!("{} Titel · {} h", p.entry_count, tenth_hours(p.duration)),
                name: p.name,
            })
            .collect(),
        error,
        name,
    };
    Ok(Html(page.render()?))
}

async fn overview(State(state): State<AppState>, user: WebUser) -> WebResult<Html<String>> {
    render_overview(&state, &user, None, String::new()).await
}

fn form_field(body: &Bytes, key: &str) -> String {
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(body).unwrap_or_default();
    form.get(key).cloned().unwrap_or_default()
}

/// "Anlegen": a new playlist, then straight to it.
async fn create(State(state): State<AppState>, user: WebUser, body: Bytes) -> WebResult<Response> {
    let name = form_field(&body, "name").trim().to_owned();
    if !valid_name(&name) {
        return Ok(render_overview(&state, &user, Some(NAME_ERROR), name)
            .await?
            .into_response());
    }
    let playlist = create_playlist(&state.db, user.id, &name).await?;
    Ok(Redirect::to(&format!("/library/playlists/{}", playlist.id)).into_response())
}

pub struct EntryRow {
    pub id: i32,
    pub title: String,
    pub subtitle: String,
    pub duration: String,
    /// The item the player opens: the entry's own, or the track's.
    pub item_id: i32,
    /// Set for a single-track entry.
    pub track_id: Option<i32>,
    /// The item's kind: a whole album plays from its first track, a book where it was left.
    pub kind: String,
}

impl EntryRow {
    /// `shows` names the podcast of each episode entry (by the episode's parent id).
    fn new(
        resolved: ResolvedEntry,
        durations: &HashMap<i32, f64>,
        shows: &HashMap<i32, String>,
    ) -> Option<EntryRow> {
        let id = resolved.entry.id;
        if let Some(item) = resolved.item {
            let show = item.parent_id.and_then(|p| shows.get(&p)).cloned();
            return Some(EntryRow {
                id,
                subtitle: show
                    .or_else(|| item.artist.clone())
                    .or_else(|| item.author.clone())
                    .unwrap_or_default(),
                duration: minutes_seconds(durations.get(&item.id).copied()),
                title: item.title,
                item_id: item.id,
                track_id: None,
                kind: item.kind,
            });
        }
        let (track, parent) = resolved.track?;
        Some(EntryRow {
            id,
            title: track.title.clone().unwrap_or_else(|| parent.title.clone()),
            duration: minutes_seconds(Some(track.duration)),
            item_id: parent.id,
            track_id: Some(track.id),
            kind: parent.kind,
            subtitle: parent.title,
        })
    }
}

#[derive(Template)]
#[template(path = "pages/library/playlist.html")]
struct DetailPage {
    shell: Shell,
    playlist: Playlist,
    subline: String,
    entries: Vec<EntryRow>,
    error: Option<&'static str>,
}

async fn own(state: &AppState, user: &WebUser, raw_id: &str) -> WebResult<Playlist> {
    let not_found = || WebError::not_found(UNKNOWN_PLAYLIST);
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    owned_playlist(&state.db, user.id, id)
        .await?
        .ok_or_else(not_found)
}

async fn render_detail(
    state: &AppState,
    user: &WebUser,
    playlist: Playlist,
    error: Option<&'static str>,
) -> WebResult<Html<String>> {
    let uri: Uri = format!("/library/playlists/{}", playlist.id)
        .parse()
        .expect("playlist uri");
    let (shell, resolved) = tokio::try_join!(Shell::load(&state.db, user, &uri), async {
        let entries = list_entries(&state.db, playlist.id).await?;
        Ok::<_, WebError>(resolve_entries(&state.db, entries).await?)
    })?;
    let item_ids: Vec<i32> = resolved
        .iter()
        .filter_map(|r| r.item.as_ref().map(|i| i.id))
        .collect();
    // An episode not downloaded yet still has the length its feed claims.
    let durations = item_durations_with_feed_fallback(&state.db, &item_ids).await?;
    let parent_ids: Vec<i32> = resolved
        .iter()
        .filter_map(|r| r.item.as_ref().and_then(|i| i.parent_id))
        .collect();
    let shows: HashMap<i32, String> =
        sqlx::query_as::<_, (i32, String)>("SELECT id, title FROM items WHERE id = ANY($1)")
            .bind(&parent_ids)
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .collect();
    let total: f64 = resolved
        .iter()
        .map(|r| match (&r.item, &r.track) {
            (Some(item), _) => durations.get(&item.id).copied().unwrap_or(0.0),
            (None, Some((track, _))) => track.duration,
            _ => 0.0,
        })
        .sum();
    let entries: Vec<EntryRow> = resolved
        .into_iter()
        .filter_map(|r| EntryRow::new(r, &durations, &shows))
        .collect();
    let page = DetailPage {
        shell,
        subline: format!("{} Titel · {}", entries.len(), format_hours(total)),
        entries,
        playlist,
        error,
    };
    Ok(Html(page.render()?))
}

async fn detail(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
) -> WebResult<Html<String>> {
    let playlist = own(&state, &user, &raw_id).await?;
    render_detail(&state, &user, playlist, None).await
}

fn back_to(playlist: &Playlist) -> Response {
    Redirect::to(&format!("/library/playlists/{}", playlist.id)).into_response()
}

async fn rename(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> WebResult<Response> {
    let playlist = own(&state, &user, &raw_id).await?;
    let name = form_field(&body, "name").trim().to_owned();
    if !valid_name(&name) {
        return Ok(render_detail(&state, &user, playlist, Some(NAME_ERROR))
            .await?
            .into_response());
    }
    let renamed = rename_playlist(&state.db, playlist.id, &name).await?;
    Ok(back_to(&renamed))
}

async fn delete(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
) -> WebResult<Response> {
    let playlist = own(&state, &user, &raw_id).await?;
    delete_playlist(&state.db, playlist.id).await?;
    Ok(Redirect::to("/library/playlists").into_response())
}

/// "Entfernen". An entry that is already gone is no error: the page shows what is left.
async fn remove(
    State(state): State<AppState>,
    user: WebUser,
    Path((raw_id, raw_entry)): Path<(String, String)>,
) -> WebResult<Response> {
    let playlist = own(&state, &user, &raw_id).await?;
    if let Ok(entry_id) = raw_entry.parse::<i32>() {
        remove_entry(&state.db, playlist.id, entry_id).await?;
    }
    Ok(back_to(&playlist))
}

/// "Nach oben" / "Nach unten": the form carries the target position; past either end it stays
/// at the end.
async fn reorder(
    State(state): State<AppState>,
    user: WebUser,
    Path((raw_id, raw_entry)): Path<(String, String)>,
    body: Bytes,
) -> WebResult<Response> {
    let playlist = own(&state, &user, &raw_id).await?;
    let position = form_field(&body, "position").trim().parse::<i32>();
    if let (Ok(entry_id), Ok(position)) = (raw_entry.parse::<i32>(), position) {
        move_entry(&state.db, playlist.id, entry_id, position).await?;
    }
    Ok(back_to(&playlist))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_hours_have_one_decimal() {
        assert_eq!(tenth_hours(0.0), "0.0");
        assert_eq!(tenth_hours(250.0), "0.1");
        assert_eq!(tenth_hours(9000.0), "2.5");
    }

    #[test]
    fn entry_lengths_are_minutes_and_seconds() {
        assert_eq!(minutes_seconds(None), "");
        assert_eq!(minutes_seconds(Some(0.0)), "0:00");
        assert_eq!(minutes_seconds(Some(200.9)), "3:20");
        assert_eq!(minutes_seconds(Some(3725.0)), "62:05");
    }
}
