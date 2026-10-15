//! Walking a library root and turning every folder with audio files into one item (legacy
//! `scanner/books.ts`, which serves the music pass as well — only `kind` differs).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::chapters::{ScannedChapter, read_chapters};
use super::series::{SeriesSource, resolve_series};
use super::tags::{TrackTags, read_tags};

pub const AUDIO: &[&str] = &["mp3", "m4a", "m4b", "flac", "ogg", "opus", "wav", "aac"];

/// What the database already knows about a file: `path → (mtime, size)`.
pub type KnownFiles = HashMap<String, (f64, i64)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Book,
    Album,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Book => "book",
            ItemKind::Album => "album",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScannedTrack {
    pub path: String,
    pub position: i32,
    pub title: Option<String>,
    pub disc: Option<i32>,
    pub duration: f64,
    pub mtime: f64,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScannedItem {
    pub source_path: String,
    pub kind: ItemKind,
    pub title: String,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub description: Option<String>,
    pub series: Option<String>,
    pub year: Option<i32>,
    pub series_index: Option<f64>,
    pub tracks: Vec<ScannedTrack>,
    pub chapters: Vec<ScannedChapter>,
    /// Every file is known with the same mtime and size: no tags were read, `tracks` and
    /// `chapters` are empty and only the tree-derived fields are filled.
    pub unchanged: bool,
}

/// A folder (or a stored item) the scan could not handle; it is left untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFailure {
    pub path: String,
    pub message: String,
}

fn reason(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "Verzeichnis existiert nicht".into(),
        io::ErrorKind::PermissionDenied => "Verzeichnis ist nicht lesbar".into(),
        _ => format!("Verzeichnis nicht zugänglich ({e})"),
    }
}

/// Why `root` cannot be scanned, or `None`. The walk below swallows `read_dir` failures per
/// folder; on the root that silence would look like an empty library and every item under it
/// would be removed — so the scan asks first and skips an unusable root entirely.
pub fn library_root_problem(root: &Path) -> Option<String> {
    let metadata = match std::fs::metadata(root) {
        Ok(m) => m,
        Err(e) => return Some(reason(&e)),
    };
    if !metadata.is_dir() {
        return Some("Pfad ist kein Verzeichnis".into());
    }
    std::fs::read_dir(root).err().map(|e| reason(&e))
}

/// The root and every directory below it, depth first in `read_dir` order. Symlinked
/// directories are not followed (no cycles), unreadable ones are skipped.
pub fn all_directories(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for sub in dirs {
            found.push(sub.clone());
            walk(&sub, found);
        }
    }
    let mut found = vec![root.to_path_buf()];
    walk(root, &mut found);
    found
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
}

/// Audio files directly in `dir`, sorted by path. Names that are not UTF-8 cannot be stored and
/// are left out.
fn audio_files_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| is_audio(p))
        .filter_map(|p| p.into_os_string().into_string().ok())
        .collect();
    files.sort();
    files
}

#[derive(Debug, Clone)]
pub struct RawTrack {
    pub path: String,
    pub tags: TrackTags,
    pub mtime: f64,
    pub size: i64,
}

fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Disc, then track number (file order for untagged files), then file order — numbered 1..n
/// without gaps. Positions land in UNIQUE(item_id, position), so a repeated number is not
/// cosmetic: the insert would fail.
pub fn order_tracks(raw: &[RawTrack], file_order: &[String]) -> Vec<ScannedTrack> {
    let fallback = |path: &str| {
        file_order
            .iter()
            .position(|f| f == path)
            .map_or(0, |i| i + 1) as i64
    };
    let mut sorted: Vec<&RawTrack> = raw.iter().collect();
    sorted.sort_by_key(|t| {
        (
            t.tags.disc.unwrap_or(0),
            t.tags.track.map_or_else(|| fallback(&t.path), i64::from),
            fallback(&t.path),
        )
    });
    sorted
        .into_iter()
        .enumerate()
        .map(|(i, t)| ScannedTrack {
            path: t.path.clone(),
            position: i as i32 + 1,
            title: Some(t.tags.title.clone().unwrap_or_else(|| stem(&t.path))),
            disc: t.tags.disc.map(|d| d as i32),
            duration: t.tags.duration,
            mtime: t.mtime,
            size: t.size,
        })
        .collect()
}

fn file_stats(path: &str) -> io::Result<(f64, i64)> {
    let metadata = std::fs::metadata(path)?;
    let mtime = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Ok((mtime, metadata.len() as i64))
}

