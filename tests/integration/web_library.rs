//! The library and settings pages of this slice, at the HTTP level (legacy page servers and the
//! settings / book-detail e2e specs where they assert on markup).

use autoreverse_server::library::paths::get_library_paths;
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

#[sqlx::test]
async fn library_pages_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for uri in [
        "/library/books",
        "/library/books/1",
        "/library/series",
        "/settings",
        "/settings/scan",
    ] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
}

#[sqlx::test]
async fn the_books_page_lists_books_with_progress_and_counts(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    let started = seed(
        app.db(),
        Seed {
            author: Some("Frank Schätzing"),
            series: Some("Meer"),
            series_index: Some(2.0),
            ..Seed::new("book", "Der Schwarm")
        },
    )
    .await;
    seed_track(app.db(), started, 1, "/s/1.mp3", 7200.0).await;
    seed_progress(app.db(), user, started, 3600.0, false).await;
    seed(app.db(), Seed::new("book", "Das Parfum")).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("book", "Verschwunden")
        },
    )
    .await;
    seed(app.db(), Seed::new("album", "Kein Buch")).await;

    let (status, html) = page(&app, "/library/books", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Bibliothek · Autoreverse</title>"));
    assert!(html.contains("Der Schwarm") && html.contains("Das Parfum"));
    assert!(!html.contains("Verschwunden") && !html.contains("Kein Buch"));
    assert!(html.contains(r#"<span class="count mono">2</span>"#));
    assert!(html.contains(r#"data-status="listening""#));
    assert!(html.contains(r#"style="width: 50%""#));
    assert!(html.contains("Wird gehört"));
    // The sidebar marks the section.
    assert!(html.contains(r#"href="/library/books" aria-current="true""#));

    let (_, list) = page(&app, "/library/books?view=list", &cookie).await;
    assert!(list.contains(r#"role="table" aria-label="Hörbücher""#));
    assert!(list.contains("Meer · Band 2"));
    assert!(list.contains("noch 1 Std"));
    assert!(list.contains("2 Std"));
}

#[sqlx::test]
async fn the_books_page_searches_sorts_and_filters_by_series(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    seed(
        app.db(),
        Seed {
            series: Some("Sternenwind"),
            author: Some("Erste"),
            ..Seed::new("book", "Ihr Band")
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            series: Some("Sternenwind"),
            author: Some("Zweite"),
            ..Seed::new("book", "Sein Band")
        },
    )
    .await;
    seed(app.db(), Seed::new("book", "Solo")).await;

    let (_, html) = page(&app, "/library/books?q=solo", &cookie).await;
    assert!(html.contains("Solo") && !html.contains("Ihr Band"));
    assert!(html.contains("Suche: solo"));
    let (_, html) = page(&app, "/library/books?q=nichts", &cookie).await;
    assert!(html.contains("Nichts gefunden für „nichts“."));

    let (_, html) = page(
        &app,
        "/library/books?series=Sternenwind&author=Erste",
        &cookie,
    )
    .await;
    assert!(html.contains("<title>Sternenwind · Autoreverse</title>"));
    assert!(html.contains("Ihr Band") && !html.contains("Sein Band"));
    assert!(html.contains(r#"<span class="head-author">Erste</span>"#));
    // In a series the order is the series' own: no sort toggle.
    assert!(!html.contains(r#"aria-label="Sortierung""#));

    let (_, html) = page(&app, "/library/books?sort=title&view=list", &cookie).await;
    // askama escapes `&` as `&#38;`.
    assert!(html.contains(r#"href="/library/books?sort=title&#38;view=grid""#));
    assert!(html.contains(
        r#"href="/library/books?sort=series&#38;view=list" aria-current="false">Serie &#38; Folge"#
    ));
}

#[sqlx::test]
async fn an_empty_library_says_so(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    let (_, html) = page(&app, "/library/books", &cookie).await;
    assert!(html.contains("Noch keine Hörbücher in der Bibliothek."));
}

#[sqlx::test]
async fn the_book_page_shows_meta_chapters_and_series(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    let title = "Ein Leben ist zu wenig - Die Autobiographie (Gekürzt)";
    let book = seed(
        app.db(),
        Seed {
            author: Some("Lew Kopelew"),
            narrator: Some("Jemand"),
            series: Some("Leben"),
            ..Seed::new("book", title)
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            series: Some("Leben"),
            ..Seed::new("book", "Band zwei")
        },
    )
    .await;
    seed_track(app.db(), book, 1, "/b/1.m4b", 5400.0).await;
    for (i, start) in [0.0, 1800.0, 3600.0].iter().enumerate() {
        sqlx::query("INSERT INTO chapters (item_id, position, title, start, \"end\") VALUES ($1, $2, $3, $4, $5)")
            .bind(book)
            .bind(i as i32 + 1)
            .bind(format!("Ein Leben ist zu wenig - Die Autobiographie, Kapitel {}", i + 1))
            .bind(start)
            .bind(start + 1800.0)
            .execute(app.db())
            .await
            .unwrap();
    }
    seed_progress(app.db(), user, book, 2000.0, false).await;

    let (status, html) = page(&app, &format!("/library/books/{book}"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(&format!("<h1>{title}</h1>")));
    assert!(html.contains("Lew Kopelew · gelesen von Jemand · 3 Kapitel · 1.5 Std"));
    assert!(html.contains("Serie · 2 Bände"));
    assert!(html.contains(r#"role="table" aria-label="Kapitel""#));
    // The repeated book title is dropped from the labels.
    assert!(html.contains(r#"<span class="title">Kapitel 2</span>"#));
    assert!(html.contains(r#"<span class="cell state" data-chapter-state>gehört</span>"#));
    assert!(html.contains("0:30:00"));
    assert!(html.contains("Band zwei"));
    assert!(html.contains("37 %"));
}

#[sqlx::test]
async fn the_book_page_404s_for_anything_but_a_book(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _) = cookie(&app, "oliver", false).await;
    let album = seed(app.db(), Seed::new("album", "Nordlicht")).await;
    for uri in [
        format!("/library/books/{album}"),
        "/library/books/999".into(),
        "/library/books/x".into(),
    ] {
        let (status, html) = page(&app, &uri, &cookie).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert!(html.contains("Unbekanntes Hörbuch"));
    }
}

#[sqlx::test]
async fn the_series_page_groups_shelves_by_author(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, user) = cookie(&app, "oliver", false).await;
    let first = seed(
        app.db(),
        Seed {
            author: Some("Andreas Eschbach"),
            series: Some("Perry Rhodan"),
            series_index: Some(1.0),
            cover_path: Some("/covers/1.jpg"),
            ..Seed::new("book", "Band 1")
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            author: Some("Andreas Eschbach"),
            series: Some("Perry Rhodan"),
            ..Seed::new("book", "Band 2")
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            series: Some("Anonym"),
            ..Seed::new("book", "A")
        },
    )
    .await;
    seed_progress(app.db(), user, first, 1.0, true).await;

    let (status, html) = page(&app, "/library/series", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Andreas Eschbach") && html.contains("Ohne Autor"));
    assert!(html.contains("2 Bände · 1 beendet"));
    assert!(html.contains("1 Band"));
    assert!(html.contains(&format!("url(/items/{first}/cover)")));
    assert!(
        html.contains(r#"href="/library/books?series=Perry+Rhodan&#38;author=Andreas+Eschbach""#)
    );
    assert!(!html.contains("Keines deiner Hörbücher"));
}

#[sqlx::test]
async fn the_settings_overview_lists_what_the_user_may_open(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "admin", true).await;
    let (user, _) = cookie(&app, "oliver", false).await;

    let (_, html) = page(&app, "/settings", &admin).await;
    let overview = &html[html.find(r#"data-testid="settings-overview""#).unwrap()..];
    assert_eq!(overview.matches(r#"class="card""#).count(), 7);
    assert!(html.contains("<h1>Einstellungen</h1>"));

    let (_, html) = page(&app, "/settings", &user).await;
    let overview = &html[html.find(r#"data-testid="settings-overview""#).unwrap()..];
    assert!(overview.contains("Wiedergabe"));
    assert!(!overview.contains("Bibliotheken"));
}

#[sqlx::test]
async fn admin_sections_send_a_non_admin_back_to_the_library(db: PgPool) {
    let app = TestApp::new(db);
    app.user("admin", true).await;
    let (user, _) = cookie(&app, "oliver", false).await;
    for uri in [
        "/settings/libraries",
        "/settings/scan",
        "/settings/scan/card",
    ] {
        let response = app.get_with_cookie(uri, &user).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/library", "{uri}");
    }
}

#[sqlx::test]
async fn the_libraries_page_saves_both_paths(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "admin", true).await;
    let (status, html) = page(&app, "/settings/libraries", &admin).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"name="booksDir" value="""#));
    assert!(html.contains("Kein Scan aktiv"));

    let response = app
        .post_form(
            "/settings/libraries?/savePaths",
            "booksDir=%2Fmedia%2Fb%C3%BCcher%2F&musicDir=",
            Some(&admin),
        )
        .await;
    let html = body_text(response).await;
    assert!(html.contains("Beide Pfade werden benötigt."));
    assert_eq!(get_library_paths(app.db()).await.unwrap().books_dir, None);

    let response = app
        .post_form(
            "/settings/libraries?/savePaths",
            "booksDir=%2Fmedia%2Fb%C3%BCcher%2F&musicDir=%2Fmedia%2Fmusik",
            Some(&admin),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains(r#"name="booksDir" value="/media/bücher""#));
    let paths = get_library_paths(app.db()).await.unwrap();
    assert_eq!(paths.books_dir.as_deref(), Some("/media/bücher"));
    assert_eq!(paths.music_dir.as_deref(), Some("/media/musik"));
}

#[sqlx::test]
async fn the_libraries_page_offers_to_clean_up_missing_items(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "admin", true).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    seed(app.db(), Seed::new("album", "Da")).await;
    let (_, html) = page(&app, "/settings/libraries", &admin).await;
    assert!(html.contains("1 Titel fehlen"));
    let response = app
        .post_form("/settings/libraries?/cleanupMissing", "", Some(&admin))
        .await;
    let html = body_text(response).await;
    assert!(!html.contains("Titel fehlen"));
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(left, 1);
}

#[sqlx::test]
async fn the_scan_card_polls_and_starts_a_scan(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = cookie(&app, "admin", true).await;
    let (status, html) = page(&app, "/settings/scan", &admin).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<h1>Scan</h1>"));
    assert!(html.contains(r#"hx-get="/settings/scan/card""#));
    assert!(html.contains(r#"hx-trigger="every 5s""#));
    assert!(html.contains("Scan starten"));

    let response = app
        .post_form("/settings/scan/start", "", Some(&admin))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let card = body_text(response).await;
    assert!(card.starts_with("<div") && card.contains(r#"id="scan-card""#));
    app.wait_for_scan().await;

    // No paths configured: the card's log says so.
    let (_, card) = page(&app, "/settings/scan/card", &admin).await;
    assert!(card.contains("Fehler: Bibliothekspfade sind noch nicht konfiguriert"));
    assert!(card.contains("Übersprungen: —"));
}
