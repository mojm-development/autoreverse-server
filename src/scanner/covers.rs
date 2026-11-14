//! Cover art: an image file in the folder, or a picture embedded in a track and copied into the
//! server's cover directory (legacy `scanner/covers.ts`).

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
