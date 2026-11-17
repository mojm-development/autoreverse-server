//! A smaller copy of a track for a slow connection: `GET /tracks/{id}/stream?quality=…&format=…`.
//!
//! The library is mostly FLAC, six to eight times the bytes a phone on mobile data needs. A
//! client that asks for a quality gets the file re-encoded by `ffmpeg` — but as a **whole file**
//! from a cache, never as a pipe: the players seek with byte ranges (a book resumes in the
//! middle, AVPlayer reads the `moov` atom first), and a range needs a length. So the first
//! request for a track in a quality waits until its copy is written, and every later one is an
//! ordinary range response. A song takes two or three seconds; a client that opens the following
//! title ahead of time (gapless) warms it without knowing.
//!
//! Not every file is worth it: an audiobook in 64 kbit/s MP3 would only get worse. Where the
//! source is not clearly above the target ([`worth_it`]) the original goes out, under its own
//! `Content-Type` — the client never has to know which of the two it got. The same happens when
//! `ffmpeg` fails or is missing: slow music beats none.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tokio::sync::{Mutex as AsyncMutex, Semaphore};

/// How far down. The names are the wire values of `quality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quality {
    High,
    Medium,
    Low,
}

impl Quality {
    pub fn kbps(self) -> u32 {
        match self {
            Quality::High => 192,
            Quality::Medium => 128,
            Quality::Low => 64,
        }
    }
}

/// What it is encoded to. Opus for everything that plays it (Android, the desktop); AAC in an
/// MP4 for AVPlayer, which does not read Ogg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Opus,
    Aac,
}

