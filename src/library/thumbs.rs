//! Scaled-down covers: rendered once, kept under `thumbs_dir`, and rendered ahead for the sizes
//! the grids and lists ask for, so a first visit does not wait for hundreds of decodes.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

/// The edge lengths a thumbnail is rendered at. A requested `size` snaps up to the next one, so
/// the cache holds a handful of files per image rather than one per pixel count a client asks for.
pub const THUMB_SIZES: &[u32] = &[128, 256, 384, 512, 768];

/// What the web UI shows in bulk: list rows, the start page, the grids. Larger sizes (detail
/// pages, the player) are one image at a time and rendered on request.
pub const AHEAD_SIZES: &[u32] = &[128, 256, 384];

fn cores() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get())
}

/// Decoding a several-megapixel cover is CPU work; a grid of 60 tiles must not start 60 at once.
static ON_REQUEST: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(cores()));

/// Where the thumbnail of `source` at `size` lives. Path, modification time and length go into
/// the name, so a replaced cover gets a new thumbnail instead of the stale one. `None` when
/// `source` cannot be read.
pub fn thumb_path(thumbs_dir: &Path, source: &Path, size: u32) -> Option<PathBuf> {
    let metadata = std::fs::metadata(source).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let mut hash = Sha256::new();
    hash.update(source.as_os_str().as_encoded_bytes());
    hash.update(modified.to_le_bytes());
    hash.update(metadata.len().to_le_bytes());
    let digest = hash.finalize();
    let hex: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    Some(thumbs_dir.join(format!("{hex}-{size}.jpg")))
}

/// The JPEG thumbnail of `source` at `size`, from the cache or rendered now. `None` when the
/// image is already that small or cannot be decoded — the caller serves the original then.
pub async fn thumbnail(source: &Path, size: u32, thumbs_dir: &Path) -> Option<Vec<u8>> {
    let target = thumb_path(thumbs_dir, source, size)?;
    if let Ok(data) = tokio::fs::read(&target).await {
        return Some(data);
    }
    let _permit = ON_REQUEST.acquire().await.expect("never closed");
    let source = source.to_path_buf();
    tokio::task::spawn_blocking(move || render(&source, &[(size, target)]).pop().flatten())
        .await
        .ok()
        .flatten()
}

/// Decodes `source` once and writes one JPEG per `(size, target)` (temp file + rename). Each
/// entry is the rendered bytes, or `None` where the image is already that small; all `None` when
/// it cannot be decoded. A failed write still returns the bytes.
pub fn render(source: &Path, targets: &[(u32, PathBuf)]) -> Vec<Option<Vec<u8>>> {
    let decoded = image::ImageReader::open(source)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.decode().ok());
    let Some(image) = decoded else {
        return vec![None; targets.len()];
    };
    targets
        .iter()
        .map(|(size, target)| {
            if image.width() <= *size && image.height() <= *size {
                return None;
            }
            let thumb = image.thumbnail(*size, *size).into_rgb8();
            let mut data = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 85)
                .encode_image(&thumb)
                .ok()?;
            if let Err(e) = write_atomically(target, &data) {
                tracing::warn!("Vorschaubild {} nicht gespeichert: {e}", target.display());
            }
            Some(data)
        })
        .collect()
}

fn write_atomically(target: &Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(target.parent().unwrap_or(Path::new(".")))?;
    let mut temp = target.as_os_str().to_owned();
    temp.push(".part");
    std::fs::write(&temp, data)?;
    std::fs::rename(&temp, target)
}

/// Renders the [`AHEAD_SIZES`] of every cover and uploaded artist image that lacks them.
/// Returns how many images were rendered. Uses half the cores, so requests keep their share.
pub async fn render_ahead(db: &PgPool, thumbs_dir: &Path) -> Result<usize, sqlx::Error> {
    let sources: Vec<String> = sqlx::query_scalar(
        "SELECT cover_path FROM items WHERE cover_path IS NOT NULL \
         UNION SELECT image_path FROM artist_covers WHERE image_path IS NOT NULL",
    )
    .fetch_all(db)
    .await?;
    let mut tasks = JoinSet::new();
    let mut rendered = 0;
    let limit = (cores() / 2).max(1);
    for source in sources {
        if tasks.len() >= limit {
            rendered += usize::from(tasks.join_next().await.and_then(Result::ok) == Some(true));
        }
        let thumbs_dir = thumbs_dir.to_path_buf();
        tasks.spawn_blocking(move || render_missing(Path::new(&source), &thumbs_dir));
    }
    while let Some(done) = tasks.join_next().await {
        rendered += usize::from(done.ok() == Some(true));
    }
    Ok(rendered)
}

/// Whether anything was rendered for `source`.
fn render_missing(source: &Path, thumbs_dir: &Path) -> bool {
    let missing: Vec<(u32, PathBuf)> = AHEAD_SIZES
        .iter()
        .filter_map(|&size| Some((size, thumb_path(thumbs_dir, source, size)?)))
        .filter(|(_, target)| !target.exists())
        .collect();
    // An image smaller than a size never gets that file; `render` decodes it again each run.
    // Cheap for small images, and those are the only ones it happens to.
    !missing.is_empty() && render(source, &missing).iter().any(Option::is_some)
}

static AHEAD_RUNNING: Mutex<()> = Mutex::const_new(());
static AHEAD_AGAIN: AtomicBool = AtomicBool::new(false);

/// Starts [`render_ahead`] in the background. A call while one runs makes that run go again
/// once it is done, instead of starting a second one beside it.
pub fn spawn_render_ahead(db: PgPool, thumbs_dir: PathBuf) {
    AHEAD_AGAIN.store(true, Ordering::SeqCst);
    tokio::spawn(async move {
        let Ok(_running) = AHEAD_RUNNING.try_lock() else {
            return;
        };
        while AHEAD_AGAIN.swap(false, Ordering::SeqCst) {
            match render_ahead(&db, &thumbs_dir).await {
                Ok(0) => {}
                Ok(n) => tracing::info!("Vorschaubilder für {n} Cover erstellt"),
                Err(e) => tracing::warn!("Vorschaubilder nicht erstellt: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_smaller_jpegs_and_leaves_small_sizes_alone() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("cover.png");
        image::RgbImage::new(1000, 500).save(&source).unwrap();
        let small = dir.path().join("thumbs/x-256.jpg");
        let large = dir.path().join("thumbs/x-1024.jpg");
        let out = render(&source, &[(256, small.clone()), (1024, large.clone())]);
        let thumb = image::load_from_memory(out[0].as_ref().unwrap()).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (256, 128));
        assert!(small.exists());
        assert_eq!(out[1], None);
        assert!(!large.exists());
    }

    #[test]
    fn a_changed_source_gets_a_new_thumbnail_name() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("cover.jpg");
        std::fs::write(&source, b"one").unwrap();
        let before = thumb_path(dir.path(), &source, 128).unwrap();
        std::fs::write(&source, b"other").unwrap();
        assert_ne!(thumb_path(dir.path(), &source, 128).unwrap(), before);
        assert_eq!(
            thumb_path(dir.path(), &dir.path().join("gone.jpg"), 128),
            None
        );
    }

    #[test]
    fn rendering_ahead_skips_what_exists() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("cover.png");
        image::RgbImage::new(800, 800).save(&source).unwrap();
        let thumbs = dir.path().join("thumbs");
        assert!(render_missing(&source, &thumbs));
        assert_eq!(
            std::fs::read_dir(&thumbs).unwrap().count(),
            AHEAD_SIZES.len()
        );
        assert!(!render_missing(&source, &thumbs));
    }
}