/// One folder as an item, or `None` if it holds no audio files.
pub fn scan_folder(
    root: &Path,
    dir: &Path,
    kind: ItemKind,
    known: &KnownFiles,
) -> io::Result<Option<ScannedItem>> {
    let files = audio_files_in(dir);
    if files.is_empty() {
        return Ok(None);
    }
    let mut stats = HashMap::new();
    for file in &files {
        stats.insert(file.clone(), file_stats(file)?);
    }
    let dir_str = dir.to_string_lossy().into_owned();
    let dir_prefix = format!("{dir_str}/");
    let known_in_folder = known
        .keys()
        .filter(|k| {
            k.strip_prefix(&dir_prefix)
                .is_some_and(|rest| !rest.contains('/'))
        })
        .count();
    let unchanged = known_in_folder == files.len()
        && files
            .iter()
            .all(|f| known.get(f).is_some_and(|k| Some(k) == stats.get(f)));

    let parts: Vec<String> = dir
        .strip_prefix(root)
        .map(|rel| {
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let folder_author_or_artist = (parts.len() >= 2).then(|| parts[0].clone());
    let folder_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir_str.clone());
    let parent_is_series = kind == ItemKind::Book && parts.len() >= 3;
    let parent_name = parent_is_series.then(|| parts[1].as_str());

    if unchanged {
        // No tags are read for an untouched folder, so this pass sees only the tree.
        let (series, series_index, title) = match kind {
            ItemKind::Book => {
                let guess = resolve_series(&SeriesSource {
                    folder_name: &folder_name,
                    parent_name,
                    parent_is_series,
                    title: &folder_name,
                    ..Default::default()
                });
                (guess.series, guess.series_index, guess.title)
            }
            ItemKind::Album => (None, None, folder_name.clone()),
        };
        let book = kind == ItemKind::Book;
        return Ok(Some(ScannedItem {
            source_path: dir_str,
            kind,
            title,
            author: folder_author_or_artist.clone().filter(|_| book),
            artist: folder_author_or_artist.filter(|_| !book),
            album_artist: None,
            description: None,
            series,
            year: None,
            series_index,
            tracks: Vec::new(),
            chapters: Vec::new(),
            unchanged: true,
        }));
    }

    let raw: Vec<RawTrack> = files
        .iter()
        .map(|path| {
            let (mtime, size) = stats[path];
            RawTrack {
                path: path.clone(),
                tags: read_tags(Path::new(path)),
                mtime,
                size,
            }
        })
        .collect();
    let tracks = order_tracks(&raw, &files);
    let first = &raw[0].tags;
    let title_from_tag = first.album.is_some();
    let base_title = first.album.clone().unwrap_or_else(|| folder_name.clone());

    let (series, series_index, title) = match kind {
        ItemKind::Book => {
            let guess = resolve_series(&SeriesSource {
                tag_series: first.series.as_deref(),
                tag_series_index: first.series_index,
                folder_name: &folder_name,
                parent_name,
                parent_is_series,
                title: &base_title,
                title_from_tag,
            });
            (guess.series, guess.series_index, guess.title)
        }
        ItemKind::Album => (None, None, base_title),
    };

    let (author, artist, album_artist, year) = match kind {
        ItemKind::Book => (
            first.artist.clone().or(folder_author_or_artist),
            None,
            None,
            None,
        ),
        ItemKind::Album => (
            None,
            first
                .album_artist
                .clone()
                .or_else(|| first.artist.clone())
                .or(folder_author_or_artist),
            first.album_artist.clone(),
            first.year,
        ),
    };

    let mut chapters = Vec::new();
    if kind == ItemKind::Book {
        if tracks.len() == 1 {
            chapters = read_chapters(Path::new(&tracks[0].path));
        }
        if chapters.is_empty() {
            // One chapter per file.
            let mut elapsed = 0.0;
            chapters = tracks
                .iter()
                .map(|t| {
                    let chapter = ScannedChapter {
                        title: t
                            .title
                            .clone()
                            .filter(|t| !t.is_empty())
                            .unwrap_or_else(|| format!("Teil {}", t.position)),
                        start: elapsed,
                        end: elapsed + t.duration,
                    };
                    elapsed += t.duration;
                    chapter
                })
                .collect();
        }
    }

    Ok(Some(ScannedItem {
        source_path: dir_str,
        kind,
        title,
        author,
        artist,
        album_artist,
        description: first.description.clone(),
        series,
        year,
        series_index,
        tracks,
        chapters,
        unchanged: false,
    }))
}

/// Scans every folder under `root`. A folder that fails is recorded in `failures` and left out.
/// `on_progress(processed, total)` after each folder; `cancelled()` is asked before each and
/// stops the walk — the caller must then not treat the result as complete.
pub fn scan_tree(
    root: &Path,
    kind: ItemKind,
    known: &KnownFiles,
    failures: &mut Vec<ScanFailure>,
    mut on_progress: impl FnMut(usize, usize),
    cancelled: impl Fn() -> bool,
) -> Vec<ScannedItem> {
    let dirs = all_directories(root);
    let mut results = Vec::new();
    for (i, dir) in dirs.iter().enumerate() {
        if cancelled() {
            break;
        }
        match scan_folder(root, dir, kind, known) {
            Ok(Some(item)) => results.push(item),
            Ok(None) => {}
            Err(e) => failures.push(ScanFailure {
                path: dir.to_string_lossy().into_owned(),
                message: reason(&e),
            }),
        }
        on_progress(i + 1, dirs.len());
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(track: Option<u32>, disc: Option<u32>) -> TrackTags {
        TrackTags {
            track,
            disc,
            duration: 10.0,
            readable: true,
            ..TrackTags::UNREADABLE
        }
    }

    fn raw(path: &str, track: Option<u32>, disc: Option<u32>) -> RawTrack {
        RawTrack {
            path: path.into(),
            tags: tags(track, disc),
            mtime: 1.0,
            size: 2,
        }
    }

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn positions(tracks: &[ScannedTrack]) -> Vec<i32> {
        tracks.iter().map(|t| t.position).collect()
    }

    fn paths(tracks: &[ScannedTrack]) -> Vec<&str> {
        tracks.iter().map(|t| t.path.as_str()).collect()
    }

    #[test]
    fn numbers_loose_singles_that_each_claim_track_1_without_colliding() {
        let order = files(&["/m/a.flac", "/m/b.flac", "/m/c.flac"]);
        let raw: Vec<_> = order.iter().map(|f| raw(f, Some(1), None)).collect();
        assert_eq!(positions(&order_tracks(&raw, &order)), [1, 2, 3]);
    }

    #[test]
    fn numbers_a_two_disc_album_in_one_folder_across_both_discs() {
        let order = files(&["/m/d1t1.mp3", "/m/d1t2.mp3", "/m/d2t1.mp3", "/m/d2t2.mp3"]);
        let tracks = order_tracks(
            &[
                raw(&order[2], Some(1), Some(2)),
                raw(&order[0], Some(1), Some(1)),
                raw(&order[3], Some(2), Some(2)),
                raw(&order[1], Some(2), Some(1)),
            ],
            &order,
        );
        assert_eq!(
            paths(&tracks),
            order.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(positions(&tracks), [1, 2, 3, 4]);
    }

    #[test]
    fn numbers_a_folder_mixing_tagged_and_untagged_files_without_colliding() {
        let order = files(&["/m/untagged.mp3", "/m/tagged.mp3"]);
        let tracks = order_tracks(
            &[raw(&order[0], None, None), raw(&order[1], Some(1), None)],
            &order,
        );
        assert_eq!(positions(&tracks), [1, 2]);
    }

    #[test]
    fn keeps_a_normally_tagged_album_in_track_order() {
        let order = files(&["/m/03.mp3", "/m/01.mp3", "/m/02.mp3"]);
        let tracks = order_tracks(
            &[
                raw(&order[0], Some(3), None),
                raw(&order[1], Some(1), None),
                raw(&order[2], Some(2), None),
            ],
            &order,
        );
        assert_eq!(paths(&tracks), ["/m/01.mp3", "/m/02.mp3", "/m/03.mp3"]);
        assert_eq!(positions(&tracks), [1, 2, 3]);
    }

    #[test]
    fn closes_gaps_rather_than_preserving_gappy_track_numbers() {
        let order = files(&["/m/a.mp3", "/m/b.mp3"]);
        let tracks = order_tracks(
            &[raw(&order[0], Some(1), None), raw(&order[1], Some(7), None)],
            &order,
        );
        assert_eq!(positions(&tracks), [1, 2]);
    }

    #[test]
    fn falls_back_to_file_order_for_ties_deterministically() {
        let order = files(&["/m/b.mp3", "/m/a.mp3"]);
        let tracks = order_tracks(
            &[raw(&order[1], Some(5), None), raw(&order[0], Some(5), None)],
            &order,
        );
        assert_eq!(paths(&tracks), ["/m/b.mp3", "/m/a.mp3"]);
    }

    #[test]
    fn titles_untagged_files_from_their_filename() {
        let order = files(&["/m/Gentleman - Hold a Medi Megamix.flac"]);
        let tracks = order_tracks(&[raw(&order[0], None, None)], &order);
        assert_eq!(
            tracks[0].title.as_deref(),
            Some("Gentleman - Hold a Medi Megamix")
        );
    }

    #[test]
    fn root_problems_are_named() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(library_root_problem(dir.path()), None);
        std::fs::create_dir(dir.path().join("leer")).unwrap();
        assert_eq!(library_root_problem(&dir.path().join("leer")), None);
        assert_eq!(
            library_root_problem(&dir.path().join("weg")).as_deref(),
            Some("Verzeichnis existiert nicht")
        );
        let file = dir.path().join("datei.txt");
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            library_root_problem(&file).as_deref(),
            Some("Pfad ist kein Verzeichnis")
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_problem_names_a_directory_it_may_not_read() {
        use std::os::unix::fs::PermissionsExt;
        // Root reads everything; the check is meaningless there.
        if std::fs::read_dir("/root").is_ok() && std::env::var("USER").as_deref() == Ok("root") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("gesperrt");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let problem = library_root_problem(&locked);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(problem.as_deref(), Some("Verzeichnis ist nicht lesbar"));
    }
}
