//! Writing scanned folders into the database (legacy `scanner/store.ts`): one transaction per
//! item, so a folder that fails is skipped and every other one still lands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures_util::{StreamExt, stream};
use sqlx::{PgPool, Postgres, Transaction};

use super::books::{KnownFiles, ScanFailure, ScannedItem};
use super::covers::{extract_embedded, find_cover_file};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreReport {
    pub new: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub skipped: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    New,
    Updated,
    Unchanged,
}

/// Files with a recorded mtime and size, for the scanner's unchanged check.
pub async fn known_files(db: &PgPool) -> Result<KnownFiles, sqlx::Error> {
    let rows: Vec<(String, f64, i64)> = sqlx::query_as(
        "SELECT path, mtime, size FROM tracks WHERE mtime IS NOT NULL AND size IS NOT NULL",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(path, mtime, size)| (path, (mtime, size)))
        .collect())
}

/// The cover to store: the current one while its file exists, else a cover file in the folder
/// or in the first track's folder, else a picture embedded in one of the first three tracks.
async fn resolve_cover(
    item_id: i32,
    dir: &str,
    track_paths: &[String],
    covers_dir: &Path,
    current: Option<String>,
) -> Option<String> {
    let dir = PathBuf::from(dir);
    let tracks: Vec<PathBuf> = track_paths.iter().take(3).map(PathBuf::from).collect();
    let covers_dir = covers_dir.to_path_buf();
    let found = tokio::task::spawn_blocking(move || {
        if let Some(current) = &current
            && Path::new(current).exists()
        {
            return Some(current.clone());
        }
        // An album of discs often keeps its cover in the first disc's folder.
        let first_folder = tracks
            .first()
            .and_then(|t| t.parent())
            .map(Path::to_path_buf);
        find_cover_file(&dir)
            .or_else(|| first_folder.as_deref().and_then(find_cover_file))
            .or_else(|| {
                tracks
                    .iter()
                    .find_map(|track| extract_embedded(track, &covers_dir, item_id))
            })
            .map(|p| p.to_string_lossy().into_owned())
            .or(current)
    })
    .await;
    found.unwrap_or(None)
}

#[derive(sqlx::FromRow)]
struct Existing {
    id: i32,
    cover_path: Option<String>,
    series: Option<String>,
    series_index: Option<f64>,
    locked_fields: Vec<String>,
}

/// `value` unless a person locked `field` by hand — a correction has to outlive the next scan.
/// A locked title keeps its sort key too, which is derived from it.
fn unless_locked(locked: &[String], field: &str) -> bool {
    let locked = |f: &str| locked.iter().any(|l| l == f);
    !(locked(field) || (field == "sortTitle" && locked("title")))
}

