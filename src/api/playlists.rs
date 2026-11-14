//! `GET|POST /playlists`, `GET|PATCH|DELETE /playlists/{id}`, `POST /playlists/{id}/entries`,
//! `PUT|DELETE /playlists/{id}/entries/{entryId}` (legacy `routes/playlists/**`), and new in
//! Rust: `GET|PUT|DELETE /playlists/{id}/cover`, `GET /playlists/{id}/export`,
//! `POST /playlists/import`.
//!
//! A user sees their own playlists and those another account shared. Someone else's private one
//! is "Unbekannte Playlist", the same 404 as one that does not exist — for admins too, and on
//! every route, so nobody learns that it exists. A shared one of somebody else can be read by
//! all; changing its entries takes `collaborative`, everything else is its owner's (403).

use axum::body::Body;
use axum::extract::multipart::Multipart;
use axum::extract::{DefaultBodyLimit, FromRequest, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get, routing::post, routing::put};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::artists::{ImageUpload, UPLOAD_BODY_LIMIT, read_image_upload};
use super::images::{serve_image, thumb_size};
use super::validate::{ApiJson, ApiQuery, JsonBody, QueryMap};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::artist_covers::{MAX_IMAGE_BYTES, TOO_LARGE};
use crate::library::m3u;
use crate::library::paths::{get_library_paths, relative_to_roots};
use crate::library::playlists::{
    EntryTarget, ExportTrack, Playlist, PlaylistChanges, PlaylistSummary, ResolvedEntry, Scope,
    append_entry, clear_cover, create_playlist, create_with_tracks, delete_playlist, entry,
    entry_count, export_tracks, list_entries, move_entry, playlist_summaries, playlist_summary,
    remove_entry, resolve_entries, store_cover, update_playlist, visible_playlist,
};
use crate::library::queries;
use crate::serialize::{ItemSummary, TrackSummary, iso, js_number};
use crate::state::AppState;

/// Room for a 2 MB playlist file inside a JSON string, where every control character and, from
/// a cautious client, every non-ASCII one takes six bytes.
const IMPORT_BODY_LIMIT: usize = 6 * m3u::MAX_IMPORT_BYTES + 64 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/playlists", get(list).post(create))
        .route(
            "/playlists/import",
            post(import).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT)),
        )
        .route("/playlists/{id}", get(detail).patch(rename).delete(remove))
        .route(
            "/playlists/{id}/cover",
            get(cover)
                .put(upload_cover)
                .delete(remove_cover)
                // As for an artist's picture: the largest allowed image plus multipart framing.
                .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/playlists/{id}/export", get(export))
        .route("/playlists/{id}/entries", post(add_entry))
        .route(
            "/playlists/{id}/entries/{entry_id}",
            put(reorder_entry).delete(remove_entry_route),
        )
}

pub const UNKNOWN_PLAYLIST: &str = "Unbekannte Playlist";
pub const UNKNOWN_ENTRY: &str = "Unbekannter Eintrag";
pub const NAME_INVALID: &str = "name muss 1–100 Zeichen haben";
pub const OWNER_ONLY: &str = "Nur der Besitzer darf diese Playlist ändern";
pub const NOT_COLLABORATIVE: &str = "Diese Playlist darf nur ihr Besitzer bearbeiten";
pub const DESCRIPTION_INVALID: &str = "description darf höchstens 2000 Zeichen haben";
pub const FOLDER_INVALID: &str = "folder darf höchstens 100 Zeichen haben";
pub const NO_COVER: &str = "Kein Cover";
pub const NOTHING_MATCHED: &str = "Kein Titel der Datei wurde in der Bibliothek gefunden";
pub const IMPORT_TOO_LARGE: &str = "Die Datei ist zu groß (maximal 2 MB)";

pub const MAX_DESCRIPTION_CHARS: usize = 2000;
pub const MAX_FOLDER_CHARS: usize = 100;

