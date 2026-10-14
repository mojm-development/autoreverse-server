//! Metadata editing in the web UI at the HTTP level: the editor sheets on the book and album
//! pages, the pick buttons and selection bar on the lists, and the artist page's forms (legacy
//! library/artists/[name] actions). What the sheets do in the browser is tests/js/metadata and
//! e2e/tests/metadata.e2e.ts.

use autoreverse_server::library::artist_covers::chosen_cover;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;

use crate::artist_covers::multipart;
use crate::common::{Seed, TempLibrary, TestApp, body_text, header_str, seed, seed_track};

async fn cookie(app: &TestApp, name: &str, is_admin: bool) -> String {
    let id = app.user(name, is_admin).await;
    let token = app.token(id).await;
    format!("autoreverse_session={token}")
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> (StatusCode, String) {
    let response = app.get_with_cookie(uri, cookie).await;
    let status = response.status();
    (status, body_text(response).await)
}

async fn lock(db: &PgPool, id: i32, fields: &[&str]) {
    sqlx::query("UPDATE items SET locked_fields = $2 WHERE id = $1")
        .bind(id)
        .bind(fields.iter().map(|f| f.to_string()).collect::<Vec<_>>())
        .execute(db)
        .await
        .unwrap();
}

#[sqlx::test]
async fn a_verwalter_gets_the_editor_on_the_book_page_and_nobody_else(db: PgPool) {
    let app = TestApp::new(db);
    let admin = cookie(&app, "oliver", true).await;
    let user = cookie(&app, "gast", false).await;
    let book = seed(
        app.db(),
        Seed {
            author: Some("Tolkien"),
            series: Some("Der Herr der Ringe"),
            series_index: Some(1.0),
            ..Seed::new("book", "Die Gefährten")
        },
    )
    .await;
    lock(app.db(), book, &["author"]).await;
    let track = seed_track(app.db(), book, 1, "/b/01.mp3", 60.0).await;

    let (status, html) = page(&app, &format!("/library/books/{book}"), &admin).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("data-editor-open>"));
    assert!(html.contains("Bearbeiten</button>"));
    assert!(html.contains("<template data-metadata-editor-template>"));
    assert!(html.contains(&format!(r#"data-item-id="{book}""#)));
    // Book fields with their values; the series index as a person writes it.
    for wire in [
        "title",
        "sort_title",
        "author",
        "narrator",
        "series",
        "series_index",
        "year",
    ] {
        assert!(html.contains(&format!(r#"data-field="{wire}""#)), "{wire}");
    }
    assert!(!html.contains(r#"data-field="artist""#));
    assert!(html.contains(r#"data-field="series_index" data-type="float" data-initial="1""#));
    // Only the hand-set author carries its lock.
    assert_eq!(html.matches("data-lock>von Hand</button>").count(), 1);
    assert!(html.contains("<h3>Dateien</h3>"));
    assert!(html.contains(&format!(r#"data-track-id="{track}""#)));

    let (_, html) = page(&app, &format!("/library/books/{book}"), &user).await;
    assert!(!html.contains("data-editor-open"));
    assert!(!html.contains("data-metadata-editor-template"));
}

#[sqlx::test]
async fn the_album_editor_offers_album_fields_and_track_titles(db: PgPool) {
    let app = TestApp::new(db);
    let admin = cookie(&app, "oliver", true).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("AC/DC"),
            ..Seed::new("album", "Iron Man 2")
        },
    )
    .await;
    seed_track(app.db(), album, 1, "/a/01.mp3", 60.0).await;
    let (_, html) = page(&app, &format!("/library/albums/{album}"), &admin).await;
    assert!(html.contains("Bearbeiten</button>"));
    assert!(html.contains(r#"data-field="artist" data-type="text" data-initial="AC/DC""#));
    assert!(html.contains(r#"data-field="album_artist""#));
    assert!(!html.contains(r#"data-field="author""#));
    assert!(html.contains("<h3>Titel</h3>"));
    assert!(html.contains("Leer lassen — folgt dem Titel"));
}

#[sqlx::test]
async fn the_lists_let_a_verwalter_pick_tiles_for_the_bulk_editor(db: PgPool) {
    let app = TestApp::new(db);
    let admin = cookie(&app, "oliver", true).await;
    let user = cookie(&app, "gast", false).await;
    let book = seed(app.db(), Seed::new("book", "Das Parfum")).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("ACDC"),
            ..Seed::new("album", "Back in Black")
        },
    )
    .await;

    let (_, html) = page(&app, "/library/books", &admin).await;
    assert!(html.contains(&format!(
        r#"aria-label="Das Parfum auswählen" title="Das Parfum auswählen" data-pick="{book}""#
    )));
    assert!(html.contains(r#"data-selection-bar data-kind="book" data-total="1" hidden"#));
    assert!(html.contains("<template data-bulk-editor-template>"));
    // Books get the series assistant, without the sort title in the field list.
    assert!(html.contains("Aus Ordnernamen erkennen"));
    assert!(!html.contains(r#"<div class="field" data-field="sort_title""#));

    let (_, html) = page(&app, "/library/albums?q=back&artist=ACDC", &admin).await;
    assert!(html.contains(&format!(r#"data-pick="{album}""#)));
    // "Alle N auswählen" sends the list's own filter.
    assert!(html.contains(
        r#"data-filter="{&#34;artist&#34;:&#34;ACDC&#34;,&#34;kind&#34;:&#34;album&#34;,&#34;missing&#34;:false,&#34;q&#34;:&#34;back&#34;}""#
    ), "{}", &html[html.find("data-selection-bar").unwrap()..][..300]);
    assert!(!html.contains("Aus Ordnernamen erkennen"));

    for uri in ["/library/books", "/library/albums"] {
        let (_, html) = page(&app, uri, &user).await;
        assert!(!html.contains("data-pick="), "{uri}");
        assert!(!html.contains("data-selection-bar"), "{uri}");
    }
}

#[sqlx::test]
async fn the_infinite_scroll_brings_pick_buttons_for_a_verwalter_only(db: PgPool) {
    let app = TestApp::new(db);
    let admin = cookie(&app, "oliver", true).await;
    let user = cookie(&app, "gast", false).await;
    let album = seed(app.db(), Seed::new("album", "Nordlicht")).await;
    let (_, html) = page(&app, "/library/albums/more?offset=0", &admin).await;
    assert!(html.contains(&format!(r#"data-pick="{album}""#)));
    let (_, html) = page(&app, "/library/albums/more?offset=0", &user).await;
    assert!(html.contains("Nordlicht"));
    assert!(!html.contains("data-pick="));
}

// --- the artist page's forms ---

async fn album(db: &PgPool, artist: &str, title: &str) -> i32 {
    seed(
        db,
        Seed {
            artist: Some(artist),
            ..Seed::new("album", title)
        },
    )
    .await
}

#[sqlx::test]
async fn picking_an_album_leads_back_to_the_artists(db: PgPool) {
    let app = TestApp::new(db);
    let admin = cookie(&app, "oliver", true).await;
    let id = album(app.db(), "AC/DC", "Back in Black").await;
    let foreign = album(app.db(), "Nirvana", "Nevermind").await;

    let (_, html) = page(&app, "/library/artists/AC%2FDC", &admin).await;
    assert!(html.contains(r#"action="/library/artists/AC%2FDC/select""#));
    assert!(
        html.contains(r#"action="/library/artists/AC%2FDC/upload" enctype="multipart/form-data""#)
    );
    assert!(html.contains("Auf Zufall zurücksetzen"));

    let response = app
        .post_form(
            "/library/artists/AC%2FDC/select",
            &format!("item_id={id}"),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), "/library/artists");
    let cover = chosen_cover(app.db(), "AC/DC").await.unwrap().unwrap();
    assert_eq!(cover.item_id, Some(id));

    for (form, error) in [
        (
            format!("item_id={foreign}"),
            "Album gehört nicht zu diesem Interpreten",
        ),
        ("item_id=abc".into(), "Ungültige Album-ID"),
        (String::new(), "Ungültige Album-ID"),
    ] {
        let response = app
            .post_form("/library/artists/AC%2FDC/select", &form, Some(&admin))
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(
            html.contains(&format!(r#"<p class="error" role="alert">{error}</p>"#)),
            "{error}"
        );
        assert!(html.contains("<h1>AC/DC</h1>"));
    }

    let response = app
        .post_form("/library/artists/AC%2FDC/reset", "", Some(&admin))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        header_str(&response, header::LOCATION),
        "/library/artists/AC%2FDC"
    );
    assert!(chosen_cover(app.db(), "AC/DC").await.unwrap().is_none());
}

async fn upload(app: &TestApp, cookie: &str, body: Vec<u8>) -> axum::response::Response {
    app.send(
        Request::post("/library/artists/Kraftwerk/upload")
            .header(header::COOKIE, cookie)
            .header(header::HOST, "localhost:3000")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(
                header::CONTENT_TYPE,
                "multipart/form-data; boundary=XyZBoundary",
            )
            .body(Body::from(body))
            .unwrap(),
    )
    .await
}

#[sqlx::test]
async fn uploading_a_picture_stores_it_and_refusals_say_why(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let admin = cookie(&app, "oliver", true).await;

    let response = upload(
        &app,
        &admin,
        multipart("image", "image/png", b"\x89PNG", true),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), "/library/artists");
    let cover = chosen_cover(app.db(), "Kraftwerk").await.unwrap().unwrap();
    assert!(
        cover
            .image_path
            .unwrap()
            .starts_with(library.data().to_str().unwrap())
    );
    let (_, html) = page(&app, "/library/artists/Kraftwerk", &admin).await;
    assert!(html.contains(r#"<img src="/artists/Kraftwerk/image?v="#));

    for (body, error) in [
        (
            multipart("image", "image/png", b"", true),
            "Keine Bilddatei ausgewählt",
        ),
        (
            multipart("image", "application/pdf", b"%PDF", true),
            "Nicht unterstütztes Bildformat",
        ),
    ] {
        let response = upload(&app, &admin, body).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body_text(response).await.contains(error), "{error}");
    }
}

#[sqlx::test]
async fn the_forms_are_for_verwalter(db: PgPool) {
    let app = TestApp::new(db);
    cookie(&app, "oliver", true).await;
    let user = cookie(&app, "gast", false).await;
    let id = album(app.db(), "Kraftwerk", "Autobahn").await;
    for (uri, form) in [
        ("/library/artists/Kraftwerk/select", format!("item_id={id}")),
        ("/library/artists/Kraftwerk/reset", String::new()),
    ] {
        let response = app.post_form(uri, &form, Some(&user)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/library");
    }
    assert!(chosen_cover(app.db(), "Kraftwerk").await.unwrap().is_none());
}