impl Format {
    /// The cache file's extension, which is also what names its `Content-Type`
    /// (`ranges::media_type`).
    fn extension(self) -> &'static str {
        match self {
            Format::Opus => "opus",
            Format::Aac => "m4a",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Target {
    pub quality: Quality,
    pub format: Format,
}

/// A value of `quality` or `format` nobody knows: 422, like an unknown `sort`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownValue(pub &'static str);

/// What the query asks for. No `quality`, or `original`, is the file as it is — whatever
/// `format` says. Without a `format` it is Opus.
pub fn parse_target(
    quality: Option<&str>,
    format: Option<&str>,
) -> Result<Option<Target>, UnknownValue> {
    let quality = match quality {
        None | Some("" | "original") => return Ok(None),
        Some("high") => Quality::High,
        Some("medium") => Quality::Medium,
        Some("low") => Quality::Low,
        Some(_) => return Err(UnknownValue("quality")),
    };
    let format = match format {
        None | Some("" | "opus") => Format::Opus,
        Some("aac") => Format::Aac,
        Some(_) => return Err(UnknownValue("format")),
    };
    Ok(Some(Target { quality, format }))
}

/// A source has to be this many times the target's bitrate before a copy is made. Below that
/// the saving is small and a second lossy encoding costs more than it brings.
const WORTH_IT_FACTOR: f64 = 1.5;

/// Whether re-encoding saves enough. The source's bitrate is its size over its length — the
/// cover inside a file counts as audio that way, which only errs towards the copy. A length
/// nobody knows says nothing, and then the original goes out.
pub fn worth_it(size: u64, duration: f64, quality: Quality) -> bool {
    if !duration.is_finite() || duration <= 0.0 {
        return false;
    }
    let source_kbps = size as f64 * 8.0 / duration / 1000.0;
    source_kbps > f64::from(quality.kbps()) * WORTH_IT_FACTOR
}

/// The copies kept on disk, together. Past this the ones not asked for longest go.
pub const DEFAULT_CACHE_BYTES: u64 = 10 * 1024 * 1024 * 1024;
/// ffmpeg uses a core each; two leave the server answering while a queue warms up.
const MAX_CONCURRENT: usize = 2;

#[derive(Debug)]
pub enum TranscodeError {
    /// `ffmpeg` cannot be started.
    NoEncoder(String),
    /// It ran and did not produce a file.
    Failed(String),
    Io(std::io::Error),
}

impl std::fmt::Display for TranscodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranscodeError::NoEncoder(why) => write!(f, "ffmpeg lässt sich nicht starten: {why}"),
            TranscodeError::Failed(why) => write!(f, "ffmpeg ist gescheitert: {why}"),
            TranscodeError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<std::io::Error> for TranscodeError {
    fn from(e: std::io::Error) -> Self {
        TranscodeError::Io(e)
    }
}

/// The cache of re-encoded tracks and the work that fills it. One per process (`AppState`).
#[derive(Clone)]
pub struct Transcoder {
    inner: Arc<Inner>,
}

struct Inner {
    /// One lock per copy being written, so that two requests for the same one (a player opens
    /// several connections) wait for a single ffmpeg instead of starting two.
    running: Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>,
    slots: Semaphore,
    max_bytes: u64,
}

impl Default for Transcoder {
    fn default() -> Self {
        Transcoder::with_limit(DEFAULT_CACHE_BYTES)
    }
}

impl Transcoder {
    pub fn with_limit(max_bytes: u64) -> Self {
        Transcoder {
            inner: Arc::new(Inner {
                running: Mutex::default(),
                slots: Semaphore::new(MAX_CONCURRENT),
                max_bytes,
            }),
        }
    }

    /// The copy of `source` in `target`, made now unless it is there. `dir` is the cache.
    ///
    /// The name carries the source's size and modification time: a file that was replaced gets
    /// a new copy, and the old one ages out like any other.
    ///
    /// The encoding runs in a task of its own. A request that goes away halfway — a driver
    /// tapping the next title — must not throw the work away: the same title is asked for again
    /// a moment later, by the retry or by the queue.
    pub async fn ensure(
        &self,
        dir: &Path,
        track_id: i32,
        source: &Path,
        target: Target,
    ) -> Result<PathBuf, TranscodeError> {
        let meta = tokio::fs::metadata(source).await?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let name = format!(
            "{track_id}-{}-{modified}-{}.{}",
            meta.len(),
            target.quality.kbps(),
            target.format.extension()
        );
        let copy = dir.join(name);
        let this = self.clone();
        let (dir, source) = (dir.to_owned(), source.to_owned());
        tokio::spawn(async move { this.produce(&dir, &source, copy, target).await })
            .await
            .map_err(|e| TranscodeError::Failed(e.to_string()))?
    }

    async fn produce(
        &self,
        dir: &Path,
        source: &Path,
        copy: PathBuf,
        target: Target,
    ) -> Result<PathBuf, TranscodeError> {
        let lock = {
            let mut running = self.inner.running.lock().unwrap_or_else(|e| e.into_inner());
            running.entry(copy.clone()).or_default().clone()
        };
        let result = {
            let _one_at_a_time = lock.lock().await;
            if is_file(&copy).await {
                touch(&copy).await;
                Ok(())
            } else {
                self.encode(dir, source, &copy, target).await
            }
        };
        {
            let mut running = self.inner.running.lock().unwrap_or_else(|e| e.into_inner());
            // Only when nobody else holds it: a waiter still needs the same lock.
            if running
                .get(&copy)
                .is_some_and(|l| Arc::strong_count(l) <= 2)
            {
                running.remove(&copy);
            }
        }
        result.map(|()| copy)
    }

    async fn encode(
        &self,
        dir: &Path,
        source: &Path,
        copy: &Path,
        target: Target,
    ) -> Result<(), TranscodeError> {
        let _slot = self
            .inner
            .slots
            .acquire()
            .await
            .map_err(|e| TranscodeError::Failed(e.to_string()))?;
        tokio::fs::create_dir_all(dir).await?;
        // Written beside its final name and moved there whole: a reader never sees half a file,
        // and what a crash leaves behind is recognisable (`prune` removes it).
        let partial = copy.with_extension(format!(
            "{}.part",
            copy.extension().and_then(|e| e.to_str()).unwrap_or("bin")
        ));
        let bitrate = format!("{}k", target.quality.kbps());
        let mut command = tokio::process::Command::new("ffmpeg");
        command
            .args(["-v", "error", "-nostdin", "-y", "-i"])
            .arg(source)
            // The first audio stream only (a cover is a video stream), stereo at most: a
            // phone in a car has two speakers' worth of use for it.
            .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-map_metadata", "-1"])
            .args(["-ac", "2", "-b:a", &bitrate]);
        // Both encoders at a faster setting than their default: the listener waits for this
        // file, and at these bitrates the difference is in the seconds, not in the sound
        // (measured on a 3:47 FLAC: Opus 3.0 → 2.0 s, AAC 5.5 → 2.7 s).
        match target.format {
            Format::Opus => command.args([
                "-c:a",
                "libopus",
                "-vbr",
                "on",
                "-compression_level",
                "5",
                "-f",
                "ogg",
            ]),
            // `faststart` puts the index in front, so a player can begin before it has the end.
            Format::Aac => command.args([
                "-c:a",
                "aac",
                "-aac_coder",
                "fast",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ]),
        };
        let output = command
            .arg(&partial)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| TranscodeError::NoEncoder(e.to_string()))?;
        let written = tokio::fs::metadata(&partial)
            .await
            .is_ok_and(|m| m.len() > 0);
        if !output.status.success() || !written {
            let _ = tokio::fs::remove_file(&partial).await;
            let why = String::from_utf8_lossy(&output.stderr);
            return Err(TranscodeError::Failed(
                why.lines().last().unwrap_or("ohne Meldung").to_owned(),
            ));
        }
        tokio::fs::rename(&partial, copy).await?;
        let (dir, max_bytes) = (dir.to_owned(), self.inner.max_bytes);
        let keep = copy.to_owned();
        // Not awaited: the listener waits for the song, not for the housekeeping.
        tokio::task::spawn_blocking(move || prune(&dir, max_bytes, Some(&keep)));
        Ok(())
    }
}

async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok_and(|m| m.is_file())
}

