//! Favourites, bookmarks and search in the web UI at the HTTP level (legacy library/favorites,
//! library/bookmarks, library/search, the hearts on the album page, "Lesezeichen setzen", and the
//! secondary-pages/search e2e specs where they assert on markup).

use axum::http::{StatusCode, header};
use sqlx::PgPool;

use autoreverse_server::library::bookmarks::add_bookmark;
use autoreverse_server::library::favorites::{add_item_favorite, add_track_favorite};

use crate::common::{Seed, TestApp, body_text, header_str, seed, seed_progress, seed_track};

async fn cookie(app: &TestApp, name: &str) -> (String, i32) {
    let id = app.user(name, false).await;
    let token = app.token(id).await;
    (format!("autoreverse_session={token}"), id)
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> String {
    let response = app.get_with_cookie(uri, cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

async fn title_track(app: &TestApp, id: i32, title: &str) {
    sqlx::query("UPDATE tracks SET title = $2 WHERE id = $1")
        .bind(id)
        .bind(title)
        .execute(app.db())
        .await
        .unwrap();
}

#[sqlx::test]
async fn the_pages_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for uri in [
        "/library/favorites",
        "/library/bookmarks",
        "/library/search",
    ] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
}

#[sqlx::test]
async fn every_sidebar_link_resolves_to_a_real_page(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver").await;
    let html = page(&app, "/library", &cookie).await;
    for href in [
        "/library/favorites",
        "/library/bookmarks",
        "/library/search",
    ] {
        assert!(html.contains(&format!("href=\"{href}\"")), "{href}");
        page(&app, href, &cookie).await;
    }
}

#[sqlx::test]
async fn favorites_shows_tiles_and_songs_or_the_empty_text(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver").await;
    let html = page(&app, "/library/favorites", &cookie).await;
    assert!(html.contains("Noch keine Favoriten."));
    assert!(html.contains("<title>Favoriten · Autoreverse</title>"));

    let album = seed(
        app.db(),
        Seed {
            artist: Some("Kraftwerk"),
            cover_path: Some("/c.jpg"),
            ..Seed::new("album", "Trans Europa Express")
        },
    )
    .await;
    let book = seed(
        app.db(),
        Seed {
            author: Some("Frank Schätzing"),
            ..Seed::new("book", "Der Schwarm")
        },
    )
    .await;
    let track = seed_track(app.db(), album, 1, "/k/1.mp3", 120.0).await;
    title_track(&app, track, "Europa Endlos").await;
    add_item_favorite(app.db(), user, album).await.unwrap();
    add_item_favorite(app.db(), user, book).await.unwrap();
    add_track_favorite(app.db(), user, track).await.unwrap();

    let html = page(&app, "/library/favorites", &cookie).await;
    assert!(!html.contains("Noch keine Favoriten."));
    assert!(html.contains("Titel &amp; Alben"));
    assert!(html.contains(&format!("href=\"/library/albums/{album}\"")));
    assert!(html.contains(&format!("src=\"/items/{album}/cover\"")));
    assert!(html.contains(&format!("href=\"/library/books/{book}\"")));
    assert!(html.contains("--accent: var(--book)"));
    assert!(html.contains("Frank Schätzing"));
    assert!(html.contains("aria-label=\"Songs\""));
    assert!(html.contains("Europa Endlos"));
}

#[sqlx::test]
async fn bookmarks_lists_every_bookmark_with_its_item(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver").await;
    let html = page(&app, "/library/bookmarks", &cookie).await;
    assert!(html.contains("Noch keine Lesezeichen."));

    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    add_bookmark(app.db(), user, book, 3723.0, "Tiefsee")
        .await
        .unwrap();
    let html = page(&app, "/library/bookmarks", &cookie).await;
    assert!(!html.contains("Noch keine Lesezeichen."));
    assert!(html.contains("Lesezeichen <span class=\"count mono\">1</span>"));
    assert!(html.contains(&format!("href=\"/library/books/{book}\"")));
    assert!(html.contains("Der Schwarm"));
    assert!(html.contains("Tiefsee"));
    assert!(html.contains("1:02:03"));
}

#[sqlx::test]
async fn search_without_a_query_shows_only_the_field(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver").await;
    let html = page(&app, "/library/search", &cookie).await;
    assert!(html.contains("type=\"search\" name=\"q\""));
    assert!(html.contains("<title>Suche · Autoreverse</title>"));
    assert!(!html.contains("Treffer in"));
}

#[sqlx::test]
async fn search_groups_hits_with_progress_and_plays_tracks(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver").await;
    let book = seed(
        app.db(),
        Seed {
            author: Some("Andreas Eschbach"),
            series: Some("Jesus"),
            ..Seed::new("book", "Das Jesus-Video")
        },
    )
    .await;
    seed_track(app.db(), book, 1, "/j/1.mp3", 1000.0).await;
    seed_progress(app.db(), user, book, 420.0, false).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Eschbach Ensemble"),
            ..Seed::new("album", "Live")
        },
    )
    .await;
    let track = seed_track(app.db(), album, 1, "/l/1.mp3", 300.0).await;
    title_track(&app, track, "Eschbach-Suite").await;

    let html = page(&app, "/library/search?q=eschbach", &cookie).await;
    assert!(html.contains("<title>Suche: eschbach · Autoreverse</title>"));
    assert!(html.contains("value=\"eschbach\""));
    // Book, album and track; the author and the artist are names, not hits.
    assert!(html.contains("3 Treffer in 2 Items"), "{html}");
    assert!(html.contains("Hörbücher · 1"));
    assert!(html.contains("Alben · 1"));
    assert!(html.contains("Titel · 1"));
    assert!(html.contains(&format!("href=\"/library/books/{book}\"")));
    assert!(html.contains("Andreas Eschbach · Jesus"));
    assert!(html.contains("42 % gehört"));
    assert!(html.contains(&format!("href=\"/library/albums/{album}\"")));
    assert!(html.contains(&format!(
        "data-item-id=\"{album}\">\n\t\t\t<button type=\"button\" class=\"activator bare\" \
         aria-label=\"Eschbach-Suite abspielen\" data-action=\"play-track\" \
         data-track-id=\"{track}\""
    )));
    assert!(html.contains("5 min"));
    assert!(html.contains("Interpreten &amp; Autoren"));
    assert!(html.contains("href=\"/library/albums?artist=Eschbach%20Ensemble\""));
    assert!(html.contains("href=\"/library/books?q=Andreas%20Eschbach\""));
    assert!(html.contains("Autor:in"));
}

