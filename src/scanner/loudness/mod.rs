//! Loudness normalisation: every music track gets the gain that brings it to [`TARGET_LUFS`],
//! the same for its album as a whole, and its peak. Clients turn the volume by that.
//!
//! Two sources, in this order:
//!
//! 1. Tags a tool already wrote ([`tags`]). Reading them is cheap, so it happens inside the
//!    scan ([`read_pending_tags`]), once per version of a file — `loudness_state` remembers
//!    that it was done, and `scanner::store` forgets it when the file's size or mtime change.
//!    That is also how the tracks from before the columns existed get their values: their
//!    state is NULL.
//! 2. Measuring ([`measure`]), where the tags leave something open. That decodes every file
//!    once in full, so it is a pass of its own in the background ([`start`]), after the scan
//!    and at every server start. It works album by album: an album's value needs all of its
//!    tracks anyway, and writing each album in one transaction is what makes the pass
//!    resumable — a restart finds the albums that are still `pending` and nothing else.
//!
//! Only music is measured. Audiobooks and podcast episodes get whatever their tags say (which
//! costs nothing) and are otherwise left alone: hundreds of hours to decode for a value no
//! client applies to spoken word.
//!
//! A file that cannot be decoded becomes `failed` and is not tried again until it changes.

pub mod measure;
pub mod tags;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::{StreamExt, stream};
use rayon::prelude::*;
use serde::Serialize;
use sqlx::PgPool;

use self::measure::{MeasureError, Measured, album_loudness, measure_file};
use crate::state::AppState;

/// The loudness every gain aims at: ReplayGain 2's reference. Low enough that nearly all
/// music is turned *down* to reach it — a client that can only attenuate (the web player's
/// `audio.volume`) still levels almost everything.
pub const TARGET_LUFS: f64 = -18.0;

/// How far the background pass has come, for `/scan/status` and the scan card. Counted in
/// albums, the unit the pass works in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LoudnessProgress {
    pub running: bool,
    /// Albums this pass set out to measure.
    pub total: u32,
    pub processed: u32,
    /// Files of this pass that could not be decoded.
    pub failed: u32,
}

#[derive(Debug, Default)]
struct Inner {
    progress: Mutex<LoudnessProgress>,
    /// Set at shutdown: decoders are killed, nothing more is started.
    stop: AtomicBool,
    /// A scan finished while a pass was running: go round once more when it is through.
    again: AtomicBool,
}

/// The one background pass of this process; cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct LoudnessJob(Arc<Inner>);