/// A playlist as every route answers it.
#[derive(Serialize)]
pub struct PlaylistOut {
    id: i32,
    name: String,
    #[serde(serialize_with = "iso")]
    created_at: DateTime<Utc>,
    entry_count: i64,
    description: Option<String>,
    folder: Option<String>,
    /// An uploaded cover exists (`GET /playlists/{id}/cover`); without one the clients compose
    /// one from the entries.
    has_cover: bool,
    /// The owner's account name.
    owner: String,
    mine: bool,
    shared: bool,
    collaborative: bool,
    /// Whether the asking account may add, remove and move entries.
    can_edit: bool,
    /// Seconds, all entries added up.
    #[serde(serialize_with = "js_number")]
    duration: f64,
}

impl PlaylistOut {
    pub fn new(summary: PlaylistSummary, user_id: i32) -> Self {
        let PlaylistSummary {
            playlist,
            owner,
            entry_count,
            duration,
        } = summary;
        PlaylistOut {
            mine: playlist.is_owner(user_id),
            can_edit: playlist.editable_by(user_id),
            has_cover: playlist.cover_path.is_some(),
            id: playlist.id,
            name: playlist.name,
            created_at: playlist.created_at,
            entry_count,
            description: playlist.description,
            folder: playlist.folder,
            owner,
            shared: playlist.shared,
            collaborative: playlist.collaborative,
            duration,
        }
    }
}

#[derive(Serialize)]
struct PlaylistsOut {
    playlists: Vec<PlaylistOut>,
}

#[derive(Serialize)]
pub struct EntryOut {
    id: i32,
    position: i32,
    item: Option<ItemSummary>,
    track: Option<TrackSummary>,
}

impl From<&ResolvedEntry> for EntryOut {
    fn from(resolved: &ResolvedEntry) -> Self {
        EntryOut {
            id: resolved.entry.id,
            position: resolved.entry.position,
            item: resolved.item.as_ref().map(ItemSummary::from),
            track: resolved
                .track
                .as_ref()
                .map(|(track, item)| TrackSummary::new(track, item)),
        }
    }
}

#[derive(Serialize)]
struct PlaylistDetailOut {
    #[serde(flatten)]
    playlist: PlaylistOut,
    entries: Vec<EntryOut>,
}

/// A playlist name as legacy accepted it: a string of 1–100 UTF-16 code units (JS `length`).
pub fn valid_name(name: &str) -> bool {
    (1..=100).contains(&name.encode_utf16().count())
}

fn name_from(body: &Map<String, Value>) -> ApiResult<&str> {
    body.get("name")
        .and_then(Value::as_str)
        .filter(|name| valid_name(name))
        .ok_or_else(|| ApiError::unprocessable(NAME_INVALID))
}

/// A description as it is stored: `None` for nothing but blanks. `Err` when it is too long.
pub fn clean_description(raw: &str) -> Result<Option<String>, &'static str> {
    if raw.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(DESCRIPTION_INVALID);
    }
    Ok((!raw.trim().is_empty()).then(|| raw.to_owned()))
}

/// A folder name as it is stored: trimmed, since two playlists are in one folder exactly when
/// the names are equal; `None` for an empty one.
pub fn clean_folder(raw: &str) -> Result<Option<String>, &'static str> {
    let name = raw.trim();
    if name.chars().count() > MAX_FOLDER_CHARS {
        return Err(FOLDER_INVALID);
    }
    Ok((!name.is_empty()).then(|| name.to_owned()))
}

/// An optional text field that `null` or an empty string clears. Outer `None`: not in the body.
fn text_field(
    body: &Map<String, Value>,
    key: &str,
    clean: fn(&str) -> Result<Option<String>, &'static str>,
    invalid: &'static str,
) -> ApiResult<Option<Option<String>>> {
    match body.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(text)) => clean(text).map(Some).map_err(ApiError::unprocessable),
        Some(_) => Err(ApiError::unprocessable(invalid)),
    }
}