/// Marks a copy as asked for just now; `prune` goes by this. A file system that refuses is no
/// reason to fail a request — the copy then ages as if it had not been used.
async fn touch(path: &Path) {
    let path = path.to_owned();
    let _ = tokio::task::spawn_blocking(move || {
        std::fs::File::options()
            .append(true)
            .open(&path)
            .and_then(|file| file.set_modified(SystemTime::now()))
    })
    .await;
}

/// Deletes the copies used longest ago until the rest fits into `max_bytes`, and every
/// leftover of an encoding that never finished and is older than an hour. `keep` is the one
/// just written, which stays even if it alone is over the limit. Returns the bytes freed.
pub fn prune(dir: &Path, max_bytes: u64, keep: Option<&Path>) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut freed = 0;
    let mut copies: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if path.extension().is_some_and(|e| e == "part") {
            let stale = modified
                .elapsed()
                .is_ok_and(|age| age > std::time::Duration::from_secs(3600));
            if stale && std::fs::remove_file(&path).is_ok() {
                freed += meta.len();
            }
            continue;
        }
        copies.push((modified, meta.len(), path));
    }
    let mut total: u64 = copies.iter().map(|(_, size, _)| size).sum();
    copies.sort();
    for (_, size, path) in copies {
        if total <= max_bytes {
            break;
        }
        if keep.is_some_and(|k| k == path) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
            freed += size;
        }
    }
    freed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_quality_means_the_file_as_it_is() {
        assert_eq!(parse_target(None, None), Ok(None));
        assert_eq!(parse_target(Some("original"), Some("aac")), Ok(None));
        assert_eq!(parse_target(Some(""), None), Ok(None));
        // Even a format nobody knows: nothing is encoded, so nothing can go wrong with it.
        assert_eq!(parse_target(None, Some("wma")), Ok(None));
    }

    #[test]
    fn a_quality_comes_as_opus_unless_aac_is_asked_for() {
        let opus = |quality| {
            Ok(Some(Target {
                quality,
                format: Format::Opus,
            }))
        };
        assert_eq!(parse_target(Some("high"), None), opus(Quality::High));
        assert_eq!(
            parse_target(Some("medium"), Some("opus")),
            opus(Quality::Medium)
        );
        assert_eq!(
            parse_target(Some("low"), Some("aac")),
            Ok(Some(Target {
                quality: Quality::Low,
                format: Format::Aac
            }))
        );
    }

    #[test]
    fn unknown_values_are_named() {
        assert_eq!(
            parse_target(Some("best"), None),
            Err(UnknownValue("quality"))
        );
        assert_eq!(
            parse_target(Some("low"), Some("mp3")),
            Err(UnknownValue("format"))
        );
    }

    #[test]
    fn only_a_source_clearly_above_the_target_is_encoded() {
        // Four minutes of FLAC at about 900 kbit/s.
        assert!(worth_it(27_000_000, 240.0, Quality::High));
        // An hour of an audiobook in 64 kbit/s MP3: no quality improves on that.
        let book = 64_000 / 8 * 3600;
        assert!(!worth_it(book, 3600.0, Quality::Low));
        assert!(!worth_it(book, 3600.0, Quality::Medium));
        // 128 kbit/s MP3 is worth going to 64 for, not to 128.
        let mp3 = 128_000 / 8 * 240;
        assert!(worth_it(mp3, 240.0, Quality::Low));
        assert!(!worth_it(mp3, 240.0, Quality::Medium));
    }

    #[test]
    fn a_length_nobody_knows_keeps_the_original() {
        assert!(!worth_it(27_000_000, 0.0, Quality::Low));
        assert!(!worth_it(27_000_000, f64::NAN, Quality::Low));
    }

    fn file(dir: &Path, name: &str, bytes: usize, age_secs: u64) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![0u8; bytes]).unwrap();
        let when = SystemTime::now() - std::time::Duration::from_secs(age_secs);
        std::fs::File::options()
            .append(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
        path
    }

    #[test]
    fn the_copies_used_longest_ago_go_first() {
        let dir = tempfile::tempdir().unwrap();
        let old = file(dir.path(), "1-1-1-64.opus", 400, 300);
        let middle = file(dir.path(), "2-1-1-64.opus", 400, 200);
        let new = file(dir.path(), "3-1-1-64.opus", 400, 100);
        assert_eq!(prune(dir.path(), 900, None), 400);
        assert!(!old.exists());
        assert!(middle.exists() && new.exists());
    }

    #[test]
    fn the_one_just_written_stays_even_alone_over_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = file(dir.path(), "1-1-1-192.m4a", 5000, 0);
        assert_eq!(prune(dir.path(), 100, Some(&fresh)), 0);
        assert!(fresh.exists());
    }

    #[test]
    fn only_an_abandoned_leftover_is_cleared_away() {
        let dir = tempfile::tempdir().unwrap();
        let abandoned = file(dir.path(), "1-1-1-64.opus.part", 10, 7200);
        let in_progress = file(dir.path(), "2-1-1-64.opus.part", 10, 5);
        prune(dir.path(), u64::MAX, None);
        assert!(!abandoned.exists());
        assert!(in_progress.exists());
    }
}
