//! Where downloaded episodes live (legacy `podcasts/paths.ts`):
//! `<podcasts_dir>/<Podcast>/<Folge>.<ext>`.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use sqlx::PgPool;

use crate::api::images::resolve;

const MAX_NAME_LENGTH: usize = 120;

static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\x00-\x1f<>:"/\\|?*]"#).unwrap());
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// Titles come straight from a feed, so they may contain anything: path separators, characters
/// that are illegal on Windows/exFAT shares, control codes, or nothing usable at all. Everything
/// outside the safe set collapses into a single space; trailing dots and spaces go because Windows
/// strips them silently, which would leave the stored path and the file on disk disagreeing.
pub fn safe_file_name(raw: &str, fallback: &str) -> String {
    let cleaned = UNSAFE.replace_all(raw, " ");
    let collapsed = SPACE.replace_all(&cleaned, " ");
    let capped: String = collapsed.trim().chars().take(MAX_NAME_LENGTH).collect();
    let trimmed = capped.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Whether `path` lies at or below `root` once both are resolved.
pub fn is_inside(path: &Path, root: &Path) -> bool {
    resolve(path).starts_with(resolve(root))
}

/// The file extension of a media URL's path, like Node's `extname` — `.mp3` if it has none, or
/// one that is no plain short extension (`.mp3%20` or `.php;x`): it becomes part of a file name
/// on disk.
pub fn suffix_for(media_url: &str) -> String {
    let path = reqwest::Url::parse(media_url)
        .map(|u| u.path().to_owned())
        .unwrap_or_default();
    let base = path.rsplit('/').next().unwrap_or("");
    match base.rfind('.') {
        Some(dot)
            if dot > 0
                && base.len() - dot <= 6
                && base[dot + 1..].bytes().all(|b| b.is_ascii_alphanumeric())
                && base.len() - dot > 1 =>
        {
            base[dot..].to_owned()
        }
        _ => ".mp3".to_owned(),
    }
}

/// Two episodes of one podcast may well share a title, so a name already taken by another file
/// gets the episode id appended instead of overwriting it. The episode's own file is not in the
/// way: re-downloading an episode replaces it in place.
pub async fn episode_destination(
    db: &PgPool,
    podcasts_dir: &Path,
    episode_id: i32,
    episode_title: &str,
    podcast_title: &str,
    suffix: &str,
) -> Result<PathBuf, sqlx::Error> {
    let folder = podcasts_dir.join(safe_file_name(podcast_title, "Podcast"));
    let base = safe_file_name(episode_title, &episode_id.to_string());
    let candidate = folder.join(format!("{base}{suffix}"));
    let with_id = folder.join(format!("{base} ({episode_id}){suffix}"));
    let claimed: Option<i32> = sqlx::query_scalar("SELECT item_id FROM tracks WHERE path = $1")
        .bind(candidate.to_string_lossy().as_ref())
        .fetch_optional(db)
        .await?;
    Ok(match claimed {
        Some(owner) if owner == episode_id => candidate,
        Some(_) => with_id,
        None if tokio::fs::try_exists(&candidate).await.unwrap_or(false) => with_id,
        None => candidate,
    })
}

/// Deleting the last episode of a podcast leaves its folder behind. Drop it once it is empty and
/// still below the podcasts root — removing a folder anything else still lives in fails.
pub async fn prune_empty_podcast_dir(path: &Path, podcasts_dir: &Path) {
    let Some(parent) = path.parent() else { return };
    let dir = resolve(parent);
    let root = resolve(podcasts_dir);
    if dir == root || !dir.starts_with(&root) {
        return;
    }
    let _ = tokio::fs::remove_dir(&dir).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_plain_title_unchanged() {
        assert_eq!(
            safe_file_name("Folge 12 – Über Bäume", "fallback"),
            "Folge 12 – Über Bäume"
        );
    }

    #[test]
    fn replaces_separators_and_characters_that_break_on_windows() {
        assert_eq!(
            safe_file_name("AC/DC: Was nun? <live>", "fallback"),
            "AC DC Was nun live"
        );
        assert_eq!(safe_file_name("a\\b|c*d\"e", "fallback"), "a b c d e");
    }

    #[test]
    fn drops_control_codes_and_collapses_whitespace() {
        assert_eq!(
            safe_file_name("Folge\u{7}1\u{7}  Auftakt", "fallback"),
            "Folge 1 Auftakt"
        );
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(
            safe_file_name("Fortsetzung folgt...  ", "fallback"),
            "Fortsetzung folgt"
        );
    }

    #[test]
    fn caps_the_length() {
        assert_eq!(safe_file_name(&"x".repeat(300), "fallback").len(), 120);
    }

    #[test]
    fn falls_back_when_nothing_usable_is_left() {
        assert_eq!(safe_file_name("", "42"), "42");
        assert_eq!(safe_file_name("///", "42"), "42");
        assert_eq!(safe_file_name("   ", "42"), "42");
    }

    #[test]
    fn suffix_comes_from_the_url_path() {
        assert_eq!(suffix_for("https://x/ep.m4a?token=1"), ".m4a");
        assert_eq!(suffix_for("https://x/episode"), ".mp3");
        assert_eq!(suffix_for("https://x/.hidden"), ".mp3");
        assert_eq!(suffix_for("kaputt"), ".mp3");
        assert_eq!(suffix_for("https://x/ep.mp3%2F..%2Fx"), ".mp3");
        assert_eq!(suffix_for("https://x/ep.verylongext"), ".mp3");
        assert_eq!(suffix_for("https://x/ep."), ".mp3");
    }

    #[test]
    fn inside_means_at_or_below_the_root() {
        assert!(is_inside(Path::new("/d/p/a.mp3"), Path::new("/d/p")));
        assert!(!is_inside(Path::new("/d/p/../x.mp3"), Path::new("/d/p")));
        assert!(!is_inside(Path::new("/d/px/a.mp3"), Path::new("/d/p")));
    }
}