fn bool_field(body: &Map<String, Value>, key: &str) -> ApiResult<Option<bool>> {
    match body.get(key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(ApiError::unprocessable(format!(
            "{key} muss true oder false sein"
        ))),
    }
}

/// The fields beside the name that the owner sets: all optional, each checked.
fn details_from(body: &Map<String, Value>) -> ApiResult<PlaylistChanges> {
    Ok(PlaylistChanges {
        name: None,
        description: text_field(body, "description", clean_description, DESCRIPTION_INVALID)?,
        folder: text_field(body, "folder", clean_folder, FOLDER_INVALID)?,
        shared: bool_field(body, "shared")?,
        collaborative: bool_field(body, "collaborative")?,
    })
}

/// A path id; anything that is not one names no playlist (or entry).
fn path_id(raw: &str, detail: &'static str) -> ApiResult<i32> {
    raw.parse().map_err(|_| ApiError::not_found(detail))
}

/// What a route is about to do with a playlist.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    /// See it, play it, export it.
    Read,
    /// Add, remove, move entries.
    Edit,
    /// Name, description, folder, sharing, cover, deleting.
    Own,
}

/// The playlist of the path if the user may do this with it. One that the user cannot even see
/// is 404 whatever was asked for; 403 only ever speaks of a playlist the user can see anyway.
async fn find(state: &AppState, user: &ApiUser, raw_id: &str, need: Need) -> ApiResult<Playlist> {
    let id = path_id(raw_id, UNKNOWN_PLAYLIST)?;
    let playlist = visible_playlist(&state.db, user.id, id)
        .await?
        .ok_or_else(|| ApiError::not_found(UNKNOWN_PLAYLIST))?;
    match need {
        Need::Edit if !playlist.editable_by(user.id) => Err(ApiError::forbidden(NOT_COLLABORATIVE)),
        Need::Own if !playlist.is_owner(user.id) => Err(ApiError::forbidden(OWNER_ONLY)),
        _ => Ok(playlist),
    }
}

/// The playlist as it stands now, for the answer of a route that changed it.
async fn current(state: &AppState, user: &ApiUser, id: i32) -> ApiResult<PlaylistOut> {
    let summary = playlist_summary(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(UNKNOWN_PLAYLIST))?;
    Ok(PlaylistOut::new(summary, user.id))
}

