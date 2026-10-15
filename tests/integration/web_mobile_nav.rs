//! The mobile navigation at the HTTP level: which tab and which chip each library page marks
//! (detail pages included), the badge on the Podcasts tab, and the header with the account menu.

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_text, seed};

const TABS: [&str; 5] = [
    "/library",
    "/library/albums",
    "/library/podcasts",
    "/library/books",
    "/library/search",
];
const CHIPS: [&str; 9] = [
    "/library/albums",
    "/library/artists",
    "/library/playlists",
    "/library/favorites",
    "/library/podcasts",
    "/library/podcasts?filter=new",
    "/library/books",
    "/library/series",
    "/library/bookmarks",
];

async fn page(app: &TestApp, uri: &str, cookie: &str) -> String {
    let response = app.get_with_cookie(uri, cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

/// The markup of the `<nav>` that starts with `opening`, or "" when the page has none.
fn nav<'a>(html: &'a str, opening: &str) -> &'a str {
    let Some(start) = html.find(opening) else {
        return "";
    };
    let end = html[start..].find("</nav>").unwrap();
    &html[start..start + end]
}

fn tab_bar(html: &str) -> &str {
    nav(html, r#"<nav class="tabbar" aria-label="Bereiche""#)
}

fn current_tabs(html: &str) -> Vec<&'static str> {
    let bar = tab_bar(html);
    TABS.into_iter()
        .filter(|href| bar.contains(&format!(r#"<a href="{href}" aria-current="page""#)))
        .collect()
}

fn chips(html: &str) -> &str {
    nav(html, r#"<nav class="area-chips""#)
}

fn current_chips(html: &str) -> Vec<&'static str> {
    let row = chips(html);
    CHIPS
        .into_iter()
        .filter(|href| {
            row.contains(&format!(
                r#"<a href="{href}" class="pill" aria-current="true""#
            ))
        })
        .collect()
}

#[sqlx::test]
async fn every_library_page_marks_its_tab_and_chip(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let cookie = format!("autoreverse_session={}", app.token(user).await);
    let album = seed(
        app.db(),
        Seed {
            artist: Some("ABBA"),
            ..Seed::new("album", "Arrival")
        },
    )
    .await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    let podcast = seed(app.db(), Seed::new("podcast", "Maschinenraum")).await;
    let playlist: i32 = sqlx::query_scalar(
        "INSERT INTO playlists (user_id, name) VALUES ($1, 'Unterwegs') RETURNING id",
    )
    .bind(user)
    .fetch_one(app.db())
    .await
    .unwrap();

    let album_href = format!("/library/albums/{album}");
    let book_href = format!("/library/books/{book}");
    let podcast_href = format!("/library/podcasts/{podcast}");
    let playlist_href = format!("/library/playlists/{playlist}");
    let cases: Vec<(&str, &str, Option<&str>, &str)> = vec![
        // (page, tab, chip, chip row label)
        ("/library", "/library", None, ""),
        ("/library/search", "/library/search", None, ""),
        ("/library/search?q=abba", "/library/search", None, ""),
        (
            "/library/albums",
            "/library/albums",
            Some("/library/albums"),
            "Musik",
        ),
        (
            &album_href,
            "/library/albums",
            Some("/library/albums"),
            "Musik",
        ),
        (
            "/library/artists",
            "/library/albums",
            Some("/library/artists"),
            "Musik",
        ),
        (
            "/library/artists/ABBA",
            "/library/albums",
            Some("/library/artists"),
            "Musik",
        ),
        (
            "/library/playlists",
            "/library/albums",
            Some("/library/playlists"),
            "Musik",
        ),
        (
            &playlist_href,
            "/library/albums",
            Some("/library/playlists"),
            "Musik",
        ),
        (
            "/library/favorites",
            "/library/albums",
            Some("/library/favorites"),
            "Musik",
        ),
        (
            "/library/podcasts",
            "/library/podcasts",
            Some("/library/podcasts"),
            "Podcasts",
        ),
        (
            "/library/podcasts?filter=new",
            "/library/podcasts",
            Some("/library/podcasts?filter=new"),
            "Podcasts",
        ),
        (
            &podcast_href,
            "/library/podcasts",
            Some("/library/podcasts"),
            "Podcasts",
        ),
        (
            "/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml",
            "/library/podcasts",
            Some("/library/podcasts"),
            "Podcasts",
        ),
        (
            "/library/books",
            "/library/books",
            Some("/library/books"),
            "Hörbücher",
        ),
        (
            &book_href,
            "/library/books",
            Some("/library/books"),
            "Hörbücher",
        ),
        (
            "/library/series",
            "/library/books",
            Some("/library/series"),
            "Hörbücher",
        ),
        (
            "/library/bookmarks",
            "/library/books",
            Some("/library/bookmarks"),
            "Hörbücher",
        ),
    ];
    for (uri, tab, chip, label) in cases {
        let html = page(&app, uri, &cookie).await;
        assert_eq!(current_tabs(&html), vec![tab], "tab of {uri}");
        assert_eq!(
            current_chips(&html),
            chip.into_iter().collect::<Vec<_>>(),
            "chip of {uri}"
        );
        if label.is_empty() {
            assert_eq!(chips(&html), "", "no chips on {uri}");
        } else {
            assert!(
                chips(&html).contains(&format!(r#"aria-label="{label}""#)),
                "chip row of {uri}"
            );
        }
    }
}

#[sqlx::test]
async fn the_tabs_carry_labels_icons_and_the_area_colour(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let cookie = format!("autoreverse_session={}", app.token(user).await);
    let html = page(&app, "/library/albums", &cookie).await;
    let bar = tab_bar(&html);
    for (label, icon) in [
        ("Start", "home"),
        ("Musik", "music"),
        ("Podcasts", "podcast"),
        ("Hörbücher", "book"),
        ("Suche", "search"),
    ] {
        assert!(
            bar.contains(&format!(r#"<span class="tab-label">{label}</span>"#)),
            "{label}"
        );
        assert!(
            bar.contains(&format!(r##"<use href="#icon-{icon}">"##)),
            "{icon}"
        );
    }
    assert!(
        bar.contains(
            r#"<a href="/library/albums" aria-current="page" style="--tab: var(--music)">"#
        )
    );
    assert!(bar.contains(r#"<a href="/library/podcasts" style="--tab: var(--podcast)">"#));
    // Chips for Musik, in sidebar order.
    let row = chips(&html);
    let order: Vec<usize> = ["Alben", "Interpreten", "Playlists", "Favoriten"]
        .iter()
        .map(|label| row.find(&format!(">{label}</a>")).unwrap())
        .collect();
    assert!(order.is_sorted(), "{row}");
}

#[sqlx::test]
async fn the_podcasts_tab_shows_unheard_episodes(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let cookie = format!("autoreverse_session={}", app.token(user).await);
    let html = page(&app, "/library", &cookie).await;
    assert!(!tab_bar(&html).contains("badge"));

    let podcast = seed(
        app.db(),
        Seed {
            added_at: Some(Utc::now() - Duration::days(7)),
            ..Seed::new("podcast", "Maschinenraum")
        },
    )
    .await;
    for title in ["Folge 1", "Folge 2"] {
        sqlx::query(
            "INSERT INTO items (kind, parent_id, title, sort_title, guid, published_at)
             VALUES ('episode', $1, $2, lower($2), $2, now() - interval '1 day')",
        )
        .bind(podcast)
        .bind(title)
        .execute(app.db())
        .await
        .unwrap();
    }
    let html = page(&app, "/library", &cookie).await;
    let bar = tab_bar(&html);
    assert!(bar.contains(r#"<span class="badge" aria-hidden="true">2</span>"#));
    assert!(bar.contains(r#"<span class="vh">, 2 neue Folgen</span>"#));
    assert_eq!(bar.matches("badge").count(), 1, "only on Podcasts");
}

#[sqlx::test]
async fn the_header_holds_the_account_menu(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", true).await;
    let cookie = format!("autoreverse_session={}", app.token(user).await);
    let html = page(&app, "/library/books", &cookie).await;
    let start = html.find(r#"<header class="m-top""#).unwrap();
    let header = &html[start..start + html[start..].find("</header>").unwrap()];
    assert!(header.contains(r#"<a href="/library" class="m-brand" aria-label="Zur Startseite">"#));
    assert!(header.contains(r#"<span class="wordmark">Autoreverse</span>"#));
    assert!(header.contains(r#"<details class="m-account" data-dismissable>"#));
    assert!(header.contains(
        r#"<summary class="m-avatar" aria-label="Konto: oliver"><span class="m-initials">OL</span></summary>"#
    ));
    assert!(header.contains(r#"<span class="role mono">Verwalter</span>"#));
    assert!(header.contains(r#"<a href="/settings" class="m-item">"#));
    assert!(header.contains("<span>Einstellungen</span>"));
    assert!(header.contains("data-theme-toggle"));
    assert!(header.contains("<span data-theme-label>Helles Thema</span>"));
    assert!(header.contains(r#"<form method="POST" action="/logout" class="m-logout">"#));
    assert!(header.contains("<span>Abmelden</span>"));
}

#[sqlx::test]
async fn settings_pages_mark_the_avatar_not_a_tab(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", true).await;
    let cookie = format!("autoreverse_session={}", app.token(user).await);
    for uri in ["/settings", "/settings/scan", "/settings/about"] {
        let html = page(&app, uri, &cookie).await;
        assert_eq!(current_tabs(&html), Vec::<&str>::new(), "{uri}");
        assert!(!tab_bar(&html).is_empty(), "{uri} has the tab bar");
        assert_eq!(chips(&html), "", "{uri} keeps its own sub-navigation");
        assert!(
            html.contains(r#"aria-label="Konto: oliver" data-here>"#),
            "{uri}"
        );
        assert!(html.contains(r#"<a href="/settings" class="m-item" aria-current="page">"#));
    }
}
