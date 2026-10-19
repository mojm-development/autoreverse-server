//! Which picture stands for an artist (legacy `library/artistCovers.ts`): an uploaded image, an
//! album chosen by hand, or else one of their albums picked at random but stably.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::error::{ApiError, ApiResult};

/// The image types an artist picture may have, with the extension it is stored under.
pub const IMAGE_TYPES: [(&str, &str); 4] = [
    ("image/jpeg", ".jpg"),
    ("image/png", ".png"),
    ("image/webp", ".webp"),
    ("image/gif", ".gif"),
];

pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const TOO_LARGE: &str = "Bild ist zu groß (maximal 5 MB)";

/// The stored file's name: a hash of the artist (any name is a safe file name that way).
pub fn image_file_name(artist: &str, extension: &str) -> String {
    let digest = Sha256::digest(artist.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}{extension}", &hex[..32])
}

/// One `artist_covers` row: exactly one of `item_id` and `image_path` is set
/// (`artist_cover_xor`).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ArtistCover {
    pub artist: String,
    pub item_id: Option<i32>,
    pub image_path: Option<String>,
}

pub async fn chosen_cover(db: &PgPool, artist: &str) -> Result<Option<ArtistCover>, sqlx::Error> {
    sqlx::query_as("SELECT artist, item_id, image_path FROM artist_covers WHERE artist = $1")
        .bind(artist)
        .fetch_optional(db)
        .await
}

/// Every choice, by artist.
pub async fn chosen_covers(db: &PgPool) -> Result<HashMap<String, ArtistCover>, sqlx::Error> {
    let rows: Vec<ArtistCover> =
        sqlx::query_as("SELECT artist, item_id, image_path FROM artist_covers")
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().map(|c| (c.artist.clone(), c)).collect())
}

