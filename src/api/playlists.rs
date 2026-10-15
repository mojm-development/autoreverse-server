//! `GET|POST /playlists`, `GET|PATCH|DELETE /playlists/{id}`, `POST /playlists/{id}/entries`,
//! `PUT|DELETE /playlists/{id}/entries/{entryId}` (legacy `routes/playlists/**`).
//!
//! A user only ever sees their own playlists: someone else's is "Unbekannte Playlist", the same
//! 404 as one that does not exist — for admins too.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing::get, routing::post, routing::put};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};

use super::validate::{ApiJson, JsonBody};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::playlists::{
    EntryTarget, Playlist, ResolvedEntry, append_entry, create_playlist, delete_playlist, entry,
    entry_count, list_entries, list_playlists, move_entry, owned_playlist, remove_entry,
    rename_playlist, resolve_entries,
};
use crate::library::queries;
use crate::serialize::{ItemSummary, TrackSummary, iso};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/playlists", get(list).post(create))
        .route("/playlists/{id}", get(detail).patch(rename).delete(remove))
        .route("/playlists/{id}/entries", post(add_entry))
        .route(
            "/playlists/{id}/entries/{entry_id}",
            put(reorder_entry).delete(remove_entry_route),
        )
}

pub const UNKNOWN_PLAYLIST: &str = "Unbekannte Playlist";
pub const UNKNOWN_ENTRY: &str = "Unbekannter Eintrag";
pub const NAME_INVALID: &str = "name muss 1–100 Zeichen haben";

#[derive(Serialize)]
struct PlaylistOut {
    id: i32,
    name: String,
    #[serde(serialize_with = "iso")]
    created_at: DateTime<Utc>,
    entry_count: i64,
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
    id: i32,
    name: String,
    #[serde(serialize_with = "iso")]
    created_at: DateTime<Utc>,
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

/// A path id; anything that is not one names no playlist (or entry).
fn path_id(raw: &str, detail: &'static str) -> ApiResult<i32> {
    raw.parse().map_err(|_| ApiError::not_found(detail))
}

async fn own(state: &AppState, user: &ApiUser, raw_id: &str) -> ApiResult<Playlist> {
    let id = path_id(raw_id, UNKNOWN_PLAYLIST)?;
    owned_playlist(&state.db, user.id, id)
        .await?
        .ok_or_else(|| ApiError::not_found(UNKNOWN_PLAYLIST))
}

async fn list(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<PlaylistsOut>> {
    let rows = list_playlists(&state.db, user.id).await?;
    Ok(Json(PlaylistsOut {
        playlists: rows
            .into_iter()
            .map(|p| PlaylistOut {
                id: p.id,
                name: p.name,
                created_at: p.created_at,
                entry_count: p.entry_count,
            })
            .collect(),
    }))
}

/// `{name}` → the new playlist. 200 like legacy, not 201.
async fn create(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Json<PlaylistOut>> {
    let name = name_from(&body)?;
    let row = create_playlist(&state.db, user.id, name).await?;
    Ok(Json(PlaylistOut {
        id: row.id,
        name: row.name,
        created_at: row.created_at,
        entry_count: 0,
    }))
}

async fn detail(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<PlaylistDetailOut>> {
    let playlist = own(&state, &user, &raw_id).await?;
    let entries = resolve_entries(&state.db, list_entries(&state.db, playlist.id).await?).await?;
    Ok(Json(PlaylistDetailOut {
        id: playlist.id,
        name: playlist.name,
        created_at: playlist.created_at,
        entries: entries.iter().map(EntryOut::from).collect(),
    }))
}

async fn rename(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
    body: JsonBody,
) -> ApiResult<Json<PlaylistOut>> {
    let playlist = own(&state, &user, &raw_id).await?;
    let body: Map<String, Value> = body.parse()?;
    let name = name_from(&body)?;
    let row = rename_playlist(&state.db, playlist.id, name).await?;
    let count = entry_count(&state.db, row.id).await?;
    Ok(Json(PlaylistOut {
        id: row.id,
        name: row.name,
        created_at: row.created_at,
        entry_count: count,
    }))
}

async fn remove(
    State(state): State<AppState>,
    user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let playlist = own(&state, &user, &raw_id).await?;
    delete_playlist(&state.db, playlist.id).await?;
    Ok(StatusCode::NO_CONTENT)
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
    let playlist = own(&state, &user, &raw_id).await?;
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
    let playlist = own(&state, &user, &raw_id).await?;
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
    let playlist = own(&state, &user, &raw_id).await?;
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
