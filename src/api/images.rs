//! Serving image files from disk (item covers, artist pictures) — only from directories the
//! server owns or was told about, whatever a stored path says.

use std::path::{Component, Path, PathBuf};

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use super::validate::{QueryMap, int_param};
use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use crate::library::paths::get_library_paths;
use crate::library::thumbs::{THUMB_SIZES, thumbnail};

/// `?size=` as a thumbnail edge length: `None` (the original) when absent or larger than the
/// largest thumbnail, else the smallest size in [`THUMB_SIZES`] that is at least as large.
pub fn thumb_size(query: &QueryMap) -> ApiResult<Option<u32>> {
    let requested = int_param(query, "size", 0, 0, 4096)?;
    if requested == 0 {
        return Ok(None);
    }
    Ok(THUMB_SIZES
        .iter()
        .copied()
        .find(|&size| i64::from(size) >= requested))
}

/// The media type of a stored image, by its extension.
pub fn content_type(path: &Path) -> &'static str {
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
pub fn resolve(path: &Path) -> PathBuf {
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

/// Where a scanned or extracted cover may live: under a library root or the cover directory.
pub async fn cover_roots(db: &PgPool, config: &Config) -> Result<Vec<PathBuf>, sqlx::Error> {
    let paths = get_library_paths(db).await?;
    Ok([
        paths.books_dir.map(PathBuf::from),
        paths.music_dir.map(PathBuf::from),
        Some(config.cover_dir.clone()),
    ]
    .into_iter()
    .flatten()
    .collect())
}

/// The file at `path` if it lies under one of `roots` and can be read; 404 with `missing`
/// otherwise. With a `size`, a JPEG (a PNG where the picture is transparent) no larger than
/// `size`×`size` instead, rendered once and kept under `thumbs_dir`; the original whenever it is
/// already that small or cannot be decoded.
pub async fn serve_image(
    path: &Path,
    roots: &[PathBuf],
    missing: &str,
    cache_control: &'static str,
    size: Option<u32>,
    thumbs_dir: &Path,
    request: &HeaderMap,
) -> ApiResult<Response> {
    let not_found = || ApiError::not_found(missing);
    if !roots.iter().any(|root| is_inside(path, root)) {
        return Err(not_found());
    }
    if let Some(size) = size
        && let Some((data, content_type)) = thumbnail(path, size, thumbs_dir).await
    {
        return Ok(image_response(content_type, cache_control, data, request));
    }
    let data = tokio::fs::read(path).await.map_err(|_| not_found())?;
    Ok(image_response(
        content_type(path),
        cache_control,
        data,
        request,
    ))
}

/// The validator of an image: its content, so a client asks again cheaply and learns of a change.
fn etag_of(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("\"{hex}\"")
}

/// An image with its validator, or `304` when the request names that validator.
pub fn image_response(
    content_type: &'static str,
    cache_control: &'static str,
    data: impl Into<Bytes>,
    request: &HeaderMap,
) -> Response {
    let data: Bytes = data.into();
    let etag = etag_of(&data);
    let unchanged = request
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|candidate| candidate.trim() == etag));
    if unchanged {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, cache_control.to_owned()),
            ],
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (header::CACHE_CONTROL, cache_control.to_owned()),
            (header::ETAG, etag),
        ],
        Body::from(data),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_image_answers_304_without_a_body() {
        let tag = etag_of(b"bild");
        let mut request = HeaderMap::new();
        request.insert(header::IF_NONE_MATCH, tag.parse().unwrap());
        let response = image_response("image/jpeg", "private", b"bild".to_vec(), &request);
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        let changed = image_response("image/jpeg", "private", b"neu".to_vec(), &request);
        assert_eq!(changed.status(), StatusCode::OK);
        assert_ne!(changed.headers()[header::ETAG].to_str().unwrap(), tag);
    }

    #[test]
    fn snaps_a_requested_size_up_to_the_next_thumbnail_size() {
        let q = |v: &str| QueryMap::from([("size".to_string(), v.to_string())]);
        assert_eq!(thumb_size(&QueryMap::new()).unwrap(), None);
        assert_eq!(thumb_size(&q("100")).unwrap(), Some(128));
        assert_eq!(thumb_size(&q("256")).unwrap(), Some(256));
        assert_eq!(thumb_size(&q("370")).unwrap(), Some(384));
        assert_eq!(thumb_size(&q("2000")).unwrap(), None);
        assert!(thumb_size(&q("-1")).is_err());
    }

    #[test]
    fn inside_check_folds_parent_components() {
        let root = Path::new("/data/covers");
        assert!(is_inside(Path::new("/data/covers/1.jpg"), root));
        assert!(!is_inside(Path::new("/data/covers/../secret"), root));
        assert!(!is_inside(Path::new("/etc/passwd"), root));
        assert!(!is_inside(Path::new("/data/covers2/1.jpg"), root));
    }

    #[test]
    fn maps_stored_extensions_back_to_their_media_type() {
        assert_eq!(
            content_type(Path::new("/data/artists/abc.png")),
            "image/png"
        );
        assert_eq!(
            content_type(Path::new("/data/artists/abc.JPG")),
            "image/jpeg"
        );
        assert_eq!(
            content_type(Path::new("/data/artists/abc.txt")),
            "application/octet-stream"
        );
    }
}