/// `?scope=mine|shared|all`. Without it the user's own, as before sharing existed: a client that
/// knows nothing of it must not suddenly list other people's playlists among its own.
async fn list(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<PlaylistsOut>> {
    let scope = match query.get("scope") {
        None => Scope::Mine,
        Some(raw) => Scope::parse(raw)
            .ok_or_else(|| ApiError::unprocessable("scope muss mine, shared oder all sein"))?,
    };
    let rows = playlist_summaries(&state.db, user.id, scope).await?;
    Ok(Json(PlaylistsOut {
        playlists: rows
            .into_iter()
            .map(|summary| PlaylistOut::new(summary, user.id))
            .collect(),
    }))
}

/// `{name}` → the new playlist; `description`, `folder`, `shared` and `collaborative` may come
/// along as on `PATCH`. 200 like legacy, not 201.
async fn create(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Json<PlaylistOut>> {
    let name = name_from(&body)?;
    let details = details_from(&body)?;
    let row = create_playlist(&state.db, user.id, name).await?;
    if !details.is_empty() {
        update_playlist(&state.db, row.id, &details).await?;
    }
    Ok(Json(current(&state, &user, row.id).await?))
}

async fn detail(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<PlaylistDetailOut>> {
    let playlist = find(&state, &user, &raw_id, Need::Read).await?;
    let entries = resolve_entries(&state.db, list_entries(&state.db, playlist.id).await?).await?;
    Ok(Json(PlaylistDetailOut {
        playlist: current(&state, &user, playlist.id).await?,
        entries: entries.iter().map(EntryOut::from).collect(),
    }))
}

/// `PATCH`: the name and, new, `description`, `folder`, `shared`, `collaborative` — any of them,
/// at least one. A body with none of them is refused as it always was (a missing name).
async fn rename(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
    body: JsonBody,
) -> ApiResult<Json<PlaylistOut>> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    let body: Map<String, Value> = body.parse()?;
    let mut changes = details_from(&body)?;
    if body.contains_key("name") || changes.is_empty() {
        changes.name = Some(name_from(&body)?.to_owned());
    }
    update_playlist(&state.db, playlist.id, &changes).await?;
    Ok(Json(current(&state, &user, playlist.id).await?))
}

async fn remove(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    delete_playlist(&state.db, playlist.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The playlist's own, uploaded cover. Without one this is 404 and the client composes one from
/// the entries: the server does not guess a picture here, unlike for an artist.
async fn cover(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    let playlist = find(&state, &user, &raw_id, Need::Read).await?;
    let size = thumb_size(&query)?;
    let path = playlist
        .cover_path
        .ok_or_else(|| ApiError::not_found(NO_COVER))?;
    serve_image(
        std::path::Path::new(&path),
        std::slice::from_ref(&state.config.playlists_dir),
        NO_COVER,
        // Short and private: the picture changes in place under the same address.
        "private, max-age=300",
        size,
        &state.config.thumbs_dir,
        &headers,
    )
    .await
}

/// The image of an upload in either shape: the multipart field `image`, exactly as an artist's
/// picture is sent, or the picture itself as the request body with its `Content-Type`.
pub async fn read_cover_upload(request: Request) -> ApiResult<Option<ImageUpload>> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if content_type
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
    {
        return read_image_upload(Multipart::from_request(request, &()).await).await;
    }
    // One byte past the limit is enough to know that it is too large.
    let bytes = match axum::body::to_bytes(request.into_body(), MAX_IMAGE_BYTES + 1).await {
        Ok(bytes) => bytes,
        Err(_) => return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, TOO_LARGE)),
    };
    Ok(Some(ImageUpload {
        content_type,
        size: bytes.len(),
        bytes: bytes.to_vec(),
    }))
}

/// `PUT`: JPEG, PNG, WebP or GIF up to 5 MB → the playlist, now with `has_cover`.
async fn upload_cover(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
    request: Request,
) -> ApiResult<Json<PlaylistOut>> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    let upload = read_cover_upload(request)
        .await?
        .ok_or_else(|| ApiError::unprocessable("Keine Bilddatei im Feld \"image\""))?;
    store_cover(
        &state.db,
        &playlist,
        &upload.content_type,
        upload.size,
        &upload.bytes,
        &state.config.playlists_dir,
    )
    .await?;
    Ok(Json(current(&state, &user, playlist.id).await?))
}

