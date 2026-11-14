//! `/library/playlists` and `/library/playlists/{id}` (legacy `library/playlists/**`), plus the
//! forms behind them: create, rename, describe and share, cover, delete, remove and move an
//! entry. Each form answers with a redirect back to the page, which htmx follows inside `#page`,
//! so a playing player keeps playing. Drag and drop, the import of a file and "Link kopieren" go
//! straight to the JSON API or the clipboard (static/js/playlists.js, static/js/share.js).
//!
//! Who may do what is `library::playlists::Playlist`'s to say, as for the API: a shared playlist
//! of somebody else shows without the owner's controls, and with the entry controls only when
//! it is collaborative.

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Router, routing::get, routing::post};
use sqlx::PgPool;

use super::format::format_hours;
use super::layout::Shell;
use crate::api::artists::{UPLOAD_BODY_LIMIT, read_image_upload};
use crate::api::playlists::{
    NOT_COLLABORATIVE, OWNER_ONLY, UNKNOWN_PLAYLIST, clean_description, clean_folder, valid_name,
};
use crate::auth::WebUser;
use crate::error::{WebError, WebResult};
use crate::library::playlists::{
    Playlist, PlaylistChanges, PlaylistSummary, ResolvedEntry, Scope, clear_cover, create_playlist,
    delete_playlist, folders, list_entries, move_entry, playlist_summaries, remove_entry,
    rename_playlist, resolve_entries, store_cover, update_playlist, visible_playlist,
};
use crate::library::queries::item_durations_with_feed_fallback;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/playlists", get(overview).post(create))
        .route("/library/playlists/{id}", get(detail))
        .route("/library/playlists/{id}/rename", post(rename))
        .route("/library/playlists/{id}/details", post(details))
        .route(
            "/library/playlists/{id}/cover",
            post(upload_cover).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/library/playlists/{id}/cover/delete", post(remove_cover))
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
pub const DESCRIPTION_ERROR: &str = "Die Beschreibung darf höchstens 2000 Zeichen lang sein.";
pub const FOLDER_ERROR: &str = "Der Ordnername darf höchstens 100 Zeichen lang sein.";

/// A playlist in the "Zur Playlist hinzufügen" dialog of the album and book pages.
pub struct PlaylistChoice {
    pub id: i32,
    pub name: String,
}

/// The playlists the user can add to, for the dialog: their own by name, then the ones others
/// opened for everybody's entries, marked with whose they are.
pub async fn playlist_choices(db: &PgPool, user_id: i32) -> WebResult<Vec<PlaylistChoice>> {
    Ok(playlist_summaries(db, user_id, Scope::All)
        .await?
        .into_iter()
        .filter(|p| p.playlist.editable_by(user_id))
        .map(|p| PlaylistChoice {
            id: p.playlist.id,
            name: if p.playlist.is_owner(user_id) {
                p.playlist.name
            } else {
                format!("{} (von {})", p.playlist.name, p.owner)
            },
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
    /// The uploaded cover; without one the tile shows the playlist icon.
    pub cover_url: Option<String>,
}

impl PlaylistTile {
    /// `theirs`: a playlist of somebody else, which names its owner.
    fn new(summary: PlaylistSummary, theirs: bool) -> PlaylistTile {
        let mut subtitle = format!(
            "{} Titel · {} h",
            summary.entry_count,
            tenth_hours(summary.duration)
        );
        if theirs {
            subtitle = format!("von {} · {subtitle}", summary.owner);
        }
        PlaylistTile {
            id: summary.playlist.id,
            cover_url: cover_url(&summary.playlist, 384),
            name: summary.playlist.name,
            subtitle,
        }
    }
}

/// The address of a playlist's own cover. The file changes in place under one address, so its
/// name (which carries the type) and the moment go along as a cache buster.
fn cover_url(playlist: &Playlist, size: u32) -> Option<String> {
    playlist.cover_path.as_ref()?;
    Some(format!(
        "/playlists/{}/cover?size={size}&v={}",
        playlist.id,
        chrono::Utc::now().timestamp_millis()
    ))
}

/// The user's playlists that share a folder name.
pub struct Folder {
    pub name: String,
    pub playlists: Vec<PlaylistTile>,
}

#[derive(Template)]
#[template(path = "pages/library/playlists.html")]
struct OverviewPage {
    shell: Shell,
    /// The user's own, outside any folder.
    playlists: Vec<PlaylistTile>,
    /// The user's folders by name, each with its playlists.
    folders: Vec<Folder>,
    /// What other accounts shared.
    shared: Vec<PlaylistTile>,
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
    let (shell, rows) = tokio::try_join!(Shell::load(state, user, &uri), async {
        Ok(playlist_summaries(&state.db, user.id, Scope::All).await?)
    })?;
    let mut playlists = Vec::new();
    let mut folders: Vec<Folder> = Vec::new();
    let mut shared = Vec::new();
    for summary in rows {
        if !summary.playlist.is_owner(user.id) {
            // Somebody else's folders are theirs: here their playlists are one flat list.
            shared.push(PlaylistTile::new(summary, true));
            continue;
        }
        let Some(folder) = summary.playlist.folder.clone() else {
            playlists.push(PlaylistTile::new(summary, false));
            continue;
        };
        let tile = PlaylistTile::new(summary, false);
        match folders.iter_mut().find(|f| f.name == folder) {
            Some(known) => known.playlists.push(tile),
            None => folders.push(Folder {
                name: folder,
                playlists: vec![tile],
            }),
        }
    }
    folders.sort_by_cached_key(|f| f.name.to_lowercase());
    let page = OverviewPage {
        shell,
        playlists,
        folders,
        shared,
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
    /// The owner's account name; shown on somebody else's playlist.
    owner: String,
    /// The owner looks at it: name, description, folder, sharing, cover, deleting.
    mine: bool,
    /// Entries may be removed and moved.
    can_edit: bool,
    cover_url: Option<String>,
    /// The user's folder names, as suggestions for the "Ordner" field.
    folders: Vec<String>,
    subline: String,
    entries: Vec<EntryRow>,
    error: Option<&'static str>,
}

/// What a form is about to do with a playlist (`api::playlists` has the same three).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    Read,
    Edit,
    Own,
}

/// The playlist of the path if the user may do this with it: "Unbekannte Playlist" for one they
/// cannot even see, 403 for one they see but may not change this way.
async fn find(state: &AppState, user: &WebUser, raw_id: &str, need: Need) -> WebResult<Playlist> {
    let not_found = || WebError::not_found(UNKNOWN_PLAYLIST);
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let playlist = visible_playlist(&state.db, user.id, id)
        .await?
        .ok_or_else(not_found)?;
    match need {
        Need::Edit if !playlist.editable_by(user.id) => Err(WebError::Page(
            StatusCode::FORBIDDEN,
            NOT_COLLABORATIVE.into(),
        )),
        Need::Own if !playlist.is_owner(user.id) => {
            Err(WebError::Page(StatusCode::FORBIDDEN, OWNER_ONLY.into()))
        }
        _ => Ok(playlist),
    }
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
    let (shell, resolved, owner, folders) = tokio::try_join!(
        Shell::load(state, user, &uri),
        async {
            let entries = list_entries(&state.db, playlist.id).await?;
            Ok::<_, WebError>(resolve_entries(&state.db, entries).await?)
        },
        async {
            Ok(
                sqlx::query_scalar::<_, String>("SELECT name FROM users WHERE id = $1")
                    .bind(playlist.user_id)
                    .fetch_optional(&state.db)
                    .await?
                    .unwrap_or_default(),
            )
        },
        async { Ok(folders(&state.db, user.id).await?) },
    )?;
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
        owner,
        mine: playlist.is_owner(user.id),
        can_edit: playlist.editable_by(user.id),
        cover_url: cover_url(&playlist, 384),
        folders,
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
    let playlist = find(&state, &user, &raw_id, Need::Read).await?;
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
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    let name = form_field(&body, "name").trim().to_owned();
    if !valid_name(&name) {
        return Ok(render_detail(&state, &user, playlist, Some(NAME_ERROR))
            .await?
            .into_response());
    }
    let renamed = rename_playlist(&state.db, playlist.id, &name).await?;
    Ok(back_to(&renamed))
}

/// "Details": description, folder and the two sharing switches, always all four — an unticked
/// checkbox is simply absent from a form, so absence means "off" here, unlike in the API.
async fn details(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> WebResult<Response> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let field = |key: &str| form.get(key).map(String::as_str).unwrap_or("");
    // Browsers send a textarea's line breaks as CRLF; stored, they would count twice.
    let description = match clean_description(&field("description").replace("\r\n", "\n")) {
        Ok(description) => description,
        Err(_) => {
            return Ok(
                render_detail(&state, &user, playlist, Some(DESCRIPTION_ERROR))
                    .await?
                    .into_response(),
            );
        }
    };
    let folder = match clean_folder(field("folder")) {
        Ok(folder) => folder,
        Err(_) => {
            return Ok(render_detail(&state, &user, playlist, Some(FOLDER_ERROR))
                .await?
                .into_response());
        }
    };
    let changes = PlaylistChanges {
        name: None,
        description: Some(description),
        folder: Some(folder),
        shared: Some(form.contains_key("shared")),
        collaborative: Some(form.contains_key("collaborative")),
    };
    let updated = update_playlist(&state.db, playlist.id, &changes).await?;
    Ok(back_to(&updated))
}

/// "Cover hochladen". A refused file shows on the page with its reason.
async fn upload_cover(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
    multipart: Result<Multipart, MultipartRejection>,
) -> WebResult<Response> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    let refused = |detail: String| WebError::Page(StatusCode::UNPROCESSABLE_ENTITY, detail);
    let upload = match read_image_upload(multipart).await {
        Ok(Some(upload)) if upload.size > 0 => upload,
        Ok(_) => return Err(refused("Keine Bilddatei ausgewählt".into())),
        Err(e) => return Err(refused(e.detail)),
    };
    store_cover(
        &state.db,
        &playlist,
        &upload.content_type,
        upload.size,
        &upload.bytes,
        &state.config.playlists_dir,
    )
    .await
    .map_err(|e| WebError::Page(e.status, e.detail))?;
    Ok(back_to(&playlist))
}

/// "Cover entfernen": back to the picture the entries make.
async fn remove_cover(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
) -> WebResult<Response> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    clear_cover(&state.db, &playlist).await?;
    Ok(back_to(&playlist))
}

async fn delete(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
) -> WebResult<Response> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    delete_playlist(&state.db, playlist.id).await?;
    Ok(Redirect::to("/library/playlists").into_response())
}

/// "Entfernen". An entry that is already gone is no error: the page shows what is left.
async fn remove(
    State(state): State<AppState>,
    user: WebUser,
    Path((raw_id, raw_entry)): Path<(String, String)>,
) -> WebResult<Response> {
    let playlist = find(&state, &user, &raw_id, Need::Edit).await?;
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
    let playlist = find(&state, &user, &raw_id, Need::Edit).await?;
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
