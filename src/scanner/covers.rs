//! Cover art: an image file in the folder, or a picture embedded in a track and copied into the
//! server's cover directory (legacy `scanner/covers.ts`). Beyond legacy, a folder without an
//! exactly named cover falls back to any other image in it, and the search for an embedded
//! picture goes past the first three tracks (both bounded, see `find_any_image` and
//! `embedded_candidates`).

use std::path::{Path, PathBuf};

const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const SUFFIXES: &[&str] = &[".jpg", ".jpeg", ".png", ".webp"];

/// `cover.jpg` before `folder.jpg`: names in priority order, each with every suffix, compared
/// case-insensitively. Directories that happen to carry such a name do not count.
pub fn find_cover_file(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let files: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            Some((name.to_lowercase(), e.path()))
        })
        .collect();
    NAMES.iter().find_map(|name| {
        SUFFIXES.iter().find_map(|suffix| {
            let wanted = format!("{name}{suffix}");
            files
                .iter()
                .find(|(lower, _)| *lower == wanted)
                .map(|(_, path)| path.clone())
        })
    })
}

/// Smaller image files are placeholders or thumbnails, never a cover worth showing.
const MIN_FALLBACK_BYTES: u64 = 1024;
/// Words in a file name that point at a cover: preferred among the fallback images.
const COVER_WORDS: &[&str] = &["cover", "front"];
/// How many tracks are asked for an embedded picture first, and how many more after them.
const FIRST_TRACKS: usize = 3;
const MORE_TRACKS: usize = 10;

/// Whether a name looks like back, inside or disc art: its words start with "back", "rueck",
/// "rück" or "inlay", or are "cd"/"disc" with an optional number ("CD1", "disc_2" → "disc").
/// Matched by word so that a title merely containing "cd" or "back" somewhere still counts.
fn is_secondary_art(lower_stem: &str) -> bool {
    lower_stem
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .any(|word| {
            ["back", "rueck", "rück", "inlay"]
                .iter()
                .any(|prefix| word.starts_with(prefix))
                || ["cd", "disc"].iter().any(|prefix| {
                    word.strip_prefix(prefix)
                        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
                })
        })
}

/// The fallback when no exactly named cover exists: any image file in `dir`, chosen by
///
/// 1. leaving out hidden files (`._x.jpg` from macOS), files under 1 KB and — unless nothing
///    else is there — back, inside and disc art (`is_secondary_art`);
/// 2. preferring names that contain "cover" or "front" (`cover_front.jpg`, `Fall 29 - Cover.png`);
/// 3. taking the largest file of what is left (a single file is simply that one), ties by name.
pub fn find_any_image(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    // (lowercase stem, size, file name, path)
    let images: Vec<(String, u64, String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(|m| m.is_file())?;
            let name = e.file_name().into_string().ok()?;
            let lower = name.to_lowercase();
            let suffix = SUFFIXES.iter().find(|s| lower.ends_with(*s))?;
            if lower.starts_with('.') || meta.len() < MIN_FALLBACK_BYTES {
                return None;
            }
            let stem = lower[..lower.len() - suffix.len()].to_string();
            Some((stem, meta.len(), name, e.path()))
        })
        .collect();
    let (primary, secondary): (Vec<_>, Vec<_>) = images
        .into_iter()
        .partition(|(stem, ..)| !is_secondary_art(stem));
    let pool = if primary.is_empty() {
        secondary
    } else {
        primary
    };
    let (named, rest): (Vec<_>, Vec<_>) = pool
        .into_iter()
        .partition(|(stem, ..)| COVER_WORDS.iter().any(|w| stem.contains(w)));
    let pool = if named.is_empty() { rest } else { named };
    pool.into_iter()
        // Largest first; among equal sizes the first name.
        .min_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)))
        .map(|(.., path)| path)
}

/// The cover image in the item's folder or the first track's folder: an exactly named cover in
/// either first (`find_cover_file`), only then any other image (`find_any_image`), in the same
/// folder order. `first_folder` may be `dir` itself.
pub fn find_folder_cover(dir: &Path, first_folder: Option<&Path>) -> Option<PathBuf> {
    let folders: Vec<&Path> = std::iter::once(dir)
        .chain(first_folder.filter(|f| *f != dir))
        .collect();
    folders
        .iter()
        .find_map(|f| find_cover_file(f))
        .or_else(|| folders.iter().find_map(|f| find_any_image(f)))
}

/// Which of `count` tracks are asked for an embedded picture, in order: the first three, then
/// at most ten more spread evenly over the rest — an item of 2000 tracks must not have every
/// file read on each scan.
pub fn embedded_candidates(count: usize) -> Vec<usize> {
    let first = count.min(FIRST_TRACKS);
    let rest = count - first;
    let more = rest.min(MORE_TRACKS);
    // `rest >= more`, so the step is at least one and no index repeats.
    (0..first)
        .chain((0..more).map(|i| first + i * rest / more))
        .collect()
}