impl LoudnessJob {
    pub fn snapshot(&self) -> LoudnessProgress {
        *self.lock()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LoudnessProgress> {
        self.0
            .progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Stops the pass for good (server shutdown). What is done stays done; the albums in work
    /// are simply still `pending` at the next start.
    pub fn stop(&self) {
        self.0.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.0.stop.load(Ordering::Relaxed)
    }

    /// Claims the pass: false if one is running — it is then told to go round again.
    fn try_start(&self) -> bool {
        let mut progress = self.lock();
        if progress.running {
            self.0.again.store(true, Ordering::Relaxed);
            return false;
        }
        *progress = LoudnessProgress {
            running: true,
            ..Default::default()
        };
        true
    }
}

/// How many albums are decoded at once when `AUTOREVERSE_LOUDNESS_THREADS` does not say: half
/// the cores, four at most — the server is there to play music meanwhile.
pub fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    (cores / 2).clamp(1, 4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TagReport {
    /// Files whose tags were read.
    pub read: u64,
    /// Of those, music tracks that still need measuring.
    pub pending: u64,
}

/// Reads the loudness tags of every track nobody has looked at yet (`loudness_state IS NULL`).
/// A file that cannot be opened keeps its NULL and is looked at again next time: not seeing
/// is not knowing. Spoken word is `done` whatever its tags say — it is never measured.
pub async fn read_pending_tags(
    db: &PgPool,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<TagReport, sqlx::Error> {
    let rows: Vec<(i32, String, bool)> = sqlx::query_as(
        "SELECT t.id, t.path, i.kind = 'album'
         FROM tracks t JOIN items i ON i.id = t.item_id
         WHERE t.loudness_state IS NULL ORDER BY t.id",
    )
    .fetch_all(db)
    .await?;
    if rows.is_empty() {
        return Ok(TagReport::default());
    }
    let found = tokio::task::spawn_blocking(move || {
        rows.par_iter()
            .filter_map(|(id, path, music)| {
                if cancelled() {
                    return None;
                }
                let tags = tags::read(Path::new(path))?;
                let state = if *music && !tags.complete() {
                    "pending"
                } else {
                    "done"
                };
                Some((*id, tags, state))
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| sqlx::Error::Protocol(format!("loudness tag task failed: {e}")))?;

    let mut report = TagReport::default();
    for chunk in found.chunks(5000) {
        let ids: Vec<i32> = chunk.iter().map(|(id, ..)| *id).collect();
        let gains: Vec<Option<f64>> = chunk.iter().map(|(_, t, _)| t.track_gain).collect();
        let albums: Vec<Option<f64>> = chunk.iter().map(|(_, t, _)| t.album_gain).collect();
        let peaks: Vec<Option<f64>> = chunk.iter().map(|(_, t, _)| t.track_peak).collect();
        let states: Vec<&str> = chunk.iter().map(|(.., state)| *state).collect();
        // Still NULL: a scan that ran meanwhile may have seen another version of the file.
        report.read += sqlx::query(
            "UPDATE tracks SET gain_db = v.gain, album_gain_db = v.album, peak = v.peak,
                 loudness_state = v.state
             FROM unnest($1::int[], $2::float8[], $3::float8[], $4::float8[], $5::text[])
                 AS v(id, gain, album, peak, state)
             WHERE tracks.id = v.id AND tracks.loudness_state IS NULL",
        )
        .bind(&ids)
        .bind(&gains)
        .bind(&albums)
        .bind(&peaks)
        .bind(&states)
        .execute(db)
        .await?
        .rows_affected();
        report.pending += states.iter().filter(|s| **s == "pending").count() as u64;
    }
    Ok(report)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AlbumTrack {
    id: i32,
    path: String,
    loudness_state: Option<String>,
}

/// What became of one album in a pass.
#[derive(Debug, PartialEq, Eq)]
enum AlbumOutcome {
    /// Written; with the number of files that could not be decoded.
    Measured {
        failed: u32,
    },
    /// Left `pending` for a later pass: a file is not there right now (a share that is not
    /// mounted must not turn a whole library into `failed`), or its tags are not read yet.
    Postponed,
    Cancelled,
    /// ffmpeg is missing: the whole pass ends.
    NoDecoder(String),
}

struct TrackResult {
    id: i32,
    measured: Option<Measured>,
}

fn round_to(value: f64, digits: i32) -> f64 {
    let factor = 10f64.powi(digits);
    (value * factor).round() / factor
}

fn gain_for(lufs: Option<f64>) -> Option<f64> {
    lufs.map(|lufs| round_to(TARGET_LUFS - lufs, 2))
}

/// Decodes the tracks of one album, one after the other, on the calling (blocking) thread.
fn decode_album(
    tracks: &[AlbumTrack],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<TrackResult>, AlbumOutcome> {
    if tracks.iter().any(|t| !Path::new(&t.path).is_file()) {
        return Err(AlbumOutcome::Postponed);
    }
    let mut results = Vec::with_capacity(tracks.len());
    for track in tracks {
        let measured = match measure_file(Path::new(&track.path), cancelled) {
            Ok(measured) => Some(measured),
            Err(MeasureError::Undecodable(why)) => {
                tracing::warn!("[lautheit] nicht messbar: {} ({why})", track.path);
                None
            }
            Err(MeasureError::Cancelled) => return Err(AlbumOutcome::Cancelled),
            Err(MeasureError::NoDecoder(why)) => return Err(AlbumOutcome::NoDecoder(why)),
        };
        results.push(TrackResult {
            id: track.id,
            measured,
        });
    }
    Ok(results)
}

/// Measures one album and writes it in one transaction.
async fn measure_album(
    db: &PgPool,
    job: &LoudnessJob,
    item_id: i32,
) -> Result<AlbumOutcome, sqlx::Error> {
    let tracks: Vec<AlbumTrack> = sqlx::query_as(
        "SELECT id, path, loudness_state FROM tracks WHERE item_id = $1 ORDER BY position",
    )
    .bind(item_id)
    .fetch_all(db)
    .await?;
    // A track nobody read the tags of yet (a scan is storing this album right now): its tags
    // come first, the next pass measures.
    if tracks.iter().any(|t| t.loudness_state.is_none()) {
        return Ok(AlbumOutcome::Postponed);
    }
    // What failed before is not decoded again; it only takes on the album's value.
    let to_decode: Vec<AlbumTrack> = tracks
        .into_iter()
        .filter(|t| t.loudness_state.as_deref() != Some("failed"))
        .collect();
    let stop = job.clone();
    let decoded = tokio::task::spawn_blocking(move || decode_album(&to_decode, &|| stop.stopped()))
        .await
        .map_err(|e| sqlx::Error::Protocol(format!("loudness task failed: {e}")))?;
    let results = match decoded {
        Ok(results) => results,
        Err(outcome) => return Ok(outcome),
    };

    let measured: Vec<&Measured> = results.iter().filter_map(|r| r.measured.as_ref()).collect();
    let album_gain = gain_for(album_loudness(&measured));
    let failed = results.iter().filter(|r| r.measured.is_none()).count() as u32;

    let ids: Vec<i32> = results.iter().map(|r| r.id).collect();
    let gains: Vec<Option<f64>> = results
        .iter()
        .map(|r| r.measured.as_ref().and_then(|m| gain_for(m.lufs)))
        .collect();
    let peaks: Vec<Option<f64>> = results
        .iter()
        .map(|r| r.measured.as_ref().map(|m| round_to(m.peak, 6)))
        .collect();
    let states: Vec<&str> = results
        .iter()
        .map(|r| {
            if r.measured.is_some() {
                "done"
            } else {
                "failed"
            }
        })
        .collect();

    let mut tx = db.begin().await?;
    // A value from the tags stays (it outranks the measurement); what the tags left open is
    // filled in. A track whose state went back to NULL meanwhile belongs to the next scan.
    sqlx::query(
        "UPDATE tracks SET gain_db = coalesce(tracks.gain_db, v.gain),
             peak = coalesce(tracks.peak, v.peak), loudness_state = v.state
         FROM unnest($1::int[], $2::float8[], $3::float8[], $4::text[]) AS v(id, gain, peak, state)
         WHERE tracks.id = v.id AND tracks.loudness_state IS NOT NULL",
    )
    .bind(&ids)
    .bind(&gains)
    .bind(&peaks)
    .bind(&states)
    .execute(&mut *tx)
    .await?;
    // One value for the whole album, on every track of it — also on those with an album gain
    // in their tags: an album measured in part must not carry two different values.
    sqlx::query(
        "UPDATE tracks SET album_gain_db = $2 WHERE item_id = $1 AND loudness_state IS NOT NULL",
    )
    .bind(item_id)
    .bind(album_gain)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(AlbumOutcome::Measured { failed })
}

/// One pass over everything that is `pending`: returns when it is through, stopped, or
/// without a decoder. Public for the tests; the server goes through [`start`].
pub async fn measure_pending(state: &AppState) -> Result<(), sqlx::Error> {
    let job = &state.loudness;
    let albums: Vec<i32> = sqlx::query_scalar(
        "SELECT DISTINCT t.item_id FROM tracks t JOIN items i ON i.id = t.item_id
         WHERE t.loudness_state = 'pending' AND i.kind = 'album' AND i.missing_since IS NULL
         ORDER BY t.item_id",
    )
    .fetch_all(&state.db)
    .await?;
    job.lock().total = albums.len() as u32;
    if albums.is_empty() {
        return Ok(());
    }
    let threads = state.config.loudness_threads.max(1);
    tracing::info!(
        "[lautheit] {} Alben werden gemessen ({threads} gleichzeitig)",
        albums.len()
    );
    let db = &state.db;
    // ffmpeg is missing: this pass ends, a later one (after the next scan or restart) tries again.
    let no_decoder = AtomicBool::new(false);
    let no_decoder = &no_decoder;
    let mut outcomes = stream::iter(albums)
        .map(|item_id| async move {
            if job.stopped() || no_decoder.load(Ordering::Relaxed) {
                return Ok(AlbumOutcome::Cancelled);
            }
            measure_album(db, job, item_id).await
        })
        .buffer_unordered(threads);
    let mut postponed = 0u32;
    while let Some(outcome) = outcomes.next().await {
        match outcome {
            Ok(AlbumOutcome::Measured { failed }) => {
                let mut progress = job.lock();
                progress.processed += 1;
                progress.failed += failed;
                if progress.processed.is_multiple_of(100) {
                    tracing::info!(
                        "[lautheit] {} von {} Alben gemessen",
                        progress.processed,
                        progress.total
                    );
                }
            }
            Ok(AlbumOutcome::Postponed) => postponed += 1,
            Ok(AlbumOutcome::Cancelled) => {}
            Ok(AlbumOutcome::NoDecoder(why)) => {
                // Nothing is marked: once ffmpeg is there, the next pass measures everything.
                if !no_decoder.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        "[lautheit] ffmpeg lässt sich nicht starten ({why}); die Lautstärke \
                         wird nicht gemessen"
                    );
                }
            }
            // One album's database trouble is not the end of the pass.
            Err(e) => tracing::warn!("[lautheit] Album übersprungen: {e}"),
        }
    }
    let progress = job.snapshot();
    tracing::info!(
        "[lautheit] {} von {} Alben gemessen{}{}",
        progress.processed,
        progress.total,
        match progress.failed {
            0 => String::new(),
            n => format!(", {n} Dateien nicht messbar"),
        },
        match postponed {
            0 => String::new(),
            n => format!(", {n} Alben zurückgestellt (Dateien nicht erreichbar)"),
        }
    );
    Ok(())
}

/// Tags first, then the measuring (unless `AUTOREVERSE_LOUDNESS=0`), until no scan asked for
/// another round. One at a time per process; a second call only books another round.
pub async fn run(state: &AppState) {
    if state.loudness.stopped() || !state.loudness.try_start() {
        return;
    }
    run_claimed(state).await;
}

async fn run_claimed(state: &AppState) {
    let job = &state.loudness;
    loop {
        job.0.again.store(false, Ordering::Relaxed);
        let stop = job.clone();
        let result = async {
            // Again here, although the scan does it too: at a server start nobody has scanned
            // yet, and an upgraded library should not wait for the next scan.
            let read = read_pending_tags(&state.db, move || stop.stopped()).await?;
            if read.read > 0 {
                tracing::info!(
                    "[lautheit] Tags von {} Dateien gelesen, {} Titel sind zu messen",
                    read.read,
                    read.pending
                );
            }
            if state.config.loudness {
                measure_pending(state).await?;
            }
            Ok::<_, sqlx::Error>(())
        }
        .await;
        if let Err(e) = result {
            tracing::warn!("[lautheit] Durchgang abgebrochen: {e}");
        }
        let mut progress = job.lock();
        // Checked under the lock `try_start` takes: a request cannot fall between the two.
        if job.stopped() || !job.0.again.load(Ordering::Relaxed) {
            progress.running = false;
            return;
        }
        *progress = LoudnessProgress {
            running: true,
            ..Default::default()
        };
    }
}

/// Starts the pass in the background; returns at once. The pass is claimed before this
/// returns, so whoever sees a scan finished also sees the pass that follows it as running.
pub fn start(state: &AppState) {
    if state.loudness.stopped() || !state.loudness.try_start() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move { run_claimed(&state).await });
}