/// One present album with artwork per artist, for artists nobody chose a picture for. Ordered by
/// a hash of artist and id rather than by title, so it is not always the alphabetically first
/// album — and the same one on every load.
pub async fn fallback_covers(db: &PgPool) -> Result<HashMap<String, i32>, sqlx::Error> {
    let rows: Vec<(String, i32)> = sqlx::query_as(
        "SELECT DISTINCT ON (artist) artist, id
         FROM items
         WHERE kind = 'album' AND artist IS NOT NULL AND cover_path IS NOT NULL
           AND missing_since IS NULL AND duplicate_of IS NULL
         ORDER BY artist, md5(artist || ':' || id::text)",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// The album whose cover stands for `artist`: the chosen one, else the fallback — `None` when an
/// uploaded image is chosen or the artist has no album with artwork.
pub fn effective_album(
    chosen: Option<&ArtistCover>,
    fallback: &HashMap<String, i32>,
    artist: &str,
) -> Option<i32> {
    match chosen {
        Some(ArtistCover {
            item_id: Some(id), ..
        }) => Some(*id),
        Some(ArtistCover {
            image_path: Some(_),
            ..
        }) => None,
        _ => fallback.get(artist).copied(),
    }
}

/// Replaces the artist's choice; an uploaded file the new choice no longer uses is deleted.
async fn replace(
    db: &PgPool,
    artist: &str,
    item_id: Option<i32>,
    image_path: Option<&str>,
) -> Result<(), sqlx::Error> {
    let previous = chosen_cover(db, artist).await?;
    sqlx::query(
        "INSERT INTO artist_covers (artist, item_id, image_path, updated_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT (artist) DO UPDATE SET item_id = excluded.item_id,
             image_path = excluded.image_path, updated_at = now()",
    )
    .bind(artist)
    .bind(item_id)
    .bind(image_path)
    .execute(db)
    .await?;
    if let Some(old) = previous.and_then(|p| p.image_path)
        && Some(old.as_str()) != image_path
    {
        let _ = tokio::fs::remove_file(old).await;
    }
    Ok(())
}

/// Picks one of the artist's own albums as their picture.
pub async fn select_album(db: &PgPool, artist: &str, item_id: i32) -> ApiResult<()> {
    let found: Option<i32> =
        sqlx::query_scalar("SELECT id FROM items WHERE id = $1 AND kind = 'album' AND artist = $2")
            .bind(item_id)
            .bind(artist)
            .fetch_optional(db)
            .await?;
    if found.is_none() {
        return Err(ApiError::unprocessable(
            "Album gehört nicht zu diesem Interpreten",
        ));
    }
    replace(db, artist, Some(item_id), None).await?;
    Ok(())
}

/// Stores an uploaded picture under `artists_dir` and points the artist at it. `size` is the
/// upload's size, which may be more than `bytes` holds when reading stopped at the limit.
pub async fn store_image(
    db: &PgPool,
    artist: &str,
    content_type: &str,
    size: usize,
    bytes: &[u8],
    artists_dir: &Path,
) -> ApiResult<PathBuf> {
    let extension = IMAGE_TYPES
        .iter()
        .find(|(media, _)| *media == content_type)
        .map(|(_, ext)| *ext)
        .ok_or_else(|| ApiError::unprocessable("Nicht unterstütztes Bildformat"))?;
    if size > MAX_IMAGE_BYTES {
        return Err(ApiError::new(
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            TOO_LARGE,
        ));
    }
    if size == 0 {
        return Err(ApiError::unprocessable("Leere Datei"));
    }
    tokio::fs::create_dir_all(artists_dir)
        .await
        .map_err(ApiError::internal)?;
    let destination = artists_dir.join(image_file_name(artist, extension));
    tokio::fs::write(&destination, bytes)
        .await
        .map_err(ApiError::internal)?;
    let stored = destination.to_string_lossy().into_owned();
    replace(db, artist, None, Some(&stored)).await?;
    Ok(destination)
}

/// Back to the random-but-stable album; an uploaded file is deleted.
pub async fn clear_cover(db: &PgPool, artist: &str) -> Result<(), sqlx::Error> {
    let previous = chosen_cover(db, artist).await?;
    sqlx::query("DELETE FROM artist_covers WHERE artist = $1")
        .bind(artist)
        .execute(db)
        .await?;
    if let Some(path) = previous.and_then(|p| p.image_path) {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

/// One spelling per album artist, case aside: "AC/DC" and "Ac/Dc" from two albums' tags would
/// otherwise be two artists with two tiles, two pictures and two filters. Every album takes the
/// spelling a Verwalter locked by hand, else the one most albums use (alphabetically first on a
/// tie). A hand-locked artist is never rewritten. A picture chosen under another spelling moves
/// along unless the kept spelling already has one. Runs after every scan.
pub async fn unify_artist_spellings(db: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "CREATE TEMP TABLE canonical_artist ON COMMIT DROP AS
         SELECT DISTINCT ON (lower(artist)) lower(artist) AS key, artist
         FROM items
         WHERE kind = 'album' AND artist IS NOT NULL
         GROUP BY lower(artist), artist
         ORDER BY lower(artist), bool_or('artist' = ANY(locked_fields)) DESC, count(*) DESC,
                  artist",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE items SET artist = c.artist
         FROM canonical_artist c
         WHERE items.kind = 'album' AND lower(items.artist) = c.key
           AND items.artist <> c.artist AND NOT ('artist' = ANY(items.locked_fields))",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE artist_covers SET artist = c.artist, updated_at = now()
         FROM canonical_artist c
         WHERE lower(artist_covers.artist) = c.key AND artist_covers.artist <> c.artist
           AND NOT EXISTS (SELECT 1 FROM artist_covers kept WHERE kept.artist = c.artist)
           AND artist_covers.artist = (SELECT min(other.artist) FROM artist_covers other
                                       WHERE lower(other.artist) = c.key)",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover(item_id: Option<i32>, image_path: Option<&str>) -> ArtistCover {
        ArtistCover {
            artist: "A".into(),
            item_id,
            image_path: image_path.map(Into::into),
        }
    }

    #[test]
    fn a_chosen_album_beats_the_fallback() {
        let fallback = HashMap::from([("A".to_owned(), 1)]);
        assert_eq!(
            effective_album(Some(&cover(Some(2), None)), &fallback, "A"),
            Some(2)
        );
        assert_eq!(effective_album(None, &fallback, "A"), Some(1));
        assert_eq!(effective_album(None, &fallback, "B"), None);
    }

    #[test]
    fn file_names_are_a_hash_of_the_artist() {
        let name = image_file_name("AC/DC", ".png");
        assert_eq!(name.len(), 36);
        assert!(name.ends_with(".png"));
        assert!(!name.contains('/'));
        assert_eq!(name, image_file_name("AC/DC", ".png"));
        assert_ne!(name, image_file_name("ACDC", ".png"));
    }

    #[test]
    fn an_uploaded_image_means_no_album() {
        let fallback = HashMap::from([("A".to_owned(), 1)]);
        assert_eq!(
            effective_album(Some(&cover(None, Some("/x.png"))), &fallback, "A"),
            None
        );
    }
}
