//! The app shell around every logged-in page: sidebar with counts, accent colour per section,
//! and the mobile navigation (tab bar, area chips, account menu) derived from the route
//! (legacy `library/+layout.svelte`, `+layout.server.ts`, `Sidebar.svelte`).

use axum::http::Uri;
use sqlx::PgPool;

use crate::auth::WebUser;
use crate::auth::preferences::Preferences;
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
    /// Handed to the web player on every page (`data-*` on `#page`), so a change in Settings →
    /// Wiedergabe reaches a player that keeps running across page changes.
    pub preferences: Preferences,
}

impl Shell {
    pub async fn load(db: &PgPool, user: &WebUser, uri: &Uri) -> WebResult<Shell> {
        let row: Option<(String, bool, f64, i32, i32)> = sqlx::query_as(
            "SELECT name, is_admin, playback_speed, skip_back, skip_forward FROM users WHERE id = $1",
        )
        .bind(user.id)
        .fetch_optional(db)
        .await?;
        let Some((user_name, is_admin, playback_speed, skip_back, skip_forward)) = row else {
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
            preferences: Preferences {
                playback_speed,
                skip_back,
                skip_forward,
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

    /// The five tabs of the mobile tab bar, the one for this page marked.
    pub fn tabs(&self) -> Vec<Tab> {
        let area = area_for(&self.active_href);
        TABS.iter()
            .map(|&(id, href, label, icon, accent)| Tab {
                href,
                label,
                icon,
                accent,
                current: area == Some(id),
                badge: if id == Area::Podcasts {
                    self.counts.unread_episodes
                } else {
                    0
                },
            })
            .collect()
    }

    /// The area's sub-navigation (mobile chip row); empty for Start, Suche and the settings.
    pub fn chips(&self) -> Vec<Chip> {
        let current = chip_for(&self.active_href, &self.active_query);
        area_for(&self.active_href)
            .map(area_chips)
            .unwrap_or_default()
            .iter()
            .map(|&(href, label)| Chip {
                href,
                label,
                current: current == Some(href),
            })
            .collect()
    }

    /// `aria-label` of the chip row, the area's name.
    pub fn area_label(&self) -> &'static str {
        match area_for(&self.active_href) {
            Some(Area::Music) => "Musik",
            Some(Area::Podcasts) => "Podcasts",
            Some(Area::Books) => "Hörbücher",
            _ => "",
        }
    }

    /// Settings pages: the account menu, not a tab, is where the visitor is.
    pub fn in_settings(&self) -> bool {
        self.active_href == "/settings" || self.active_href.starts_with("/settings/")
    }
}

/// The top-level areas of the mobile tab bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Home,
    Music,
    Podcasts,
    Books,
    Search,
}

/// One tab of the mobile tab bar. `accent` is empty for Start and Suche, which keep the page's
/// own `--a`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub href: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub accent: &'static str,
    pub current: bool,
    pub badge: i64,
}

/// One chip of an area's sub-navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub href: &'static str,
    pub label: &'static str,
    pub current: bool,
}

type TabSpec = (Area, &'static str, &'static str, &'static str, &'static str);
const TABS: &[TabSpec] = &[
    (Area::Home, "/library", "Start", "home", ""),
    (Area::Music, "/library/albums", "Musik", "music", "music"),
    (
        Area::Podcasts,
        "/library/podcasts",
        "Podcasts",
        "podcast",
        "podcast",
    ),
    (Area::Books, "/library/books", "Hörbücher", "book", "book"),
    (Area::Search, "/library/search", "Suche", "search", ""),
];

const MUSIC_CHIPS: &[(&str, &str)] = &[
    ("/library/albums", "Alben"),
    ("/library/artists", "Interpreten"),
    ("/library/playlists", "Playlists"),
    ("/library/favorites", "Favoriten"),
];
const PODCAST_CHIPS: &[(&str, &str)] = &[
    ("/library/podcasts", "Abos"),
    ("/library/podcasts?filter=new", "Neue Folgen"),
];
const BOOK_CHIPS: &[(&str, &str)] = &[
    ("/library/books", "Bibliothek"),
    ("/library/series", "Serien"),
    ("/library/bookmarks", "Lesezeichen"),
];

fn area_chips(area: Area) -> &'static [(&'static str, &'static str)] {
    match area {
        Area::Music => MUSIC_CHIPS,
        Area::Podcasts => PODCAST_CHIPS,
        Area::Books => BOOK_CHIPS,
        Area::Home | Area::Search => &[],
    }
}

/// `path` is `section` or lies below it (`/library/albums/12`, not `/library/albumsX`).
fn within(path: &str, section: &str) -> bool {
    path.strip_prefix(section)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The chip a page belongs to: the section it lies in, detail pages included (an album under
/// Alben, a podcast and its preview under Abos). `None` outside the three areas.
pub fn chip_for(path: &str, query: &str) -> Option<&'static str> {
    if path == "/library/podcasts" && query == "?filter=new" {
        return Some("/library/podcasts?filter=new");
    }
    [MUSIC_CHIPS, PODCAST_CHIPS, BOOK_CHIPS]
        .into_iter()
        .flatten()
        .map(|&(href, _)| href)
        .find(|href| !href.contains('?') && within(path, href))
}

/// The tab a page belongs to. `None` for the settings and anything outside `/library`: they are
/// reached through the account menu, so no tab claims them.
pub fn area_for(path: &str) -> Option<Area> {
    if path == "/library" {
        return Some(Area::Home);
    }
    if within(path, "/library/search") {
        return Some(Area::Search);
    }
    let chip = chip_for(path, "")?;
    [
        (Area::Music, MUSIC_CHIPS),
        (Area::Podcasts, PODCAST_CHIPS),
        (Area::Books, BOOK_CHIPS),
    ]
    .into_iter()
    .find(|(_, chips)| chips.iter().any(|&(href, _)| href == chip))
    .map(|(area, _)| area)
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
            preferences: Preferences::default(),
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
    fn detail_pages_belong_to_their_area_and_chip() {
        assert_eq!(area_for("/library/albums/7"), Some(Area::Music));
        assert_eq!(
            chip_for("/library/artists/ABBA", ""),
            Some("/library/artists")
        );
        assert_eq!(
            chip_for("/library/podcasts/preview", ""),
            Some("/library/podcasts")
        );
        assert_eq!(area_for("/library/albumsX"), None);
        assert_eq!(area_for("/settings/scan"), None);
        assert_eq!(area_for("/library"), Some(Area::Home));
    }

    #[test]
    fn initials_are_the_first_two_letters_upper_cased() {
        assert_eq!(shell("/", "", "oliver").initials(), "OL");
        assert_eq!(shell("/", "", "ä").initials(), "Ä");
    }
}
