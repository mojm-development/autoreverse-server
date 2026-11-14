//! Keeps `tracks.has_lyrics` true to the files, at the end of every scan.
//!
//! Its own pass over the tracks rather than part of reading a folder, for two reasons. A folder
//! whose audio files did not change is never read again — but somebody may have put a `.lrc`
//! beside one of them since, which changes no audio file. And the tracks that were in the library
//! before the column existed need the flag without anyone rescanning anything by hand.
//!
//! So on every scan: the files beside each track are looked for (a directory listing per folder,
//! a handful of small files read), and the tags of an audio file are read once per version of
//! the file — `lyrics_embedded` remembers the answer until the file's size or mtime change
//! (`scanner::store` forgets it then). The first scan after the upgrade therefore reads the tags
//! of the whole library once; every later one only those of new and changed files.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use sqlx::PgPool;

use crate::library::lyrics::{has_embedded, has_sidecar};

#[derive(Debug, Clone, sqlx::FromRow)]
struct Row {
    id: i32,
    path: String,
    has_lyrics: bool,
    lyrics_embedded: Option<bool>,
}

/// What a track's flags should become; only tracks where something differs are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Verdict {
    id: i32,
    has_lyrics: bool,
    embedded: bool,
}

/// The file names in a folder, or `None` if it cannot be listed — a share that is not mounted,
/// a folder that is gone. Its tracks then keep their flags: not seeing is not knowing.
fn listing(dir: &Path) -> Option<HashSet<OsString>> {
    Some(
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| Some(entry.ok()?.file_name()))
            .collect(),
    )
}

fn judge(row: &Row, names: &HashSet<OsString>) -> Option<Verdict> {
    let path = Path::new(&row.path);
    // The audio file itself is gone: the item is about to be marked missing or removed, and a
    // flag guessed from an absent file would be wrong either way.
    if !names.contains(path.file_name()?) {
        return None;
    }
    let stem = path.file_stem()?;
    // Only where the listing shows a candidate is a file opened at all.
    let beside = names.iter().any(|name| {
        let name = Path::new(name);
        name.file_stem() == Some(stem)
            && name
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("lrc") || e.eq_ignore_ascii_case("txt"))
    }) && has_sidecar(path);
    let embedded = row.lyrics_embedded.unwrap_or_else(|| has_embedded(path));
    let verdict = Verdict {
        id: row.id,
        has_lyrics: beside || embedded,
        embedded,
    };
    (verdict.has_lyrics != row.has_lyrics || row.lyrics_embedded != Some(embedded))
        .then_some(verdict)
}

fn judge_all(rows: &[Row], cancelled: &(impl Fn() -> bool + Sync)) -> Vec<Verdict> {
    let mut by_folder: HashMap<PathBuf, Vec<&Row>> = HashMap::new();
    for row in rows {
        if let Some(dir) = Path::new(&row.path).parent() {
            by_folder.entry(dir.to_path_buf()).or_default().push(row);
        }
    }
    by_folder
        .par_iter()
        .flat_map_iter(|(dir, rows)| {
            let names = (!cancelled()).then(|| listing(dir)).flatten();
            rows.iter()
                .filter_map(move |row| judge(row, names.as_ref()?))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Brings `has_lyrics` of every track in line with its files; returns how many rows changed.
/// Tracks whose folder or file cannot be seen are left as they are.
pub async fn refresh_flags(
    db: &PgPool,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<u64, sqlx::Error> {
    let rows: Vec<Row> =
        sqlx::query_as("SELECT id, path, has_lyrics, lyrics_embedded FROM tracks ORDER BY id")
            .fetch_all(db)
            .await?;
    let verdicts = tokio::task::spawn_blocking(move || judge_all(&rows, &cancelled))
        .await
        .map_err(|e| sqlx::Error::Protocol(format!("lyrics task failed: {e}")))?;
    let mut changed = 0;
    // In slices, so that a first run over a large library is not one statement with a hundred
    // thousand parameters' worth of arrays held in a single transaction.
    for chunk in verdicts.chunks(5000) {
        let ids: Vec<i32> = chunk.iter().map(|v| v.id).collect();
        let has: Vec<bool> = chunk.iter().map(|v| v.has_lyrics).collect();
        let embedded: Vec<bool> = chunk.iter().map(|v| v.embedded).collect();
        changed += sqlx::query(
            "UPDATE tracks SET has_lyrics = v.has_lyrics, lyrics_embedded = v.embedded
             FROM unnest($1::int[], $2::bool[], $3::bool[]) AS v(id, has_lyrics, embedded)
             WHERE tracks.id = v.id",
        )
        .bind(&ids)
        .bind(&has)
        .bind(&embedded)
        .execute(db)
        .await?
        .rows_affected();
    }
    Ok(changed)
}
