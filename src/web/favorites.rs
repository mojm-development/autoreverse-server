//! `/library/favorites` and `/library/bookmarks` (legacy `library/favorites`, `library/bookmarks`):
//! the user's own collections across the whole library.

use askama::Template;
use axum::extract::State;
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::format::format_hms;
use super::layout::Shell;
use crate::auth::WebUser;
use crate::error::WebResult;
use crate::library::bookmarks::all_bookmarks;
use crate::library::favorites::{list_favorite_items, list_favorite_tracks};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/favorites", get(favorites))
        .route("/library/bookmarks", get(bookmarks))
}

/// The detail page of an item (legacy `hrefFor`): books and albums have their own, everything
/// else lives under podcasts.
pub fn href_for(kind: &str, id: i32) -> String {
    let segment = match kind {
        "book" => "books",
        "album" => "albums",
        _ => "podcasts",
    };
    format!("/library/{segment}/{id}")
}

/// The accent token of a kind, for placeholders and dots.
pub fn kind_accent(kind: &str) -> &'static str {
    match kind {
        "book" => "book",
        "album" => "music",
        _ => "podcast",
    }
}

pub struct FavoriteTile {
    pub id: i32,
    pub href: String,
    pub accent: &'static str,
    pub has_cover: bool,
    pub title: String,
    pub subtitle: String,
}

pub struct FavoriteTrackRow {
    pub href: String,
    pub title: String,
    pub item_title: String,
}

#[derive(Template)]
#[template(path = "pages/library/favorites.html")]
struct FavoritesPage {
    shell: Shell,
    items: Vec<FavoriteTile>,
    tracks: Vec<FavoriteTrackRow>,
}

async fn favorites(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    let (shell, items, tracks) = tokio::try_join!(
        Shell::load(&state.db, &user, &uri),
        async { Ok(list_favorite_items(&state.db, user.id).await?) },
        async { Ok(list_favorite_tracks(&state.db, user.id).await?) },
    )?;
    let page = FavoritesPage {
        shell,
        items: items
            .into_iter()
            .map(|item| FavoriteTile {
                id: item.id,
                href: href_for(&item.kind, item.id),
                accent: kind_accent(&item.kind),
                has_cover: item.cover_path.is_some(),
                subtitle: item.artist.or(item.author).unwrap_or_default(),
                title: item.title,
            })
            .collect(),
        tracks: tracks
            .into_iter()
            .map(|f| FavoriteTrackRow {
                href: href_for(&f.item.kind, f.item.id),
                title: f.track.title.unwrap_or_else(|| f.item.title.clone()),
                item_title: f.item.title,
            })
            .collect(),
    };
    Ok(Html(page.render()?))
}

pub struct BookmarkLine {
    pub href: String,
    pub item_title: String,
    pub title: String,
    pub position: String,
}

#[derive(Template)]
#[template(path = "pages/library/bookmarks.html")]
struct BookmarksPage {
    shell: Shell,
    bookmarks: Vec<BookmarkLine>,
}

async fn bookmarks(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    let (shell, rows) = tokio::try_join!(Shell::load(&state.db, &user, &uri), async {
        Ok(all_bookmarks(&state.db, user.id).await?)
    })?;
    let page = BookmarksPage {
        shell,
        bookmarks: rows
            .into_iter()
            .map(|row| BookmarkLine {
                href: href_for(&row.item.kind, row.item.id),
                item_title: row.item.title,
                title: row.bookmark.title,
                position: format_hms(row.bookmark.position),
            })
            .collect(),
    };
    Ok(Html(page.render()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_go_to_the_detail_page_of_the_kind() {
        assert_eq!(href_for("book", 3), "/library/books/3");
        assert_eq!(href_for("album", 4), "/library/albums/4");
        assert_eq!(href_for("podcast", 5), "/library/podcasts/5");
        assert_eq!(href_for("episode", 6), "/library/podcasts/6");
    }

    #[test]
    fn episodes_and_shows_share_the_podcast_accent() {
        assert_eq!(kind_accent("book"), "book");
        assert_eq!(kind_accent("album"), "music");
        assert_eq!(kind_accent("episode"), "podcast");
    }
}