async fn store_one(
    tx: &mut Transaction<'_, Postgres>,
    entry: &ScannedItem,
    covers_dir: &Path,
) -> Result<Outcome, sqlx::Error> {
    let existing: Option<Existing> = sqlx::query_as(
        "SELECT id, cover_path, series, series_index, locked_fields
         FROM items WHERE source_path = $1",
    )
    .bind(&entry.source_path)
    .fetch_optional(&mut **tx)
    .await?;

    if entry.unchanged {
        if let Some(existing) = existing {
            let cover = resolve_cover(
                existing.id,
                &entry.source_path,
                &[],
                covers_dir,
                existing.cover_path.clone(),
            )
            .await;
            // Backfill only. A folder whose files never change is never read again, so a library
            // scanned before series detection existed would keep its empty columns forever — but
            // a series that came from the file's tags must not be overwritten by a guess from the
            // path.
            let locked = &existing.locked_fields;
            let series = (existing.series.is_none()
                && entry.series.is_some()
                && unless_locked(locked, "series"))
            .then(|| entry.series.clone())
            .flatten();
            let series_index = (existing.series_index.is_none()
                && entry.series_index.is_some()
                && unless_locked(locked, "seriesIndex")
                && (existing.series.is_some() || entry.series.is_some()))
            .then_some(entry.series_index)
            .flatten();
            sqlx::query(
                "UPDATE items SET missing_since = NULL, cover_path = $2,
                     series = coalesce($3, series), series_index = coalesce($4, series_index)
                 WHERE id = $1",
            )
            .bind(existing.id)
            .bind(cover)
            .bind(series)
            .bind(series_index)
            .execute(&mut **tx)
            .await?;
        }
        return Ok(Outcome::Unchanged);
    }

    let sort_title = entry.title.to_lowercase();
    let (item_id, outcome, current_cover) = match existing {
        Some(existing) => {
            // Every scanned field, each kept as stored when locked.
            let locked = &existing.locked_fields;
            sqlx::query(
                "UPDATE items SET missing_since = NULL,
                     title = CASE WHEN $2 THEN $3 ELSE title END,
                     sort_title = CASE WHEN $4 THEN $5 ELSE sort_title END,
                     author = CASE WHEN $6 THEN $7 ELSE author END,
                     artist = CASE WHEN $8 THEN $9 ELSE artist END,
                     album_artist = CASE WHEN $10 THEN $11 ELSE album_artist END,
                     description = CASE WHEN $12 THEN $13 ELSE description END,
                     series = CASE WHEN $14 THEN $15 ELSE series END,
                     series_index = CASE WHEN $16 THEN $17 ELSE series_index END,
                     year = CASE WHEN $18 THEN $19 ELSE year END
                 WHERE id = $1",
            )
            .bind(existing.id)
            .bind(unless_locked(locked, "title"))
            .bind(&entry.title)
            .bind(unless_locked(locked, "sortTitle"))
            .bind(&sort_title)
            .bind(unless_locked(locked, "author"))
            .bind(&entry.author)
            .bind(unless_locked(locked, "artist"))
            .bind(&entry.artist)
            .bind(unless_locked(locked, "albumArtist"))
            .bind(&entry.album_artist)
            .bind(unless_locked(locked, "description"))
            .bind(&entry.description)
            .bind(unless_locked(locked, "series"))
            .bind(&entry.series)
            .bind(unless_locked(locked, "seriesIndex"))
            .bind(entry.series_index)
            .bind(unless_locked(locked, "year"))
            .bind(entry.year)
            .execute(&mut **tx)
            .await?;
            (existing.id, Outcome::Updated, existing.cover_path)
        }
        None => {
            let id: i32 = sqlx::query_scalar(
                "INSERT INTO items (kind, source_path, title, sort_title, author, artist,
                     album_artist, description, series, series_index, year)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 RETURNING id",
            )
            .bind(entry.kind.as_str())
            .bind(&entry.source_path)
            .bind(&entry.title)
            .bind(&sort_title)
            .bind(&entry.author)
            .bind(&entry.artist)
            .bind(&entry.album_artist)
            .bind(&entry.description)
            .bind(&entry.series)
            .bind(entry.series_index)
            .bind(entry.year)
            .fetch_one(&mut **tx)
            .await?;
            (id, Outcome::New, None)
        }
    };

    let current_paths: Vec<String> = entry.tracks.iter().map(|t| t.path.clone()).collect();
    // A track about to go may be the twin of one that stays (a duplicate file the scanner now
    // leaves out): its favorites and playlist entries move to the twin with the same title.
    for statement in [
        "UPDATE favorites SET track_id = twin.id
         FROM tracks AS gone, tracks AS twin
         WHERE favorites.track_id = gone.id
           AND gone.item_id = $1 AND NOT (gone.path = ANY($2))
           AND twin.item_id = $1 AND twin.path = ANY($2)
           AND lower(btrim(twin.title)) = lower(btrim(gone.title))
           AND NOT EXISTS (SELECT 1 FROM favorites AS taken
                           WHERE taken.user_id = favorites.user_id
                             AND taken.track_id = twin.id)",
        "UPDATE playlist_entries SET track_id = twin.id
         FROM tracks AS gone, tracks AS twin
         WHERE playlist_entries.track_id = gone.id
           AND gone.item_id = $1 AND NOT (gone.path = ANY($2))
           AND twin.item_id = $1 AND twin.path = ANY($2)
           AND lower(btrim(twin.title)) = lower(btrim(gone.title))",
    ] {
        sqlx::query(statement)
            .bind(item_id)
            .bind(&current_paths)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query("DELETE FROM tracks WHERE item_id = $1 AND NOT (path = ANY($2))")
        .bind(item_id)
        .bind(&current_paths)
        .execute(&mut **tx)
        .await?;
    // Out of the way first: two surviving tracks that swap positions would otherwise collide on
    // UNIQUE(item_id, position) halfway through the upserts.
    sqlx::query("UPDATE tracks SET position = -id WHERE item_id = $1")
        .bind(item_id)
        .execute(&mut **tx)
        .await?;
    // A track title or disc edited by hand stays, like an item's locked fields (legacy locked
    // them in PATCH /tracks but rewrote them here anyway).
    for t in &entry.tracks {
        sqlx::query(
            "INSERT INTO tracks (item_id, position, path, duration, title, disc, mtime, size)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (path) DO UPDATE SET item_id = excluded.item_id,
                 position = excluded.position, duration = excluded.duration,
                 title = CASE WHEN 'title' = ANY(tracks.locked_fields)
                     THEN tracks.title ELSE excluded.title END,
                 disc = CASE WHEN 'disc' = ANY(tracks.locked_fields)
                     THEN tracks.disc ELSE excluded.disc END,
                 mtime = excluded.mtime, size = excluded.size",
        )
        .bind(item_id)
        .bind(t.position)
        .bind(&t.path)
        .bind(t.duration)
        .bind(&t.title)
        .bind(t.disc)
        .bind(t.mtime)
        .bind(t.size)
        .execute(&mut **tx)
        .await?;
    }

    absorb_emptied_subfolders(tx, item_id, entry).await?;

    sqlx::query("DELETE FROM chapters WHERE item_id = $1")
        .bind(item_id)
        .execute(&mut **tx)
        .await?;
    for (i, c) in entry.chapters.iter().enumerate() {
        sqlx::query(
            "INSERT INTO chapters (item_id, position, title, start, \"end\")
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(item_id)
        .bind(i as i32 + 1)
        .bind(&c.title)
        .bind(c.start)
        .bind(c.end)
        .execute(&mut **tx)
        .await?;
    }

    let cover = resolve_cover(
        item_id,
        &entry.source_path,
        &current_paths,
        covers_dir,
        current_cover,
    )
    .await;
    sqlx::query("UPDATE items SET cover_path = $2 WHERE id = $1")
        .bind(item_id)
        .bind(cover)
        .execute(&mut **tx)
        .await?;
    Ok(outcome)
}

/// Items of folders below `item_id`'s whose tracks all just moved to it: the per-disc albums of
/// an album now read as one. What users kept on them moves along — progress and bookmarks
/// shifted by the length of the discs before, favorites, playlist entries, listening history,
/// an artist's chosen cover — and then they go. Track favorites need nothing: the tracks moved
/// with their ids.
async fn absorb_emptied_subfolders(
    tx: &mut Transaction<'_, Postgres>,
    item_id: i32,
    entry: &ScannedItem,
) -> Result<(), sqlx::Error> {
    let prefix = format!("{}/", entry.source_path);
    let emptied: Vec<(i32, String)> = sqlx::query_as(
        "SELECT id, source_path FROM items AS old
         WHERE old.kind = $1 AND old.id <> $2
           AND substr(old.source_path, 1, length($3)) = $3
           AND NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.item_id = old.id)",
    )
    .bind(entry.kind.as_str())
    .bind(item_id)
    .bind(&prefix)
    .fetch_all(&mut **tx)
    .await?;
    for (old_id, old_path) in emptied {
        // Where the old album starts inside the new one: everything before its first track.
        let offset: f64 = sqlx::query_scalar(
            "SELECT coalesce(sum(duration), 0) FROM tracks
             WHERE item_id = $1 AND position < (
                 SELECT coalesce(min(position), 0) FROM tracks
                 WHERE item_id = $1 AND substr(path, 1, length($2)) = $2)",
        )
        .bind(item_id)
        .bind(format!("{old_path}/"))
        .fetch_one(&mut **tx)
        .await?;
        // Shifted by the discs before it.
        for statement in [
            "INSERT INTO progress (user_id, item_id, position, finished, updated_at)
             SELECT user_id, $2, position + $3, false, updated_at FROM progress
             WHERE item_id = $1
             ON CONFLICT (user_id, item_id) DO UPDATE
                 SET position = excluded.position, finished = false,
                     updated_at = excluded.updated_at
                 WHERE excluded.updated_at > progress.updated_at",
            "INSERT INTO bookmarks (user_id, item_id, position, title)
             SELECT user_id, $2, position + $3, title FROM bookmarks WHERE item_id = $1
             ON CONFLICT DO NOTHING",
        ] {
            sqlx::query(statement)
                .bind(old_id)
                .bind(item_id)
                .bind(offset)
                .execute(&mut **tx)
                .await?;
        }
        for statement in [
            "INSERT INTO favorites (user_id, item_id)
             SELECT user_id, $2 FROM favorites WHERE item_id = $1
             ON CONFLICT (user_id, item_id) WHERE item_id IS NOT NULL DO NOTHING",
            "UPDATE playlist_entries SET item_id = $2 WHERE item_id = $1",
            "UPDATE listen_events SET item_id = $2 WHERE item_id = $1",
            "UPDATE artist_covers SET item_id = $2 WHERE item_id = $1",
        ] {
            sqlx::query(statement)
                .bind(old_id)
                .bind(item_id)
                .execute(&mut **tx)
                .await?;
        }
        sqlx::query("DELETE FROM items WHERE id = $1")
            .bind(old_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// How many items are stored at once. Each holds a pool connection for its transaction; the
/// pool has 10, and the web UI needs some while a scan runs.
const STORE_CONCURRENCY: usize = 6;

/// Stores every scanned item, [`STORE_CONCURRENCY`] transactions at a time. Items never share
/// rows — one per `source_path`, tracks by their own path — so they cannot collide. A failing
/// one is rolled back, recorded in `failures` and counted as skipped; the rest carry on.
/// Results are taken in input order. `on_progress(processed, total, report)` after each item;
/// `cancelled()` is asked before each starts and stops the loop.
pub async fn store_items(
    db: &PgPool,
    scanned: &[ScannedItem],
    covers_dir: &Path,
    failures: &mut Vec<ScanFailure>,
    mut on_progress: impl FnMut(usize, usize, StoreReport),
    cancelled: impl Fn() -> bool,
) -> StoreReport {
    let cancelled = &cancelled;
    // Over indices, not `&ScannedItem`: a closure taking a reference makes the future too
    // generic for `tokio::spawn(run_scan(…))` to prove it `Send`.
    let mut results = stream::iter(0..scanned.len())
        .map(|i| async move {
            let entry = &scanned[i];
            if cancelled() {
                return None;
            }
            let result = async {
                let mut tx = db.begin().await?;
                let outcome = store_one(&mut tx, entry, covers_dir).await?;
                tx.commit().await?;
                Ok::<_, sqlx::Error>(outcome)
            }
            .await;
            Some((entry, result))
        })
        .buffered(STORE_CONCURRENCY);
    let mut report = StoreReport::default();
    let mut processed = 0;
    while let Some(Some((entry, result))) = results.next().await {
        match result {
            Ok(Outcome::New) => report.new += 1,
            Ok(Outcome::Updated) => report.updated += 1,
            Ok(Outcome::Unchanged) => report.unchanged += 1,
            Err(e) => {
                failures.push(ScanFailure {
                    path: entry.source_path.clone(),
                    message: e.to_string(),
                });
                report.skipped += 1;
            }
        }
        processed += 1;
        on_progress(processed, scanned.len(), report);
    }
    report
}

/// Deletes the items under `root` this run did not find — the root's own item included —
/// together with their history (cascade). `skipped` folders failed this run and were never seen
/// absent, so they and everything below them stay. Items outside `root` are not touched: the
/// books pass never removes music and vice versa.
pub async fn remove_vanished(
    db: &PgPool,
    root: &str,
    found: &HashSet<String>,
    skipped: &[String],
) -> Result<u64, sqlx::Error> {
    let prefix = format!("{root}/");
    let is_skipped = |path: &str| {
        skipped
            .iter()
            .any(|entry| path == entry || path.starts_with(&format!("{entry}/")))
    };
    let candidates: Vec<(i32, Option<String>)> = sqlx::query_as(
        "SELECT id, source_path FROM items
         WHERE source_path = $1 OR substr(source_path, 1, length($2)) = $2",
    )
    .bind(root)
    .bind(&prefix)
    .fetch_all(db)
    .await?;
    let doomed: Vec<i32> = candidates
        .into_iter()
        .filter(|(_, path)| match path {
            None => true,
            Some(path) => !found.contains(path) && !is_skipped(path),
        })
        .map(|(id, _)| id)
        .collect();
    if doomed.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query("DELETE FROM items WHERE id = ANY($1)")
        .bind(&doomed)
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}
