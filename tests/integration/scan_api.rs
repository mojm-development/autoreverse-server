//! `/scan`, `/scan/status`, `/scan/cancel` and whole scans of real folders (legacy
//! scan-api.test.ts plus the scan parts of the e2e smoke spec).

use std::path::{Path, PathBuf};

use autoreverse_server::library::paths::set_library_paths;
use autoreverse_server::scanner::run::{PATHS_MISSING, run_scan};
use axum::http::StatusCode;
use serde_json::Value;
use sqlx::PgPool;

use crate::common::{Seed, TempLibrary, TestApp, body_json, make_audio, make_audio_with, seed};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library")
}

async fn admin(app: &TestApp) -> String {
    let id = app.user("admin", true).await;
    app.token(id).await
}

async fn configure(app: &TestApp, books: &Path, music: &Path) {
    set_library_paths(app.db(), books.to_str().unwrap(), music.to_str().unwrap())
        .await
        .unwrap();
}

async fn scan(app: &TestApp, token: &str) -> autoreverse_server::scanner::state::ScanState {
    let response = app.post_raw("/scan", "", Some(token)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    app.wait_for_scan().await
}

async fn book_titles(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar("SELECT title FROM items WHERE kind = 'book' ORDER BY title")
        .fetch_all(app.db())
        .await
        .unwrap()
}

#[sqlx::test]
async fn scan_routes_are_for_admins_only(db: PgPool) {
    let app = TestApp::new(db);
    app.user("admin", true).await;
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    for (method, uri) in [
        ("GET", "/scan/status"),
        ("POST", "/scan"),
        ("POST", "/scan/cancel"),
    ] {
        let response = if method == "GET" {
            app.get(uri, Some(&token)).await
        } else {
            app.post_raw(uri, "", Some(&token)).await
        };
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(body_json(response).await["detail"], "Nur für Verwalter");
        let anonymous = app.get(uri, None).await;
        assert!(matches!(
            anonymous.status(),
            StatusCode::UNAUTHORIZED | StatusCode::METHOD_NOT_ALLOWED
        ));
    }
    assert_eq!(
        app.get("/scan/status", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn post_scan_409s_while_a_scan_is_running(db: PgPool) {
    let app = TestApp::new(db);
    let token = admin(&app).await;
    assert!(app.state.scan.try_start());
    let response = app.post_raw("/scan", "", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["detail"],
        "Es läuft bereits ein Scan"
    );
}

#[sqlx::test]
async fn cancel_409s_when_nothing_runs_and_202s_when_something_does(db: PgPool) {
    let app = TestApp::new(db);
    let token = admin(&app).await;
    let response = app.post_raw("/scan/cancel", "", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["detail"],
        "Es läuft gerade kein Scan"
    );

    assert!(app.state.scan.try_start());
    let response = app.post_raw("/scan/cancel", "", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(body_json(response).await["cancel_requested"], true);
}

#[sqlx::test]
async fn status_has_the_wire_shape(db: PgPool) {
    let app = TestApp::new(db);
    let token = admin(&app).await;
    let body = body_json(app.get("/scan/status", Some(&token)).await).await;
    let mut keys: Vec<_> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "cancel_requested",
            "cancelled",
            "finished_at",
            "last_error",
            "last_report",
            "last_skipped",
            "progress",
            "running",
            "started_at"
        ]
    );
    assert_eq!(body["running"], false);
    assert_eq!(body["last_skipped"], Value::Array(vec![]));
}

#[sqlx::test]
async fn without_library_paths_the_scan_only_reports_that(db: PgPool) {
    let app = TestApp::new(db);
    let token = admin(&app).await;
    let first = scan(&app, &token).await;
    assert_eq!(first.last_error.as_deref(), Some(PATHS_MISSING));
    assert!(!first.running);
    let finished = first.finished_at.clone().unwrap();
    // Milliseconds stay, so two scans in the same second still differ.
    assert!(
        finished.contains('.') && finished.ends_with('Z'),
        "{finished}"
    );
    let second = scan(&app, &token).await;
    assert_ne!(second.finished_at, first.finished_at);
}

#[sqlx::test]
async fn scans_the_fixture_library_and_serves_the_book(db: PgPool) {
    let data = tempfile::tempdir().unwrap();
    let app = TestApp::with_data_dir(db, data.path());
    let token = admin(&app).await;
    configure(&app, &fixtures().join("books"), &fixtures().join("music")).await;

    let response = app.post_raw("/scan", "", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(body_json(response).await["running"], true);
    let status = app.wait_for_scan().await;
    assert_eq!(status.last_error, None);
    let report = status.last_report.unwrap();
    // One book from the books root, one album from the music root.
    assert_eq!(
        (report.new, report.updated, report.removed, report.skipped),
        (2, 0, 0, 0)
    );
    assert!(status.started_at.is_some() && status.progress.is_none());

    let items = body_json(app.get("/items?kind=book", Some(&token)).await).await;
    let book = &items["items"][0];
    assert_eq!(book["title"], "Buch");
    assert_eq!(book["author"], "Autor");
    let detail = body_json(
        app.get(&format!("/items/{}", book["id"]), Some(&token))
            .await,
    )
    .await;
    assert_eq!(detail["tracks"].as_array().unwrap().len(), 1);
    assert_eq!(detail["tracks"][0]["title"], "Kapitel 1");
    assert_eq!(detail["chapters"][0]["title"], "Kapitel 1");
    assert!(detail["tracks"][0]["duration"].as_f64().unwrap() > 1.0);

    // The music pass: folder artist, album title from the tag, the track with its title.
    let albums = body_json(app.get("/items?kind=album", Some(&token)).await).await;
    let album = &albums["items"][0];
    assert_eq!(album["title"], "Testalbum");
    assert_eq!(album["artist"], "Interpret");
    assert_eq!(album["author"], Value::Null);
    let detail = body_json(
        app.get(&format!("/items/{}", album["id"]), Some(&token))
            .await,
    )
    .await;
    assert_eq!(detail["tracks"][0]["title"], "Titel 1");
    // Music has no chapters, not even one per file.
    assert_eq!(detail["chapters"], serde_json::json!([]));

    // Nothing changed on disk: the second scan reads no tags.
    let again = scan(&app, &token).await.last_report.unwrap();
    assert_eq!((again.new, again.unchanged), (0, 2));
}

#[sqlx::test]
async fn a_rescan_picks_up_changes_and_removes_vanished_books(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let token = admin(&app).await;
    configure(&app, &library.books(), &library.music()).await;
    make_audio(
        &library.books().join("Autor/Erstes/01.mp3"),
        &[("title", "Eins")],
    );
    make_audio(&library.books().join("Autor/Zweites/01.mp3"), &[]);
    // An album under the music root must survive the books pass whatever happens there.
    make_audio(&library.music().join("Interpret/Album/01.mp3"), &[]);
    assert_eq!(scan(&app, &token).await.last_report.unwrap().new, 3);

    std::fs::remove_dir_all(library.books().join("Autor/Zweites")).unwrap();
    make_audio(
        &library.books().join("Autor/Erstes/02.mp3"),
        &[("title", "Zwei")],
    );
    let report = scan(&app, &token).await.last_report.unwrap();
    assert_eq!((report.updated, report.removed), (1, 1));
    assert_eq!(book_titles(&app).await, ["Erstes"]);
    let tracks: i64 = sqlx::query_scalar("SELECT count(*) FROM tracks")
        .fetch_one(app.db())
        .await
        .unwrap();
    // Two in the book, one in the album.
    assert_eq!(tracks, 3);
    let albums: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE kind = 'album'")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(albums, 1);
}

#[sqlx::test]
async fn an_unreadable_root_is_skipped_not_emptied(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let token = admin(&app).await;
    let gone = library.dir.path().join("weg");
    configure(&app, &gone, &library.music()).await;
    let source = gone.join("Autor/Buch");
    seed(
        app.db(),
        Seed {
            source_path: source.to_str(),
            ..Seed::new("book", "Buch")
        },
    )
    .await;
    let status = scan(&app, &token).await;
    assert_eq!(
        status.last_error,
        Some(format!("{}: Verzeichnis existiert nicht", gone.display()))
    );
    assert_eq!(book_titles(&app).await, ["Buch"]);
}

#[sqlx::test]
async fn a_cancelled_scan_stops_without_removing_anything(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    configure(&app, &library.books(), &library.music()).await;
    let source = library.books().join("Autor/Weg");
    seed(
        app.db(),
        Seed {
            source_path: source.to_str(),
            ..Seed::new("book", "Weg")
        },
    )
    .await;
    assert!(app.state.scan.try_start());
    assert!(app.state.scan.request_cancel());
    run_scan(app.state.clone()).await;
    let status = app.state.scan.snapshot();
    assert!(status.cancelled && !status.running);
    assert_eq!(book_titles(&app).await, ["Weg"]);
}

#[sqlx::test]
async fn takes_a_folder_cover_and_an_embedded_picture(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let token = admin(&app).await;
    configure(&app, &library.books(), &library.music()).await;

    let with_file = library.books().join("Autor/Mit Datei");
    make_audio(&with_file.join("01.mp3"), &[]);
    std::fs::write(with_file.join("folder.jpg"), [0xff, 0xd8, 0xff, 0xe0]).unwrap();

    let embedded = library.books().join("Autor/Eingebettet");
    let picture = library.dir.path().join("bild.png");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=16x16",
            "-frames:v",
            "1",
        ])
        .arg(&picture)
        .status()
        .unwrap();
    assert!(status.success());
    make_audio_with(
        &embedded.join("01.mp3"),
        &[],
        &[
            "-i",
            picture.to_str().unwrap(),
            "-map",
            "0:a",
            "-map",
            "1:v",
            "-c:v",
            "copy",
            "-disposition:v",
            "attached_pic",
            "-id3v2_version",
            "3",
        ],
    );

    let report = scan(&app, &token).await;
    assert_eq!(report.last_error, None);
    let covers: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT title, cover_path FROM items ORDER BY title")
            .fetch_all(app.db())
            .await
            .unwrap();
    assert_eq!(covers[0].0, "Eingebettet");
    let extracted = covers[0].1.clone().expect("embedded picture extracted");
    assert!(extracted.ends_with(".png"), "{extracted}");
    assert!(Path::new(&extracted).starts_with(std::path::absolute(library.data()).unwrap()));
    assert_eq!(
        covers[1].1.as_deref(),
        Some(with_file.join("folder.jpg").to_str().unwrap())
    );

    let items = body_json(app.get("/items?kind=book", Some(&token)).await).await;
    for item in items["items"].as_array().unwrap() {
        assert_eq!(item["has_cover"], true);
        let response = app
            .get(&format!("/items/{}/cover", item["id"]), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{}", item["title"]);
    }
}
