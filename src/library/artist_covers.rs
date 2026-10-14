//! Which picture stands for an artist (legacy `library/artistCovers.ts`): an uploaded image, an
//! album chosen by hand, or else one of their albums picked at random but stably. Reading only —
//! choosing and uploading join with metadata editing (slice 8).

use std::collections::HashMap;

use sqlx::PgPool;

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
           AND missing_since IS NULL
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
    fn an_uploaded_image_means_no_album() {
        let fallback = HashMap::from([("A".to_owned(), 1)]);
        assert_eq!(
            effective_album(Some(&cover(None, Some("/x.png"))), &fallback, "A"),
            None
        );
    }
}