#[sqlx::test]
async fn the_album_page_has_hearts_for_the_album_and_each_track(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver").await;
    let album = seed(app.db(), Seed::new("album", "Radioaktivität")).await;
    let one = seed_track(app.db(), album, 1, "/r/1.mp3", 60.0).await;
    let two = seed_track(app.db(), album, 2, "/r/2.mp3", 60.0).await;
    let uri = format!("/library/albums/{album}");

    let html = page(&app, &uri, &cookie).await;
    assert!(html.contains(&format!(
        "aria-label=\"Zu Favoriten hinzufügen\" title=\"Zu Favoriten hinzufügen\" \
         aria-pressed=\"false\" data-favorite=\"items\" data-favorite-id=\"{album}\""
    )));
    assert_eq!(html.matches("aria-label=\"Titel favorisieren\"").count(), 2);

    add_item_favorite(app.db(), user, album).await.unwrap();
    add_track_favorite(app.db(), user, two).await.unwrap();
    let html = page(&app, &uri, &cookie).await;
    assert!(html.contains(&format!(
        "aria-label=\"Aus Favoriten entfernen\" title=\"Aus Favoriten entfernen\" \
         aria-pressed=\"true\" data-favorite=\"items\" data-favorite-id=\"{album}\""
    )));
    assert!(html.contains(&format!(
        "aria-pressed=\"false\" data-favorite=\"tracks\" data-favorite-id=\"{one}\""
    )));
    assert!(html.contains(&format!(
        "aria-pressed=\"true\" data-favorite=\"tracks\" data-favorite-id=\"{two}\""
    )));
    assert!(html.contains("data-testid=\"icon-heart-filled\""));
}

#[sqlx::test]
async fn the_book_page_sets_bookmarks_at_the_saved_position_and_has_a_heart(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver").await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    seed_track(app.db(), book, 1, "/s/1.mp3", 3600.0).await;
    seed_progress(app.db(), user, book, 1234.5, false).await;
    add_bookmark(app.db(), user, book, 60.0, "0:01:00")
        .await
        .unwrap();

    let html = page(&app, &format!("/library/books/{book}"), &cookie).await;
    assert!(html.contains("data-action=\"bookmark\" data-position=\"1234.5\">"));
    assert!(html.contains("<span data-bookmark-label>Lesezeichen setzen</span>"));
    assert!(html.contains("Lesezeichen · <span data-refresh=\"bookmark-count\">1</span>"));
    assert!(html.contains("data-refresh=\"bookmark-list\""));
    assert!(html.contains(&format!(
        "data-favorite=\"items\" data-favorite-id=\"{book}\""
    )));
}

#[sqlx::test]
async fn the_fullscreen_player_can_set_a_bookmark(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver").await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    seed_track(app.db(), book, 1, "/s/1.mp3", 3600.0).await;

    let html = page(&app, &format!("/library/{book}/player"), &cookie).await;
    assert!(html.contains("data-action=\"bookmark\""));
    assert!(html.contains("<div data-refresh=\"bookmark-card\">"));
    assert!(!html.contains("Zuletzt gesetzt"));

    add_bookmark(app.db(), user, book, 60.0, "Tiefsee")
        .await
        .unwrap();
    let html = page(&app, &format!("/library/{book}/player"), &cookie).await;
    assert!(html.contains("Zuletzt gesetzt"));
    assert!(html.contains("Lesezeichen · <span data-refresh=\"bookmark-count\">1</span>"));
}
