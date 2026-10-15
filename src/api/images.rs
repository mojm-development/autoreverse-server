//! Serving image files from disk (item covers, artist pictures) — only from directories the
//! server owns or was told about, whatever a stored path says.

use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use axum::body::Body;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::Semaphore;

use super::validate::{QueryMap, int_param};
use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use crate::library::paths::get_library_paths;

/// The edge lengths a thumbnail is rendered at. A requested `size` snaps up to the next one, so
/// the cache holds a handful of files per image rather than one per pixel count a client asks for.
pub const THUMB_SIZES: &[u32] = &[128, 256, 384, 512, 768];

/// Decoding a several-megapixel cover is CPU work; a grid of 60 tiles must not start 60 at once.
static THUMB_RENDERS: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(std::thread::available_parallelism().map_or(2, |n| n.get())));

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
/// otherwise. With a `size`, a JPEG no larger than `size`×`size` instead, rendered once and kept
/// under `thumbs_dir`; the original whenever it is already that small or cannot be decoded.
pub async fn serve_image(
    path: &Path,
    roots: &[PathBuf],
    missing: &str,
    cache_control: &'static str,
    size: Option<u32>,
    thumbs_dir: &Path,
) -> ApiResult<Response> {
    let not_found = || ApiError::not_found(missing);
    if !roots.iter().any(|root| is_inside(path, root)) {
        return Err(not_found());
    }
    if let Some(size) = size {
        let metadata = tokio::fs::metadata(path).await.map_err(|_| not_found())?;
        let thumb = thumbs_dir.join(thumb_name(path, &metadata, size));
        if let Ok(data) = tokio::fs::read(&thumb).await {
            return Ok(image_response("image/jpeg", cache_control, data));
        }
        let rendered = {
            let _permit = THUMB_RENDERS.acquire().await.expect("never closed");
            let (source, target) = (path.to_path_buf(), thumb.clone());
            tokio::task::spawn_blocking(move || render_thumb(&source, &target, size))
                .await
                .ok()
                .flatten()
        };
        if let Some(data) = rendered {
            return Ok(image_response("image/jpeg", cache_control, data));
        }
    }
    let data = tokio::fs::read(path).await.map_err(|_| not_found())?;
    Ok(image_response(content_type(path), cache_control, data))
}

fn image_response(
    content_type: &'static str,
    cache_control: &'static str,
    data: Vec<u8>,
) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        Body::from(data),
    )
        .into_response()
}

/// The thumbnail's file name: the source path, its modification time and length go into the
/// hash, so a replaced cover gets a new thumbnail instead of the stale one.
fn thumb_name(path: &Path, metadata: &std::fs::Metadata, size: u32) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let mut hash = Sha256::new();
    hash.update(path.as_os_str().as_encoded_bytes());
    hash.update(modified.to_le_bytes());
    hash.update(metadata.len().to_le_bytes());
    let digest = hash.finalize();
    let hex: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}-{size}.jpg")
}

/// Decodes `source`, scales it to fit `size`×`size` and writes it to `target` as a JPEG
/// (temp file + rename). `None` when the image is already small enough or cannot be decoded —
/// the caller serves the original then. A failed write still returns the rendered bytes.
fn render_thumb(source: &Path, target: &Path, size: u32) -> Option<Vec<u8>> {
    let image = image::ImageReader::open(source)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    if image.width() <= size && image.height() <= size {
        return None;
    }
    let thumb = image.thumbnail(size, size).into_rgb8();
    let mut data = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 85)
        .encode_image(&thumb)
        .ok()?;
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(target.parent().unwrap_or(Path::new(".")))?;
        let mut temp = target.as_os_str().to_owned();
        temp.push(".part");
        std::fs::write(&temp, &data)?;
        std::fs::rename(&temp, target)
    };
    if let Err(e) = write() {
        tracing::warn!("Vorschaubild {} nicht gespeichert: {e}", target.display());
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn renders_a_smaller_jpeg_and_leaves_small_images_alone() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("cover.png");
        image::RgbImage::new(1000, 500).save(&source).unwrap();
        let target = dir.path().join("thumbs/x-256.jpg");
        let data = render_thumb(&source, &target, 256).unwrap();
        let thumb = image::load_from_memory(&data).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (256, 128));
        assert!(target.exists());
        assert_eq!(render_thumb(&source, &target, 1024), None);
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