async fn remove_cover(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let playlist = find(&state, &user, &raw_id, Need::Own).await?;
    clear_cover(&state.db, &playlist).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The playlist's tracks with the paths made relative to the root each lies under: the music
/// and books roots, and the folder the server keeps downloaded episodes in.
pub async fn tracks_for_export(state: &AppState, playlist_id: i32) -> ApiResult<Vec<ExportTrack>> {
    let paths = get_library_paths(&state.db).await?;
    let roots: Vec<String> = [
        paths.music_dir,
        paths.books_dir,
        Some(state.config.podcasts_dir.to_string_lossy().into_owned()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut tracks = export_tracks(&state.db, playlist_id).await?;
    for track in &mut tracks {
        track.path = relative_to_roots(&track.path, &roots);
    }
    Ok(tracks)
}

/// `attachment` with the name twice: plain ASCII for every client, and the real one
/// (RFC 6266 `filename*`) for those that read it.
pub fn attachment(file_name: &str) -> HeaderValue {
    let ascii: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = crate::web::format::encode_component(file_name);
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

#[derive(Serialize)]
struct ExportTrackOut {
    title: String,
    artist: Option<String>,
    album: String,
    #[serde(serialize_with = "js_number")]
    duration: f64,
    path: String,
}

/// `?format=m3u8` (the default) → an Extended M3U file to download; `?format=json` → the same
/// list for tools. Whole-item entries are resolved into their tracks.
async fn export(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    let playlist = find(&state, &user, &raw_id, Need::Read).await?;
    let format = query.get("format").map_or("m3u8", String::as_str);
    if !matches!(format, "m3u8" | "json") {
        return Err(ApiError::unprocessable("format muss m3u8 oder json sein"));
    }
    let tracks = tracks_for_export(&state, playlist.id).await?;
    if format == "json" {
        let tracks: Vec<ExportTrackOut> = tracks
            .into_iter()
            .map(|t| ExportTrackOut {
                title: t.title,
                artist: t.artist,
                album: t.album,
                duration: t.duration,
                path: t.path,
            })
            .collect();
        return Ok(Json(json!({
            "name": playlist.name,
            "description": playlist.description,
            "tracks": tracks,
        }))
        .into_response());
    }
    let file_name = format!("{}.m3u8", m3u::safe_file_name(&playlist.name));
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("audio/x-mpegurl; charset=utf-8"),
            ),
            (header::CONTENT_DISPOSITION, attachment(&file_name)),
        ],
        Body::from(m3u::write(&playlist.name, &tracks)),
    )
        .into_response())
}

/// `{name, content, folder?}` → 201 `{playlist, matched, unmatched}`: a new playlist of the
/// tracks of an M3U text that the collection has, in the file's order. What was not found comes
/// back as readable lines. A file of which nothing was found makes no playlist: 422 with the
/// same list, so the client can still show what was looked for.
async fn import(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Response> {
    let name = name_from(&body)?;
    let folder = text_field(&body, "folder", clean_folder, FOLDER_INVALID)?.flatten();
    let content = body
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::unprocessable("content muss der Text einer M3U-Datei sein"))?;
    if content.len() > m3u::MAX_IMPORT_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            IMPORT_TOO_LARGE,
        ));
    }
    let entries = m3u::parse(content);
    let mut matched = Vec::with_capacity(entries.len());
    let mut unmatched = Vec::new();
    if !entries.is_empty() {
        let index = m3u::TrackIndex::load(&state.db).await?;
        for entry in &entries {
            match index.find(entry) {
                Some(track_id) => matched.push(track_id),
                None => unmatched.push(entry.label()),
            }
        }
    }
    if matched.is_empty() {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "detail": NOTHING_MATCHED, "unmatched": unmatched })),
        )
            .into_response());
    }
    let playlist =
        create_with_tracks(&state.db, user.id, name, folder.as_deref(), &matched).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "playlist": current(&state, &user, playlist.id).await?,
            "matched": matched.len(),
            "unmatched": unmatched,
        })),
    )
        .into_response())
}

/// An id field of the entry body.
#[derive(Debug, PartialEq)]
enum IdField {
    /// Absent or `null`.
    Absent,
    Id(i32),
    /// Given, but no whole number in range: names nothing (legacy handed it to the database and
    /// failed with a 500).
    Broken,
}

fn id_field(body: &Map<String, Value>, key: &str) -> IdField {
    match body.get(key) {
        None | Some(Value::Null) => IdField::Absent,
        Some(value) => value
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= i32::MIN as f64 && *n <= i32::MAX as f64)
            .map_or(IdField::Broken, |n| IdField::Id(n as i32)),
    }
}

