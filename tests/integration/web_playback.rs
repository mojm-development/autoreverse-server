//! The playback side of the web UI at the HTTP level: start page, fullscreen player, the play
//! buttons on the detail pages and tiles, the persistent player in the layout and Settings →
//! Wiedergabe (legacy library/+page.server.ts, (fullscreen)/library/[id]/player, settings/playback
//! and the start / now-playing / album-detail e2e specs where they assert on markup).

use axum::http::{StatusCode, header};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_text, header_str, seed, seed_progress, seed_track};

async fn cookie(app: &TestApp, name: &str, is_admin: bool) -> (String, i32) {
    let id = app.user(name, is_admin).await;
    let token = app.token(id).await;
    (format!("autoreverse_session={token}"), id)
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> (StatusCode, String) {
    let response = app.get_with_cookie(uri, cookie).await;
    let status = response.status();
    (status, body_text(response).await)
}

async fn chapter(app: &TestApp, item: i32, position: i32, title: &str, start: f64, end: f64) {
    sqlx::query(
        "INSERT INTO chapters (item_id, position, title, start, \"end\") VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(item)
    .bind(position)
    .bind(title)
    .bind(start)
    .bind(end)
    .execute(app.db())
    .await
    .unwrap();
}

#[sqlx::test]
async fn playback_pages_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for uri in ["/library", "/library/1/player", "/settings/playback"] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
}

#[sqlx::test]
async fn the_start_page_shows_continue_listening_and_the_newest_items(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    let book = seed(
        app.db(),
        Seed {
            author: Some("Frank Schätzing"),
            cover_path: Some("/covers/schwarm.jpg"),
            ..Seed::new("book", "Der Schwarm")
        },
    )
    .await;
    seed_track(app.db(), book, 1, "/s/1.mp3", 5400.0).await;
    seed_progress(app.db(), user, book, 1350.0, false).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Kraftwerk"),
            ..Seed::new("album", "Autobahn")
        },
    )
    .await;
    seed_track(app.db(), album, 1, "/m/1.mp3", 1500.0).await;
    seed_progress(app.db(), user, album, 10.0, false).await;
    let finished = seed(app.db(), Seed::new("book", "Ausgehört")).await;
    seed_progress(app.db(), user, finished, 10.0, true).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("book", "Verschwunden")
        },
    )
    .await;

    let (status, html) = page(&app, "/library", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Start · Autoreverse</title>"));
    assert!(html.contains(", Oliver</h1>"));
    assert!(html.contains("<h2>Weiter hören</h2>"));
    assert!(html.contains("2 offen"));
    assert!(html.contains(&format!(
        r#"<a href="/library/books/{book}" class="continue-card">"#
    )));
    assert!(html.contains(&format!(r#"src="/items/{book}/cover""#)));
    assert!(html.contains("Frank Schätzing"));
    assert!(html.contains(r#"style="width: 25.000%""#));
    assert!(html.contains("1.5 h"));
    assert!(html.contains(&format!(
        r#"<a href="/library/albums/{album}" class="continue-card">"#
    )));
    assert!(html.contains("25 min"));
    // Finished items are not "open".
    assert!(!html.contains(&format!(
        r#"<a href="/library/books/{finished}" class="continue-card">"#
    )));

    assert!(html.contains("<h2>Neu im Bestand</h2>"));
    assert!(html.contains(r#"<a href="/library/albums?sort=added">Alle ansehen</a>"#));
    assert!(html.contains("Ausgehört"));
    assert!(!html.contains("Verschwunden"));
}

#[sqlx::test]
async fn the_start_page_lists_at_most_three_unfinished_and_six_new(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    for i in 0..8 {
        let id = seed(app.db(), Seed::new("book", &format!("Buch {i}"))).await;
        seed_progress(app.db(), user, id, 1.0, false).await;
    }
    let (_, html) = page(&app, "/library", &cookie).await;
    assert!(html.contains("3 offen"));
    assert_eq!(html.matches(r#"class="continue-card""#).count(), 3);
    assert_eq!(html.matches(r#"<figure class="cover-tile">"#).count(), 6);
    // Newest first.
    assert!(html.contains("Buch 7") && !html.contains("Buch 1<"));
}

#[sqlx::test]
async fn every_app_page_carries_the_persistent_player(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    sqlx::query("UPDATE users SET playback_speed = 1.25, skip_back = 20 WHERE id = $1")
        .bind(user)
        .execute(app.db())
        .await
        .unwrap();
    for uri in ["/library", "/library/books", "/library/albums", "/settings"] {
        let (status, html) = page(&app, uri, &cookie).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(
            html.contains(r#"<div class="boost-root" hx-boost="true">"#),
            "{uri}"
        );
        assert!(html.contains(r#"id="page""#), "{uri}");
        assert!(html.contains(r#"data-speed="1.25""#), "{uri}");
        assert!(html.contains(r#"data-skip-back="20""#), "{uri}");
        assert!(html.contains(r#"data-skip-forward="15""#), "{uri}");
        assert!(
            html.contains(r#"data-testid="mini-player" data-mini-player hidden"#),
            "{uri}"
        );
        assert!(
            html.contains("<audio hidden data-player-audio></audio>"),
            "{uri}"
        );
        assert!(
            html.contains(r#"<script type="module" src="/js/player/main.js"></script>"#),
            "{uri}"
        );
    }
}

#[sqlx::test]
async fn the_player_assets_are_served(db: PgPool) {
    let app = TestApp::new(db);
    for (uri, kind) in [
        ("/js/player/main.js", "javascript"),
        ("/js/player/store.js", "javascript"),
        ("/css/player.css", "css"),
        ("/css/pages/now-playing.css", "css"),
    ] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert!(
            header_str(&response, header::CONTENT_TYPE).contains(kind),
            "{uri}"
        );
    }
}

#[sqlx::test]
async fn the_fullscreen_player_shows_a_book_with_its_chapters(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    let book = seed(
        app.db(),
        Seed {
            author: Some("Lew Kopelew"),
            narrator: Some("Jemand"),
            cover_path: Some("/covers/b.jpg"),
            ..Seed::new("book", "Aufbewahren für alle Zeit")
        },
    )
    .await;
    seed_track(app.db(), book, 1, "/b/1.mp3", 3600.0).await;
    chapter(
        &app,
        book,
        0,
        "Aufbewahren für alle Zeit - Kapitel 1",
        0.0,
        1800.0,
    )
    .await;
    chapter(
        &app,
        book,
        1,
        "Aufbewahren für alle Zeit - Kapitel 2",
        1800.0,
        3600.0,
    )
    .await;
    sqlx::query(
        "INSERT INTO bookmarks (user_id, item_id, position, title) VALUES ($1, $2, 90, 'Hier')",
    )
    .bind(user)
    .bind(book)
    .execute(app.db())
    .await
    .unwrap();

    let (status, html) = page(&app, &format!("/library/{book}/player"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Aufbewahren für alle Zeit · Autoreverse</title>"));
    // No sidebar on this page; the mini player is there but hidden.
    assert!(!html.contains(r#"<nav class="sidebar""#));
    assert!(html.contains(r#"data-mini-player hidden"#));
    assert!(html.contains(&format!(
        r#"data-np data-item-id="{book}" data-kind="book""#
    )));
    assert!(html.contains("Läuft gerade · Hörbuch"));
    assert!(html.contains("<h1 data-np-headline>Kapitel 1</h1>"));
    assert!(html.contains("Kapitel 1 von 2 · Aufbewahren für alle Zeit"));
    assert!(html.contains("Lew Kopelew · Jemand"));
    assert!(html.contains(r#"aria-label="Position im Hörbuch""#));
    assert!(html.contains(r#"<span class="tick" style="left: 50.000%"></span>"#));
    assert!(html.contains(r#"aria-label="30 Sekunden zurück""#));
    assert!(html.contains(r#"aria-label="15 Sekunden vor""#));
    assert!(html.contains(r#"aria-label="Vorheriges Kapitel""#));
    assert!(html.contains(r#"<option value="1" selected>1,00×</option>"#));
    assert!(html.contains(r#"<option value="1.25">1,25×</option>"#));
    assert!(html.contains("Shift+←/→ Kapitel · Esc zurück"));
    assert!(html.contains(r#"data-action="play-from" data-position="1800""#));
    assert!(html.contains(r#"aria-label="Aufbewahren für alle Zeit - Kapitel 2 abspielen""#));
    assert!(html.contains(r#"Lesezeichen · <span data-refresh="bookmark-count">1</span>"#));
    assert!(html.contains(r#"data-action="play-from" data-position="90""#));
    assert!(html.contains("Zuletzt gesetzt"));
    assert!(html.contains(&format!(r#"src="/items/{book}/cover""#)));
}

#[sqlx::test]
async fn the_fullscreen_player_lists_an_albums_tracks_and_steps_ten_seconds(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Kraftwerk"),
            ..Seed::new("album", "Autobahn")
        },
    )
    .await;
    let first = seed_track(app.db(), album, 1, "/m/1.mp3", 1350.0).await;
    seed_track(app.db(), album, 2, "/m/2.mp3", 20.0).await;

    let (status, html) = page(&app, &format!("/library/{album}/player"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Läuft gerade · Album"));
    assert!(html.contains("<h1 data-np-headline>Autobahn</h1>"));
    assert!(html.contains(r#"aria-label="Position im Titel""#));
    assert!(html.contains(r#"aria-label="10 Sekunden zurück""#));
    assert!(html.contains(r#"aria-label="Nächster Titel""#));
    assert!(html.contains(r#"<button class="tab active" data-tab="chapters">Titel</button>"#));
    assert!(html.contains(&format!(r#"data-track-row data-track-id="{first}""#)));
    assert!(html.contains("23 min"));
    // A track under a minute still reads as one.
    assert!(html.contains(">1 min<"));
    assert!(html.contains(r#"aria-label="Titel 1 abspielen""#));
}

#[sqlx::test]
async fn the_fullscreen_player_404s_for_an_unknown_item(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    for uri in ["/library/999/player", "/library/abc/player"] {
        let (status, html) = page(&app, uri, &cookie).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert!(html.contains("Unbekanntes Item"), "{uri}");
    }
}

#[sqlx::test]
async fn the_book_page_offers_play_and_resume(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    seed_track(app.db(), book, 1, "/s/1.mp3", 7200.0).await;
    chapter(&app, book, 0, "Eins", 0.0, 7200.0).await;

    let (_, html) = page(&app, &format!("/library/books/{book}"), &cookie).await;
    assert!(html.contains(&format!(
        r#"data-live-item data-item-id="{book}" data-kind="book" data-total="7200""#
    )));
    assert!(html.contains(r#"data-action="play"><svg"#));
    assert!(html.contains("</svg> Abspielen</button>"));
    assert!(html.contains("</svg> Von vorn</button>"));
    assert!(html.contains(r#"data-chapter data-start="0" data-end="7200""#));

    seed_progress(app.db(), user, book, 2000.0, false).await;
    let (_, html) = page(&app, &format!("/library/books/{book}"), &cookie).await;
    assert!(html.contains("</svg> Weiter ab 0:33:20</button>"));
}

#[sqlx::test]
async fn the_album_page_offers_play_shuffle_and_every_track(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    let album = seed(app.db(), Seed::new("album", "Autobahn")).await;
    let track = seed_track(app.db(), album, 1, "/m/1.mp3", 1350.0).await;

    let (_, html) = page(&app, &format!("/library/albums/{album}"), &cookie).await;
    assert!(html.contains(&format!(
        r#"data-live-item data-item-id="{album}" data-kind="album""#
    )));
    assert!(html.contains("</svg> Abspielen</button>"));
    assert!(html.contains("</svg> Zufall</button>"));
    assert!(html.contains(&format!(
        r#"data-action="play-track" data-track-id="{track}""#
    )));
    assert!(html.contains(r#"aria-current="false" data-track-row"#));
}

#[sqlx::test]
async fn tiles_carry_a_play_button(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    let album = seed(app.db(), Seed::new("album", "Autobahn")).await;
    // Labelled with the title like legacy's CoverTile `playLabel`.
    let (_, html) = page(&app, "/library/books", &cookie).await;
    assert!(html.contains(&format!(
        r#"aria-label="Der Schwarm abspielen" title="Der Schwarm abspielen" data-action="play" data-item-id="{book}""#
    )));
    let (_, html) = page(&app, "/library/albums", &cookie).await;
    assert!(html.contains(&format!(
        r#"aria-label="Autobahn abspielen" title="Autobahn abspielen" data-action="play" data-item-id="{album}""#
    )));
    // The list view's row number turns into a play button, as in legacy.
    let (_, html) = page(&app, "/library/albums?view=list", &cookie).await;
    assert!(html.contains(&format!(
        r#"class="row-play" aria-label="Autobahn abspielen" data-action="play" data-item-id="{album}""#
    )));
}

#[sqlx::test]
async fn errors_under_library_and_settings_keep_the_app_shell(db: PgPool) {
    // legacy library/+error.svelte and settings/+error.svelte rendered inside their layouts.
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    let (status, html) = page(&app, "/library/books/999999", &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains(r#"class="error-page in-shell""#));
    assert!(html.contains(r#"<nav class="sidebar""#));
    assert!(html.contains("<title>Fehler 404 · Autoreverse</title>"));
    // Logged out there is no shell to show: the standalone page, as before.
    let response = app.get("/library/books/999999", None).await;
    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "logged out redirects to /login"
    );
}

#[sqlx::test]
async fn settings_playback_shows_and_stores_the_preferences(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;

    let (status, html) = page(&app, "/settings", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"<a class="card" href="/settings/playback">"#));

    let (status, html) = page(&app, "/settings/playback", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<h1>Wiedergabe</h1>"));
    assert!(html.contains(
        r#"<input type="range" name="playbackSpeed" min="0.5" max="4" step="0.05" value="1" />"#
    ));
    assert!(html.contains(r#"name="skipBack" min="1" max="300" value="30""#));
    assert!(html.contains(r#"name="skipForward" min="1" max="300" value="15""#));
    assert!(!html.contains("Gespeichert"));

    let response = app
        .post_form(
            "/settings/playback",
            "playbackSpeed=1.5&skipBack=45.7&skipForward=999",
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("Gespeichert"));
    assert!(html.contains(r#"value="1.5""#));
    assert!(html.contains(r#"name="skipBack" min="1" max="300" value="45""#));
    assert!(html.contains(r#"name="skipForward" min="1" max="300" value="300""#));
    // The player reads the new values from the page.
    assert!(html.contains(r#"data-speed="1.5""#));
    let stored: (f64, i32, i32) =
        sqlx::query_as("SELECT playback_speed, skip_back, skip_forward FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(stored, (1.5, 45, 300));

    // Out of range is clamped, not refused; an empty field reads as 0.
    let html = body_text(
        app.post_form(
            "/settings/playback",
            "playbackSpeed=9&skipBack=&skipForward=10",
            Some(&cookie),
        )
        .await,
    )
    .await;
    assert!(html.contains(r#"value="4""#));
    assert!(html.contains(r#"name="skipBack" min="1" max="300" value="1""#));

    let html = body_text(
        app.post_form(
            "/settings/playback",
            "playbackSpeed=schnell&skipBack=10&skipForward=10",
            Some(&cookie),
        )
        .await,
    )
    .await;
    assert!(html.contains(r#"<p class="error" role="alert">Bitte nur Zahlen eingeben.</p>"#));
    assert!(!html.contains("Gespeichert"));
}
