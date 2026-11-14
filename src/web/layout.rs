//! The app shell around every logged-in page: sidebar with counts, accent colour per section,
//! and the mobile navigation (tab bar, area chips, account menu) derived from the route
//! (legacy `library/+layout.svelte`, `+layout.server.ts`, `Sidebar.svelte`).

use axum::http::Uri;

use crate::auth::WebUser;
use crate::auth::preferences::Preferences;
use crate::error::{WebError, WebResult};
use crate::library::queries::{artist_count, library_counts, unheard_episode_count};
use crate::state::AppState;

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
    /// Whether the server offers internet radio (`AUTOREVERSE_RADIO`): without it the sidebar
    /// and the Musik chips have no "Radio".
    pub radio: bool,
    /// Whether the server offers remote control between devices (`AUTOREVERSE_DEVICES`): without
    /// it the web player neither reports itself as a device nor shows the devices button.
    pub devices: bool,
}

impl Shell {
    pub async fn load(state: &AppState, user: &WebUser, uri: &Uri) -> WebResult<Shell> {
        let db = &state.db;
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
            radio: state.config.radio,
            devices: state.config.devices,
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

    /// The area's sub-navigation (mobile chip row); empty for Start, Entdecken, the search and the
    /// settings.
    pub fn chips(&self) -> Vec<Chip> {
        let current = chip_for(&self.active_href, &self.active_query);
        area_for(&self.active_href)
            .map(area_chips)
            .unwrap_or_default()
            .iter()
            .filter(|&&(href, _)| self.radio || href != RADIO_HREF)
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

    /// The statistics and the year in review are one entry of the sidebar and the account menu.
    pub fn in_stats(&self) -> bool {
        matches!(
            self.active_href.as_str(),
            "/library/stats" | "/library/review"
        )
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
    Discover,
}

/// One tab of the mobile tab bar. `accent` is empty for Start and Entdecken, which keep the
/// page's own `--a`.
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
    (
        Area::Discover,
        "/library/discover",
        "Entdecken",
        "discover",
        "",
    ),
    (Area::Music, "/library/albums", "Musik", "music", "music"),
    (
        Area::Podcasts,
        "/library/podcasts",
        "Podcasts",
        "podcast",
        "podcast",
    ),
    (Area::Books, "/library/books", "Hörbücher", "book", "book"),
];

/// The page of the internet radio stations (`web::stations`).
pub const RADIO_HREF: &str = "/library/radio";

const MUSIC_CHIPS: &[(&str, &str)] = &[
    ("/library/albums", "Alben"),
    ("/library/artists", "Interpreten"),
    ("/library/playlists", "Playlists"),
    ("/library/favorites", "Favoriten"),
    (RADIO_HREF, "Radio"),
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
        Area::Home | Area::Discover => &[],
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

/// The tab a page belongs to. `None` for the search, the settings and anything outside
/// `/library`: they are reached through the header, so no tab claims them.
pub fn area_for(path: &str) -> Option<Area> {
    if path == "/library" {
        return Some(Area::Home);
    }
    if within(path, "/library/discover") {
        return Some(Area::Discover);
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
    ("/library/discover", "music"),
    ("/library/stats", "music"),
    ("/library/review", "music"),
    ("/library/albums", "music"),
    ("/library/books", "book"),
    ("/library/podcasts", "podcast"),
    ("/library/playlists", "music"),
    ("/library/favorites", "music"),
    ("/library/radio", "music"),
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
            radio: true,
            devices: true,
        }
    }

    #[test]
    fn radio_is_a_music_chip_only_where_the_server_offers_it() {
        let labels =
            |shell: &Shell| -> Vec<&str> { shell.chips().iter().map(|chip| chip.label).collect() };
        let mut on = shell("/library/radio", "", "x");
        assert_eq!(area_for("/library/radio"), Some(Area::Music));
        assert_eq!(accent_for("/library/radio"), "music");
        assert_eq!(
            labels(&on),
            ["Alben", "Interpreten", "Playlists", "Favoriten", "Radio"]
        );
        assert!(on.chips().last().unwrap().current);
        on.radio = false;
        assert_eq!(
            labels(&on),
            ["Alben", "Interpreten", "Playlists", "Favoriten"]
        );
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
        assert_eq!(area_for("/library/discover"), Some(Area::Discover));
        // The search sits in the header, so no tab lights up for it.
        assert_eq!(area_for("/library/search"), None);
    }

    #[test]
    fn initials_are_the_first_two_letters_upper_cased() {
        assert_eq!(shell("/", "", "oskar").initials(), "OS");
        assert_eq!(shell("/", "", "ä").initials(), "Ä");
    }
}
