//! `GET /items`, `GET /items/{id}`, `GET /items/{id}/cover`.

use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get};
use serde::Serialize;

use super::validate::{ApiQuery, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::paths::get_library_paths;
use crate::library::queries::{self, Item, ItemsFilter, SortKey};
use crate::serialize::{ChapterOut, ItemDetail, ItemSummary, TrackOut};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/items", get(list))
        .route("/items/{id}", get(detail))
        .route("/items/{id}/cover", get(cover))
}

/// JS `Number.MAX_SAFE_INTEGER`, legacy's upper bound for `offset`.
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Serialize)]
struct ItemsOut {
    items: Vec<ItemSummary>,
}

/// `kind`, `q`, `missing=true|false`, `limit` (1–500, default 200), `offset`, and
/// `sort=title|added|series|played`. An unknown sort is refused rather than read as `title`: a
/// sort that silently does something else is a bug that takes long to notice.
async fn list(
    State(state): State<AppState>,
    user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<ItemsOut>> {
    let non_empty = |name: &str| query.get(name).filter(|v| !v.is_empty()).cloned();
    let limit = int_param(&query, "limit", 200, 1, 500)?;
    let offset = int_param(&query, "offset", 0, 0, MAX_SAFE_INTEGER)?;
    let sort = match query.get("sort") {
        None => SortKey::Title,
        Some(raw) => {
            SortKey::parse(raw).ok_or_else(|| ApiError::unprocessable("Unbekannte Sortierung"))?
        }
    };
    let rows = queries::items(
        &state.db,
        &ItemsFilter {
            kind: non_empty("kind"),
            q: non_empty("q"),
            missing: query.get("missing").map(|m| m == "true"),
            sort: Some(sort),
            limit,
            offset,
            // Only `played` needs the progress join.
            played_by: (sort == SortKey::Played).then_some(user.id),
        },
    )
    .await?;
    Ok(Json(ItemsOut {
        items: rows.iter().map(ItemSummary::from).collect(),
    }))
}

/// The item behind a path segment; anything that is not a known id is 404 "Unbekanntes Item".
async fn find_item(state: &AppState, raw_id: &str) -> ApiResult<Item> {
    let not_found = || ApiError::not_found("Unbekanntes Item");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    queries::item(&state.db, id).await?.ok_or_else(not_found)
}

#[derive(Serialize)]
struct ItemOut {
    #[serde(flatten)]
    item: ItemDetail,
    tracks: Vec<TrackOut>,
    chapters: Vec<ChapterOut>,
}

async fn detail(
    State(state): State<AppState>,
    _user: ApiUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<ItemOut>> {
    let row = find_item(&state, &id).await?;
    let (tracks, chapters) = tokio::try_join!(
        queries::tracks(&state.db, row.id),
        queries::chapters(&state.db, row.id)
    )?;
    Ok(Json(ItemOut {
        item: ItemDetail::from(&row),
        tracks: tracks.iter().map(TrackOut::from).collect(),
        chapters: chapters.iter().map(ChapterOut::from).collect(),
    }))
}

fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}

/// Absolute and with `.`/`..` folded away, without touching the filesystem (`path.resolve`).
fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Whether `path` lies inside `root` once both are resolved.
pub fn is_inside(path: &Path, root: &Path) -> bool {
    resolve(path).starts_with(resolve(root))
}

/// The cover image. Only files under a library root or the server's cover directory are served,
/// whatever `cover_path` says.
async fn cover(
    State(state): State<AppState>,
    _user: ApiUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Response> {
    let no_cover = || ApiError::not_found("Kein Cover");
    let row = find_item(&state, &id).await?;
    let cover_path = queries::cover_path_for(&state.db, &row)
        .await?
        .ok_or_else(no_cover)?;
    let cover_path = PathBuf::from(cover_path);
    let paths = get_library_paths(&state.db).await?;
    let roots = [
        paths.books_dir.map(PathBuf::from),
        paths.music_dir.map(PathBuf::from),
        Some(state.config.cover_dir.clone()),
    ];
    if !roots
        .iter()
        .flatten()
        .any(|root| is_inside(&cover_path, root))
    {
        return Err(no_cover());
    }
    let data = tokio::fs::read(&cover_path).await.map_err(|_| no_cover())?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type(&cover_path)),
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        Body::from(data),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_check_folds_parent_components() {
        let root = Path::new("/data/covers");
        assert!(is_inside(Path::new("/data/covers/1.jpg"), root));
        assert!(!is_inside(Path::new("/data/covers/../secret"), root));
        assert!(!is_inside(Path::new("/etc/passwd"), root));
        assert!(!is_inside(Path::new("/data/covers2/1.jpg"), root));
    }
}