/// The file suffix for an image, by its magic bytes.
pub fn sniff_suffix(data: &[u8]) -> &'static str {
    const MAGIC: &[(&[u8], &str)] = &[
        (&[0xff, 0xd8, 0xff], ".jpg"),
        (&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a], ".png"),
        (b"RIFF", ".webp"),
        (b"GIF8", ".gif"),
    ];
    MAGIC
        .iter()
        .find(|(magic, _)| data.starts_with(magic))
        .map_or(".bin", |(_, suffix)| suffix)
}

/// Writes `<covers_dir>/<item_id><suffix>` atomically (temp file + rename).
pub fn write_cover_bytes(covers_dir: &Path, item_id: i32, data: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(covers_dir)?;
    let destination = covers_dir.join(format!("{item_id}{}", sniff_suffix(data)));
    let mut temp = destination.clone().into_os_string();
    temp.push(".part");
    std::fs::write(&temp, data)?;
    std::fs::rename(&temp, &destination)?;
    Ok(destination)
}

/// Copies the first picture embedded in `track` into the cover directory.
pub fn extract_embedded(track: &Path, covers_dir: &Path, item_id: i32) -> Option<PathBuf> {
    let picture = super::tags::embedded_picture(track)?;
    write_cover_bytes(covers_dir, item_id, &picture).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn finds_cover_before_folder_name_then_suffix_priority() {
        let dir = temp_dir();
        std::fs::write(dir.path().join("folder.jpg"), "").unwrap();
        std::fs::write(dir.path().join("cover.png"), "").unwrap();
        assert_eq!(
            find_cover_file(dir.path()),
            Some(dir.path().join("cover.png"))
        );
    }

    #[test]
    fn matches_case_insensitively() {
        let dir = temp_dir();
        std::fs::write(dir.path().join("Cover.JPG"), "").unwrap();
        assert_eq!(
            find_cover_file(dir.path()),
            Some(dir.path().join("Cover.JPG"))
        );
    }

    #[test]
    fn returns_none_when_no_candidate_exists() {
        assert_eq!(find_cover_file(temp_dir().path()), None);
    }

    #[test]
    fn ignores_directories_named_like_cover_files() {
        let dir = temp_dir();
        std::fs::create_dir(dir.path().join("cover.jpg")).unwrap();
        assert_eq!(find_cover_file(dir.path()), None);
        std::fs::write(dir.path().join("folder.png"), "").unwrap();
        assert_eq!(
            find_cover_file(dir.path()),
            Some(dir.path().join("folder.png"))
        );
    }

    /// An image file of `bytes` bytes (the content does not matter to the folder lookup).
    fn image(dir: &Path, name: &str, bytes: usize) {
        std::fs::write(dir.join(name), vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn exact_name_wins_over_fallback_images() {
        let dir = temp_dir();
        image(dir.path(), "Fall 29 - Cover.png", 50_000);
        image(dir.path(), "folder.jpg", 2_000);
        assert_eq!(
            find_folder_cover(dir.path(), None),
            Some(dir.path().join("folder.jpg"))
        );
    }

    #[test]
    fn fallback_prefers_names_with_cover_or_front() {
        let dir = temp_dir();
        image(dir.path(), "scan_big.jpg", 90_000);
        image(dir.path(), "Fall 29 - Cover.png", 5_000);
        assert_eq!(
            find_any_image(dir.path()),
            Some(dir.path().join("Fall 29 - Cover.png"))
        );
        let dir = temp_dir();
        image(dir.path(), "scan_big.jpg", 90_000);
        image(dir.path(), "cover_front.jpg", 5_000);
        assert_eq!(
            find_any_image(dir.path()),
            Some(dir.path().join("cover_front.jpg"))
        );
    }

    #[test]
    fn fallback_takes_a_single_image_whatever_its_name() {
        let dir = temp_dir();
        image(dir.path(), "Sherlock Holmes 29.JPG", 3_000);
        std::fs::write(dir.path().join("notes.txt"), vec![0u8; 9_000]).unwrap();
        assert_eq!(
            find_any_image(dir.path()),
            Some(dir.path().join("Sherlock Holmes 29.JPG"))
        );
    }

    #[test]
    fn fallback_takes_the_largest_image_ties_by_name() {
        let dir = temp_dir();
        image(dir.path(), "a.jpg", 2_000);
        image(dir.path(), "b.webp", 8_000);
        image(dir.path(), "c.png", 4_000);
        assert_eq!(find_any_image(dir.path()), Some(dir.path().join("b.webp")));
        let dir = temp_dir();
        image(dir.path(), "z.jpg", 4_000);
        image(dir.path(), "m.jpg", 4_000);
        assert_eq!(find_any_image(dir.path()), Some(dir.path().join("m.jpg")));
    }

    #[test]
    fn fallback_ignores_hidden_tiny_and_non_image_files() {
        let dir = temp_dir();
        image(dir.path(), "._cover.jpg", 50_000);
        image(dir.path(), ".DS_Store", 50_000);
        image(dir.path(), "thumb.jpg", 1_023);
        image(dir.path(), "booklet.pdf", 50_000);
        std::fs::create_dir(dir.path().join("big.jpg")).unwrap();
        assert_eq!(find_any_image(dir.path()), None);
        image(dir.path(), "real.jpg", 1_024);
        assert_eq!(
            find_any_image(dir.path()),
            Some(dir.path().join("real.jpg"))
        );
    }

    #[test]
    fn fallback_uses_back_and_disc_art_only_as_a_last_resort() {
        let dir = temp_dir();
        image(dir.path(), "Back.jpg", 90_000);
        image(dir.path(), "Rückseite.jpg", 90_000);
        image(dir.path(), "rueckseite.jpg", 90_000);
        image(dir.path(), "inlay.jpg", 90_000);
        image(dir.path(), "CD1.jpg", 90_000);
        image(dir.path(), "disc_cover.jpg", 90_000);
        image(dir.path(), "back_cover.jpg", 90_000);
        image(dir.path(), "artwork.jpg", 2_000);
        assert_eq!(
            find_any_image(dir.path()),
            Some(dir.path().join("artwork.jpg"))
        );
        std::fs::remove_file(dir.path().join("artwork.jpg")).unwrap();
        // Only secondary art left: it is used, a cover word still first, then size and name.
        assert_eq!(
            find_any_image(dir.path()),
            Some(dir.path().join("back_cover.jpg"))
        );
    }

    #[test]
    fn secondary_art_is_matched_by_word() {
        assert!(is_secondary_art("cd2"));
        assert!(is_secondary_art("fall 29 - backcover"));
        assert!(!is_secondary_art("abcd"));
        assert!(!is_secondary_art("cdcover"));
        assert!(!is_secondary_art("discovery"));
    }

    #[test]
    fn folder_cover_checks_exact_names_in_both_folders_before_any_image() {
        let item = temp_dir();
        let disc = temp_dir();
        image(item.path(), "scan.jpg", 50_000);
        image(disc.path(), "cover.jpg", 2_000);
        assert_eq!(
            find_folder_cover(item.path(), Some(disc.path())),
            Some(disc.path().join("cover.jpg"))
        );
        // Without exact names the item folder's image comes before the first track's folder.
        std::fs::remove_file(disc.path().join("cover.jpg")).unwrap();
        image(disc.path(), "Front Scan.png", 90_000);
        image(disc.path(), "Front 1.png", 80_000);
        assert_eq!(
            find_folder_cover(item.path(), Some(disc.path())),
            Some(item.path().join("scan.jpg"))
        );
        std::fs::remove_file(item.path().join("scan.jpg")).unwrap();
        assert_eq!(
            find_folder_cover(item.path(), Some(disc.path())),
            Some(disc.path().join("Front Scan.png"))
        );
    }

    #[test]
    fn embedded_candidates_are_the_first_three_then_ten_spread_evenly() {
        assert_eq!(embedded_candidates(0), Vec::<usize>::new());
        assert_eq!(embedded_candidates(2), vec![0, 1]);
        assert_eq!(embedded_candidates(8), (0..8).collect::<Vec<_>>());
        assert_eq!(embedded_candidates(13), (0..13).collect::<Vec<_>>());
        assert_eq!(
            embedded_candidates(23),
            vec![0, 1, 2, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21]
        );
        let many = embedded_candidates(2000);
        assert_eq!(many.len(), 13);
        assert_eq!(&many[..4], &[0, 1, 2, 3]);
        assert!(many.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(many.last(), Some(&1800));
    }

    #[test]
    fn sniffs_the_image_type() {
        assert_eq!(sniff_suffix(&[0xff, 0xd8, 0xff, 0xe0]), ".jpg");
        assert_eq!(sniff_suffix(b"\x89PNG\r\n\x1a\nrest"), ".png");
        assert_eq!(sniff_suffix(b"RIFF...."), ".webp");
        assert_eq!(sniff_suffix(b"hello"), ".bin");
    }

    #[test]
    fn writes_the_cover_under_the_item_id() {
        let dir = temp_dir();
        let path = write_cover_bytes(&dir.path().join("covers"), 7, b"\x89PNG\r\n\x1a\n").unwrap();
        assert_eq!(path, dir.path().join("covers/7.png"));
        assert!(path.exists());
    }
}