/// `{item_id}` or `{track_id}`, exactly one of them → the new entry at the end.
async fn add_entry(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
    body: JsonBody,
) -> ApiResult<Json<EntryOut>> {
    let playlist = find(&state, &user, &raw_id, Need::Edit).await?;
    let body: Map<String, Value> = body.parse()?;
    let unknown_item = || ApiError::not_found("Unbekanntes Item");
    let unknown_track = || ApiError::not_found("Unbekannter Track");
    let target = match (id_field(&body, "item_id"), id_field(&body, "track_id")) {
        (IdField::Absent, IdField::Absent) => None,
        (item, IdField::Absent) => {
            let IdField::Id(id) = item else {
                return Err(unknown_item());
            };
            let row = queries::item(&state.db, id)
                .await?
                .ok_or_else(unknown_item)?;
            Some(EntryTarget::Item(row.id))
        }
        (IdField::Absent, track) => {
            let IdField::Id(id) = track else {
                return Err(unknown_track());
            };
            let row = queries::track(&state.db, id)
                .await?
                .ok_or_else(unknown_track)?;
            Some(EntryTarget::Track(row.id))
        }
        _ => None,
    };
    let target = target
        .ok_or_else(|| ApiError::unprocessable("Genau eines von item_id/track_id angeben"))?;
    let created = append_entry(&state.db, playlist.id, target).await?;
    let resolved = resolve_entries(&state.db, vec![created]).await?;
    Ok(Json(EntryOut::from(&resolved[0])))
}

const POSITION_OUT_OF_RANGE: &str = "position außerhalb des gültigen Bereichs";

/// `{position}` (1-based) → 204. The entries in between shift by one.
async fn reorder_entry(
    State(state): State<AppState>,
    user: ApiUser,
    Path((raw_id, raw_entry)): Path<(String, String)>,
    body: JsonBody,
) -> ApiResult<StatusCode> {
    let playlist = find(&state, &user, &raw_id, Need::Edit).await?;
    let entry_id = path_id(&raw_entry, UNKNOWN_ENTRY)?;
    entry(&state.db, playlist.id, entry_id)
        .await?
        .ok_or_else(|| ApiError::not_found(UNKNOWN_ENTRY))?;
    let body: Map<String, Value> = body.parse()?;
    let count = entry_count(&state.db, playlist.id).await?;
    let position = body
        .get("position")
        .and_then(Value::as_f64)
        .filter(|p| p.fract() == 0.0 && *p >= 1.0 && *p <= count as f64)
        .ok_or_else(|| ApiError::unprocessable(POSITION_OUT_OF_RANGE))?;
    if !move_entry(&state.db, playlist.id, entry_id, position as i32).await? {
        return Err(ApiError::not_found(UNKNOWN_ENTRY));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_entry_route(
    State(state): State<AppState>,
    user: ApiUser,
    Path((raw_id, raw_entry)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let playlist = find(&state, &user, &raw_id, Need::Edit).await?;
    let entry_id = path_id(&raw_entry, UNKNOWN_ENTRY)?;
    if !remove_entry(&state.db, playlist.id, entry_id).await? {
        return Err(ApiError::not_found(UNKNOWN_ENTRY));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn names_count_like_js_length() {
        assert!(valid_name("a"));
        assert!(valid_name(&"x".repeat(100)));
        assert!(!valid_name(""));
        assert!(!valid_name(&"x".repeat(101)));
        // 50 emoji are 100 UTF-16 units: still fine; 51 are not.
        assert!(valid_name(&"😀".repeat(50)));
        assert!(!valid_name(&"😀".repeat(51)));
    }

    #[test]
    fn entry_ids_tell_absent_from_broken() {
        let b = body(json!({"item_id": 7, "track_id": null, "x": "1", "y": 1.5}));
        assert_eq!(id_field(&b, "item_id"), IdField::Id(7));
        assert_eq!(id_field(&b, "track_id"), IdField::Absent);
        assert_eq!(id_field(&b, "missing"), IdField::Absent);
        assert_eq!(id_field(&b, "x"), IdField::Broken);
        assert_eq!(id_field(&b, "y"), IdField::Broken);
    }
}
