//! The two library roots (legacy `settings/libraryPaths.ts`). They live in the single
//! `library_config` row and are set at runtime under Settings → Bibliotheken, not via the
//! environment.

use sqlx::PgPool;

const ROW_ID: i32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryPaths {
    pub books_dir: Option<String>,
    pub music_dir: Option<String>,
}

/// Trims and strips trailing slashes, so the scanner's `root/` prefix matches the stored source
/// paths. The filesystem root stays `/`.
pub fn normalize_dir(path: &str) -> String {
    let trimmed = path.trim();
    let stripped = trimmed.trim_end_matches('/');
    if stripped.is_empty() && !trimmed.is_empty() {
        "/".to_owned()
    } else {
        stripped.to_owned()
    }
}

pub async fn get_library_paths(db: &PgPool) -> Result<LibraryPaths, sqlx::Error> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT books_dir, music_dir FROM library_config WHERE id = $1")
            .bind(ROW_ID)
            .fetch_optional(db)
            .await?;
    let (books, music) = row.unwrap_or_default();
    let clean = |dir: Option<String>| dir.filter(|d| !d.is_empty()).map(|d| normalize_dir(&d));
    Ok(LibraryPaths {
        books_dir: clean(books),
        music_dir: clean(music),
    })
}

pub async fn set_library_paths(
    db: &PgPool,
    books_dir: &str,
    music_dir: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO library_config (id, books_dir, music_dir) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO UPDATE
         SET books_dir = excluded.books_dir, music_dir = excluded.music_dir, updated_at = now()",
    )
    .bind(ROW_ID)
    .bind(normalize_dir(books_dir))
    .bind(normalize_dir(music_dir))
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_slashes_so_the_scanner_prefix_matches_stored_source_paths() {
        assert_eq!(normalize_dir("/media/musik/"), "/media/musik");
        assert_eq!(normalize_dir("/media/musik///"), "/media/musik");
        assert_eq!(normalize_dir("  /media/musik/  "), "/media/musik");
    }

    #[test]
    fn leaves_a_plain_path_and_the_filesystem_root_alone() {
        assert_eq!(normalize_dir("/media/musik"), "/media/musik");
        assert_eq!(normalize_dir("/"), "/");
        assert_eq!(normalize_dir(""), "");
    }
}
