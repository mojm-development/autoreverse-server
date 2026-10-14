//! The app shell around every logged-in page: sidebar with counts, accent colour per section
//! (legacy `library/+layout.svelte`, `+layout.server.ts`, `Sidebar.svelte`).

use axum::http::Uri;
use sqlx::PgPool;

use crate::auth::WebUser;
use crate::error::{WebError, WebResult};
use crate::library::queries::{artist_count, library_counts, unheard_episode_count};

#[derive(Debug, Clone, Default)]
pub struct SidebarCounts {
    pub albums: i64,
    pub artists: i64,
    pub podcasts: i64,
    pub unread_episodes: i64,
    pub books: i64,
}

/// Everything the shell template needs. Page templates embed it as `shell`.
#[derive(Debug, Clone)]
pub struct Shell {
    /// `book`, `music` or `podcast`: picks `--a` for the sidebar and the page.
    pub accent: &'static str,
    pub active_href: String,
    pub active_query: String,
    pub user_name: String,
    pub is_admin: bool,
    pub counts: SidebarCounts,
}

impl Shell {
    pub async fn load(db: &PgPool, user: &WebUser, uri: &Uri) -> WebResult<Shell> {
        let row: Option<(String, bool)> =
            sqlx::query_as("SELECT name, is_admin FROM users WHERE id = $1")
                .bind(user.id)
                .fetch_optional(db)
                .await?;
        let Some((user_name, is_admin)) = row else {
            return Err(WebError::redirect("/login"));
        };
        let (counts, artists, unread) = tokio::try_join!(
            library_counts(db),
            artist_count(db),
            unheard_episode_count(db, user.id)
        )?;
        let path = uri.path().to_owned();
        Ok(Shell {
            accent: accent_for(&path),
            active_query: uri.query().map(|q| format!("?{q}")).unwrap_or_default(),
            active_href: path,
            user_name,
            is_admin,
            counts: SidebarCounts {
                albums: counts.album_count,
                artists,
                podcasts: counts.podcast_count,
                unread_episodes: unread,
                books: counts.book_count,
            },
        })
    }

    /// `aria-current` for a plain sidebar link.
    pub fn current(&self, href: &str) -> bool {
        self.active_href == href
    }

    /// The two podcast links share a path and differ by `?filter=new`.
    pub fn podcasts_current(&self, new_only: bool) -> bool {
        self.active_href == "/library/podcasts" && (self.active_query == "?filter=new") == new_only
    }

    /// The avatar's two letters.
    pub fn initials(&self) -> String {
        self.user_name
            .chars()
            .take(2)
            .flat_map(char::to_uppercase)
            .collect()
    }

    pub fn role(&self) -> &'static str {
        if self.is_admin { "Verwalter" } else { "Nutzer" }
    }
}

const ACCENTS: &[(&str, &str)] = &[
    ("/library", "book"),
    ("/library/search", "music"),
    ("/library/albums", "music"),
    ("/library/books", "book"),
    ("/library/podcasts", "podcast"),
    ("/library/playlists", "music"),
    ("/library/favorites", "music"),
    ("/library/artists", "music"),
    ("/library/series", "book"),
    ("/library/bookmarks", "book"),
];

/// The accent of the longest section prefix matching `path`; books for anything else.
pub fn accent_for(path: &str) -> &'static str {
    ACCENTS
        .iter()
        .filter(|(prefix, _)| path == *prefix || path.starts_with(&format!("{prefix}/")))
        .max_by_key(|(prefix, _)| prefix.len())
        .map_or("book", |(_, accent)| accent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_follows_the_longest_matching_section() {
        assert_eq!(accent_for("/library"), "book");
        assert_eq!(accent_for("/library/albums"), "music");
        assert_eq!(accent_for("/library/albums/12"), "music");
        assert_eq!(accent_for("/library/podcasts/3"), "podcast");
        assert_eq!(accent_for("/library/albumsX"), "book");
        assert_eq!(accent_for("/settings"), "book");
    }

    fn shell(path: &str, query: &str, name: &str) -> Shell {
        Shell {
            accent: "book",
            active_href: path.into(),
            active_query: query.into(),
            user_name: name.into(),
            is_admin: false,
            counts: SidebarCounts::default(),
        }
    }

    #[test]
    fn podcast_links_tell_the_filter_apart() {
        let all = shell("/library/podcasts", "", "x");
        assert!(all.podcasts_current(false) && !all.podcasts_current(true));
        let new = shell("/library/podcasts", "?filter=new", "x");
        assert!(new.podcasts_current(true) && !new.podcasts_current(false));
    }

    #[test]
    fn initials_are_the_first_two_letters_upper_cased() {
        assert_eq!(shell("/", "", "oliver").initials(), "OL");
        assert_eq!(shell("/", "", "ä").initials(), "Ä");
    }
}
