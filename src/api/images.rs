//! Serving image files from disk (item covers, artist pictures) — only from directories the
//! server owns or was told about, whatever a stored path says.

use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use sqlx::PgPool;

use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use crate::library::paths::get_library_paths;

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
/// otherwise.
pub async fn serve_image(
    path: &Path,
    roots: &[PathBuf],
    missing: &str,
    cache_control: &'static str,
) -> ApiResult<Response> {
    let not_found = || ApiError::not_found(missing);
    if !roots.iter().any(|root| is_inside(path, root)) {
        return Err(not_found());
    }
    let data = tokio::fs::read(path).await.map_err(|_| not_found())?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type(path)),
            (header::CACHE_CONTROL, cache_control),
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
