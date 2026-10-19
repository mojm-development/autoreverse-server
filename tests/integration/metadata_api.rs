//! Metadata editing: `PATCH /items/{id}`, `PATCH /tracks/{id}`, `POST /items/bulk`,
//! `POST /items/bulk/undo`, `POST /items/series`, `DELETE /items/missing` (legacy
//! metadata-edit, metadata-bulk, metadata-series and the missing part of scan-api tests).

use std::path::Path;

use autoreverse_server::library::bulk_metadata::BULK_LIMIT;
use autoreverse_server::library::paths::set_library_paths;
use autoreverse_server::scanner::books::{ItemKind, ScannedItem, ScannedTrack};
use autoreverse_server::scanner::chapters::ScannedChapter;
use autoreverse_server::scanner::store::store_items;
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_json, seed, seed_track};

async fn admin(app: &TestApp) -> (i32, String) {
    let id = app.user("oliver", true).await;
    (id, app.token(id).await)
}

async fn guest(app: &TestApp) -> String {
    let id = app.user("gast", false).await;
    app.token(id).await
}

async fn patch_item(app: &TestApp, token: &str, id: i32, body: Value) -> (StatusCode, Value) {
    let response = app
        .call(
            Method::PATCH,
            &format!("/items/{id}"),
            Some(body),
            Some(token),
        )
        .await;
    (response.status(), body_json(response).await)
}

async fn post(app: &TestApp, token: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app.post_json(uri, body, Some(token)).await;
    (response.status(), body_json(response).await)
}

fn scanned(title: &str, author: &str, mtime: f64) -> ScannedItem {
    ScannedItem {
        source_path: "/library/books/Autor/Buch".into(),
        kind: ItemKind::Book,
        title: title.into(),
        author: Some(author.into()),
        artist: None,
        album_artist: None,
        description: None,
        series: None,
        year: None,
        genre: None,
        series_index: None,
        tracks: vec![ScannedTrack {
            path: "/library/books/Autor/Buch/01.mp3".into(),
            position: 1,
            title: Some("Track aus der Datei".into()),
            artist: None,
            disc: None,
            duration: 10.0,
            mtime,
            size: mtime as i64,
        }],
        chapters: vec![ScannedChapter {
            title: "K".into(),
            start: 0.0,
            end: 10.0,
        }],
        unchanged: false,
    }
}

async fn store(db: &PgPool, item: ScannedItem) {
    let mut failures = Vec::new();
    store_items(
        db,
        &[item],
        Path::new("/nonexistent/covers"),
        &mut failures,
        |_, _, _| {},
        || false,
    )
    .await;
    assert!(failures.is_empty(), "{failures:?}");
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct Row {
    id: i32,
    title: String,
    sort_title: String,
    author: Option<String>,
    narrator: Option<String>,
    artist: Option<String>,
    series: Option<String>,
    series_index: Option<f64>,
    year: Option<i32>,
    locked_fields: Vec<String>,
}

async fn row(db: &PgPool, id: i32) -> Row {
    sqlx::query_as("SELECT * FROM items WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn rows(db: &PgPool) -> Vec<Row> {
    sqlx::query_as("SELECT * FROM items ORDER BY sort_title")
        .fetch_all(db)
        .await
        .unwrap()
}

async fn the_item(db: &PgPool) -> i32 {
    sqlx::query_scalar("SELECT id FROM items")
        .fetch_one(db)
        .await
        .unwrap()
}

async fn edit_count(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM metadata_edits")
        .fetch_one(db)
        .await
        .unwrap()
}

// --- single edits (metadata-edit.test.ts) ---

#[sqlx::test]
async fn locks_what_a_person_set_so_the_next_scan_cannot_undo_it(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    store(
        app.db(),
        scanned("Titel aus der Datei", "Autor aus der Datei", 100.0),
    )
    .await;
    let id = the_item(app.db()).await;

    let (status, _) = patch_item(
        &app,
        &token,
        id,
        json!({ "set": { "title": "Der richtige Titel", "narrator": "Anna Sprecherin" } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A changed file makes the scanner rewrite the folder — the correction stays.
    store(
        app.db(),
        scanned("Titel aus der Datei", "Neuer Autor aus der Datei", 999.0),
    )
    .await;
    let after = row(app.db(), id).await;
    assert_eq!(after.title, "Der richtige Titel");
    assert_eq!(after.narrator.as_deref(), Some("Anna Sprecherin"));
    // The sort key follows a locked title rather than falling back to the file's.
    assert_eq!(after.sort_title, "der richtige titel");
    // Everything untouched still belongs to the scanner.
    assert_eq!(after.author.as_deref(), Some("Neuer Autor aus der Datei"));
}

#[sqlx::test]
async fn hands_a_field_back_to_the_scanner_when_it_is_reset(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    store(app.db(), scanned("Titel aus der Datei", "A", 100.0)).await;
    let id = the_item(app.db()).await;

    patch_item(
        &app,
        &token,
        id,
        json!({ "set": { "title": "Handkorrektur" } }),
    )
    .await;
    let (status, body) = patch_item(&app, &token, id, json!({ "reset": ["title"] })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locked_fields"], json!([]));

    store(app.db(), scanned("Titel aus der Datei", "A", 998.0)).await;
    assert_eq!(row(app.db(), id).await.title, "Titel aus der Datei");
}

#[sqlx::test]
async fn records_every_change_with_the_value_it_replaced_under_one_batch_id(db: PgPool) {
    let app = TestApp::new(db);
    let (user, token) = admin(&app).await;
    store(app.db(), scanned("Titel aus der Datei", "A", 100.0)).await;
    let id = the_item(app.db()).await;

    let (_, body) = patch_item(
        &app,
        &token,
        id,
        json!({ "set": { "title": "Neu", "year": 2011 } }),
    )
    .await;
    let batch = body["batch_id"].as_str().unwrap();
    type Logged = (String, Option<String>, Option<String>, Option<i32>);
    let edits: Vec<Logged> = sqlx::query_as(
        "SELECT field, old_value, new_value, user_id FROM metadata_edits WHERE batch_id = $1",
    )
    .bind(batch)
    .fetch_all(app.db())
    .await
    .unwrap();
    let by_field = |name: &str| edits.iter().find(|e| e.0 == name).unwrap().clone();
    assert_eq!(by_field("title").1.as_deref(), Some("Titel aus der Datei"));
    assert_eq!(by_field("title").2.as_deref(), Some("Neu"));
    assert_eq!(by_field("year").1, None);
    assert_eq!(by_field("year").2.as_deref(), Some("2011"));
    // The derived sort key is logged as well, or an undo could not restore it.
    assert_eq!(by_field("sortTitle").2.as_deref(), Some("neu"));
    assert!(edits.iter().all(|e| e.3 == Some(user)));
}

#[sqlx::test]
async fn clears_a_field_on_null_and_keeps_it_clear_and_refuses_nonsense(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    store(app.db(), scanned("T", "Falscher Autor", 100.0)).await;
    let id = the_item(app.db()).await;

    let (status, body) = patch_item(&app, &token, id, json!({ "set": { "author": null } })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["author"], Value::Null);
    assert!(
        body["locked_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("author"))
    );

    for (request, detail) in [
        (
            json!({ "set": { "kind": "album" } }),
            "Unbekanntes Feld: kind",
        ),
        (
            json!({ "set": { "year": "bald" } }),
            "year muss eine Zahl sein",
        ),
        (json!({}), "Keine Änderungen angegeben"),
        (
            json!({ "set": { "title": "" } }),
            "title darf nicht leer sein",
        ),
    ] {
        let (status, body) = patch_item(&app, &token, id, request).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["detail"], detail);
    }
    let response = app
        .send(
            axum::http::Request::patch(format!("/items/{id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::from("{kaputt"))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(response).await["detail"], "Ungültiger JSON-Body");
}

#[sqlx::test]
async fn a_cleared_sort_title_follows_the_title_again(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let id = seed(app.db(), Seed::new("album", "Iron Man 2")).await;
    patch_item(&app, &token, id, json!({ "set": { "sort_title": "zzz" } })).await;
    assert_eq!(row(app.db(), id).await.locked_fields, ["sortTitle"]);

    let (status, body) = patch_item(&app, &token, id, json!({ "set": { "sort_title": "" } })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locked_fields"], json!([]));
    assert_eq!(row(app.db(), id).await.sort_title, "iron man 2");
}

#[sqlx::test]
async fn edits_a_track_title_and_locks_it_through_the_next_scan(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    store(app.db(), scanned("T", "A", 100.0)).await;
    let track: i32 = sqlx::query_scalar("SELECT id FROM tracks")
        .fetch_one(app.db())
        .await
        .unwrap();

    let response = app
        .call(
            Method::PATCH,
            &format!("/tracks/{track}"),
            Some(json!({ "set": { "title": "Kapitel eins" } })),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["title"], "Kapitel eins");
    assert_eq!(body["locked_fields"], json!(["title"]));
    assert_eq!(body["changed"], json!(["title"]));
    assert_eq!(body["disc"], Value::Null);
    assert!(body["batch_id"].is_string());

    store(app.db(), scanned("T", "A", 999.0)).await;
    let title: Option<String> = sqlx::query_scalar("SELECT title FROM tracks WHERE id = $1")
        .bind(track)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(title.as_deref(), Some("Kapitel eins"));
}

#[sqlx::test]
async fn unknown_items_and_tracks_are_404(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let body = json!({ "set": { "title": "X" } });
    for (uri, detail) in [
        ("/items/999", "Unbekanntes Item"),
        ("/items/abc", "Unbekanntes Item"),
        ("/tracks/999", "Unbekannter Titel"),
        ("/tracks/abc", "Unbekannter Titel"),
    ] {
        let response = app
            .call(Method::PATCH, uri, Some(body.clone()), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body_json(response).await["detail"], detail);
    }
    let response = app
        .call(
            Method::PATCH,
            "/tracks/1",
            Some(json!({ "set": { "author": "X" } })),
            Some(&token),
        )
        .await;
    assert_eq!(
        body_json(response).await["detail"],
        "Unbekanntes Feld: author"
    );
}

#[sqlx::test]
async fn patch_item_edits_for_a_verwalter_and_refuses_everyone_else(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let guest = guest(&app).await;
    store(app.db(), scanned("T", "A", 100.0)).await;
    let id = the_item(app.db()).await;

    let (status, body) = patch_item(
        &app,
        &guest,
        id,
        json!({ "set": { "title": "Nicht erlaubt" } }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["detail"], "Nur für Verwalter");
    let response = app
        .call(
            Method::PATCH,
            &format!("/items/{id}"),
            Some(json!({})),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let (status, body) =
        patch_item(&app, &token, id, json!({ "set": { "title": "Erlaubt" } })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Erlaubt");
    assert_eq!(body["locked_fields"], json!(["sortTitle", "title"]));
    assert!(
        body["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("title"))
    );
    assert!(body["batch_id"].is_string());
    assert_eq!(body["kind"], "book");
    assert!(body.get("description").is_some());
}

// --- bulk (metadata-bulk.test.ts) ---

async fn seed_albums(db: &PgPool) -> (i32, i32, i32) {
    let album = |title, artist| Seed {
        artist: Some(artist),
        ..Seed::new("album", title)
    };
    (
        seed(db, album("Live in Berlin (Remaster)", "ACDC")).await,
        seed(db, album("Back in Black (Remaster)", "ACDC")).await,
        seed(db, album("Nevermind", "Nirvana")).await,
    )
}

#[sqlx::test]
async fn a_dry_run_answers_with_the_diff_and_writes_nothing(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let (first, second, _) = seed_albums(app.db()).await;

    let (status, preview) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "ids": [first, second], "set": { "artist": "AC/DC" } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["matched"], 2);
    assert_eq!(preview["changed"], 2);
    assert_eq!(preview["batch_id"], Value::Null);
    assert_eq!(
        preview["changes"][0]["fields"],
        json!([{ "field": "artist", "old": "ACDC", "new": "AC/DC" }])
    );
    assert_eq!(row(app.db(), first).await.artist.as_deref(), Some("ACDC"));
    assert_eq!(edit_count(app.db()).await, 0);
}

#[sqlx::test]
async fn applies_a_search_and_replace_across_a_filter_and_locks_what_it_wrote(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    seed_albums(app.db()).await;

    let (_, result) = post(
        &app,
        &token,
        "/items/bulk",
        json!({
            "filter": { "kind": "album", "artist": "ACDC" },
            "replace": { "field": "title", "from": " (Remaster)", "to": "" },
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(result["changed"], 2);
    assert!(result["batch_id"].is_string());

    let all = rows(app.db()).await;
    let titles: Vec<&str> = all.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, ["Back in Black", "Live in Berlin", "Nevermind"]);
    // The sort key follows the title, and both are locked against the scanner.
    assert_eq!(all[0].sort_title, "back in black");
    assert_eq!(all[0].locked_fields, ["sortTitle", "title"]);
    // The album the filter did not match is untouched.
    assert!(all[2].locked_fields.is_empty());
}

#[sqlx::test]
async fn a_regex_replace_uses_groups(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let (first, _, _) = seed_albums(app.db()).await;
    let (_, result) = post(
        &app,
        &token,
        "/items/bulk",
        json!({
            "ids": [first],
            "replace": { "field": "title", "from": "^Live in (\\w+).*$", "to": "Berlin: $1", "regex": true },
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(result["changed"], 1);
    assert_eq!(row(app.db(), first).await.title, "Berlin: Berlin");
}

#[sqlx::test]
async fn undo_restores_the_values_and_the_locks_that_came_with_them(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let (first, _, _) = seed_albums(app.db()).await;

    let (_, applied) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "ids": [first], "set": { "artist": "AC/DC", "year": 1980 }, "dry_run": false }),
    )
    .await;
    let (status, undone) = post(
        &app,
        &token,
        "/items/bulk/undo",
        json!({ "batch_id": applied["batch_id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(undone, json!({ "restored": 2, "skipped": 0 }));

    let after = row(app.db(), first).await;
    assert_eq!(after.artist.as_deref(), Some("ACDC"));
    assert_eq!(after.year, None);
    // Locks go back too, or the restored value would stay frozen.
    assert!(after.locked_fields.is_empty());
    assert_eq!(edit_count(app.db()).await, 0);
}

#[sqlx::test]
async fn undo_leaves_alone_what_someone_changed_after_the_batch(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let (first, _, _) = seed_albums(app.db()).await;

    let (_, applied) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "ids": [first], "set": { "artist": "AC/DC", "year": 1980 }, "dry_run": false }),
    )
    .await;
    // Someone corrects the artist again by hand before pressing undo.
    patch_item(
        &app,
        &token,
        first,
        json!({ "set": { "artist": "AC/DC (live)" } }),
    )
    .await;

    let (_, undone) = post(
        &app,
        &token,
        "/items/bulk/undo",
        json!({ "batch_id": applied["batch_id"] }),
    )
    .await;
    assert_eq!(undone["skipped"], 1);
    let after = row(app.db(), first).await;
    assert_eq!(after.artist.as_deref(), Some("AC/DC (live)"));
    // The untouched field still rolls back.
    assert_eq!(after.year, None);
    // The later hand edit keeps its lock; the rolled-back field loses its own.
    assert_eq!(after.locked_fields, ["artist"]);
}

#[sqlx::test]
async fn undo_takes_back_a_track_edit_too(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let album = seed(app.db(), Seed::new("album", "A")).await;
    let track = seed_track(app.db(), album, 1, "/x/01.mp3", 10.0).await;
    let response = app
        .call(
            Method::PATCH,
            &format!("/tracks/{track}"),
            Some(json!({ "set": { "title": "Neu", "disc": 2 } })),
            Some(&token),
        )
        .await;
    let batch = body_json(response).await["batch_id"].clone();
    let (_, undone) = post(
        &app,
        &token,
        "/items/bulk/undo",
        json!({ "batch_id": batch }),
    )
    .await;
    assert_eq!(undone, json!({ "restored": 2, "skipped": 0 }));
    let (title, disc, locks): (Option<String>, Option<i32>, Vec<String>) =
        sqlx::query_as("SELECT title, disc, locked_fields FROM tracks WHERE id = $1")
            .bind(track)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!((title, disc), (None, None));
    assert!(locks.is_empty());
}

#[sqlx::test]
async fn refuses_an_ambiguous_or_oversized_request_instead_of_guessing(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let (first, _, _) = seed_albums(app.db()).await;
    let too_many: Vec<usize> = (1..=BULK_LIMIT + 1).collect();

    for (body, detail) in [
        (
            json!({ "ids": [first], "filter": { "kind": "album" }, "set": { "year": 1 } }),
            "Entweder ids oder filter, nicht beides".to_owned(),
        ),
        (
            json!({ "ids": [first] }),
            "Keine Änderungen angegeben".into(),
        ),
        (
            json!({ "ids": too_many, "set": { "year": 1 } }),
            format!("Höchstens {BULK_LIMIT} Items auf einmal"),
        ),
        (
            json!({ "ids": [first], "replace": { "field": "title", "from": "(", "to": "", "regex": true } }),
            "Ungültiger regulärer Ausdruck".into(),
        ),
        (
            json!({ "ids": [first], "replace": { "field": "year", "from": "1", "to": "2" } }),
            "Ersetzen geht nur in Textfeldern".into(),
        ),
        (
            json!({ "ids": [first], "replace": { "field": "title", "from": "" } }),
            "Suchtext fehlt".into(),
        ),
        (
            json!({ "ids": [], "set": { "year": 1 } }),
            "Keine Items ausgewählt".into(),
        ),
        (
            json!({ "ids": [1.5], "set": { "year": 1 } }),
            "Ungültige Item-ID".into(),
        ),
        (
            json!({ "set": { "year": 1 } }),
            "Weder ids noch filter angegeben".into(),
        ),
    ] {
        let (status, answer) = post(&app, &token, "/items/bulk", body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{detail}");
        assert_eq!(answer["detail"], detail.as_str());
    }
    let (status, answer) = post(
        &app,
        &token,
        "/items/bulk/undo",
        json!({ "batch_id": "gibt-es-nicht" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(answer["detail"], "Unbekannte Änderung");
}

#[sqlx::test]
async fn a_filter_that_matches_too_much_is_refused(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    sqlx::query(
        "INSERT INTO items (kind, title, sort_title)
         SELECT 'album', 'A' || n, 'a' || n FROM generate_series(1, $1) AS n",
    )
    .bind(BULK_LIMIT as i32 + 1)
    .execute(app.db())
    .await
    .unwrap();
    let (status, answer) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "filter": { "kind": "album" }, "set": { "year": 1 } }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        answer["detail"],
        format!("Der Filter trifft mehr als {BULK_LIMIT} Items")
    );
}

#[sqlx::test]
async fn counts_what_matched_even_when_most_of_it_needs_no_change(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    seed_albums(app.db()).await;
    let (_, preview) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "filter": { "kind": "album" }, "set": { "artist": "ACDC" } }),
    )
    .await;
    assert_eq!(preview["matched"], 3);
    // Two already carry that artist: nothing to change, but the field does come off the scanner
    // — reported separately rather than as a diff.
    assert_eq!(preview["changed"], 1);
    assert_eq!(preview["locked_only"], 2);
    assert_eq!(preview["changes"].as_array().unwrap().len(), 1);
    assert_eq!(preview["changes"][0]["title"], "Nevermind");
    assert_eq!(preview["truncated"], false);
}

#[sqlx::test]
async fn filters_by_query_and_missing(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    seed_albums(app.db()).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            artist: Some("ACDC"),
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    let (_, preview) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "filter": { "kind": "album", "q": "acdc", "missing": false }, "set": { "year": 1 } }),
    )
    .await;
    assert_eq!(preview["matched"], 2);
    let (_, preview) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "filter": { "kind": "album", "missing": true }, "set": { "year": 1 } }),
    )
    .await;
    assert_eq!(preview["matched"], 1);
    assert_eq!(preview["changes"][0]["title"], "Weg");
}

#[sqlx::test]
async fn the_endpoints_are_for_verwalter_and_undo_needs_a_batch_id(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let guest = guest(&app).await;
    let (first, _, _) = seed_albums(app.db()).await;

    for uri in ["/items/bulk", "/items/bulk/undo", "/items/series"] {
        let (status, body) = post(&app, &guest, uri, json!({ "ids": [first] })).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(body["detail"], "Nur für Verwalter");
    }

    let (status, payload) = post(
        &app,
        &token,
        "/items/bulk",
        json!({ "ids": [first], "set": { "artist": "AC/DC" } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The route defaults to a dry run: an omitted dry_run must not write.
    assert_eq!(payload["batch_id"], Value::Null);
    assert_eq!(payload["changed"], 1);

    let (status, body) = post(&app, &token, "/items/bulk/undo", json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"], "batch_id fehlt");
}

// --- series assistant (metadata-series.test.ts) ---

async fn seed_books(db: &PgPool) -> Vec<i32> {
    let book = |title, author, source| Seed {
        author: Some(author),
        source_path: Some(source),
        ..Seed::new("book", title)
    };
    vec![
        seed(
            db,
            book(
                "Die Gefährten",
                "Tolkien",
                "/books/Tolkien/Der Herr der Ringe 01 - Die Gefährten",
            ),
        )
        .await,
        seed(
            db,
            book(
                "Der Fluch",
                "Eschbach",
                "/books/Eschbach/Die drei Sonnen/03 - Der Fluch",
            ),
        )
        .await,
        seed(
            db,
            book("Das Parfum", "Süskind", "/books/Süskind/Das Parfum"),
        )
        .await,
    ]
}

#[sqlx::test]
async fn detects_series_and_volume_from_the_folder_and_leaves_a_standalone_book_alone(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    set_library_paths(app.db(), "/books", "/music")
        .await
        .unwrap();
    seed_books(app.db()).await;

    let (_, preview) = post(
        &app,
        &token,
        "/items/series",
        json!({ "filter": { "kind": "book" } }),
    )
    .await;
    assert_eq!(preview["matched"], 3);
    // Only the two that actually belong to a series.
    assert_eq!(preview["changed"], 2);
    assert_eq!(preview["batch_id"], Value::Null);

    let (_, applied) = post(
        &app,
        &token,
        "/items/series",
        json!({ "filter": { "kind": "book" }, "dry_run": false }),
    )
    .await;
    assert_eq!(applied["changed"], 2);

    let all = rows(app.db()).await;
    let find = |title: &str| all.iter().find(|r| r.title == title).unwrap();
    assert_eq!(
        find("Die Gefährten").series.as_deref(),
        Some("Der Herr der Ringe")
    );
    assert_eq!(find("Die Gefährten").series_index, Some(1.0));
    // Series folder above, volume number in the book's own folder.
    assert_eq!(find("Der Fluch").series.as_deref(), Some("Die drei Sonnen"));
    assert_eq!(find("Der Fluch").series_index, Some(3.0));
    // A book that is not in a series stays untouched and unlocked.
    assert_eq!(find("Das Parfum").series, None);
    assert!(find("Das Parfum").locked_fields.is_empty());
}

#[sqlx::test]
async fn assigns_one_series_and_numbers_it_through_and_undo_takes_it_back(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    set_library_paths(app.db(), "/books", "/music")
        .await
        .unwrap();
    let books = seed_books(app.db()).await;

    let (_, applied) = post(
        &app,
        &token,
        "/items/series",
        json!({ "ids": books, "mode": "assign", "series": "Sammlung", "start": 5, "dry_run": false }),
    )
    .await;
    assert_eq!(applied["changed"], 3);

    let all = rows(app.db()).await;
    // Numbered in the order the list shows: sorted by title, starting at five.
    let numbered: Vec<(&str, Option<f64>)> = all
        .iter()
        .map(|r| (r.title.as_str(), r.series_index))
        .collect();
    assert_eq!(
        numbered,
        [
            ("Das Parfum", Some(5.0)),
            ("Der Fluch", Some(6.0)),
            ("Die Gefährten", Some(7.0))
        ]
    );
    assert!(all.iter().all(|r| r.series.as_deref() == Some("Sammlung")));
    assert_eq!(all[0].locked_fields, ["series", "seriesIndex"]);

    let (_, undone) = post(
        &app,
        &token,
        "/items/bulk/undo",
        json!({ "batch_id": applied["batch_id"] }),
    )
    .await;
    assert_eq!(undone["skipped"], 0);
    let all = rows(app.db()).await;
    assert!(
        all.iter()
            .all(|r| r.series.is_none() && r.series_index.is_none())
    );
    assert!(all.iter().all(|r| r.locked_fields.is_empty()));
}

#[sqlx::test]
async fn refuses_an_assignment_without_a_name_and_a_selection_without_books(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let books = seed_books(app.db()).await;
    let album = seed(app.db(), Seed::new("album", "Nevermind")).await;

    for (body, detail) in [
        (
            json!({ "ids": books, "mode": "assign" }),
            "Serienname fehlt",
        ),
        // Albums have no series; a selection of them has nothing to plan.
        (json!({ "ids": [album] }), "Keine Hörbücher ausgewählt"),
        (
            json!({ "ids": books, "mode": "assign", "series": "S", "start": "fünf" }),
            "Startnummer muss eine Zahl sein",
        ),
    ] {
        let (status, answer) = post(&app, &token, "/items/series", body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(answer["detail"], detail);
    }
}

// --- missing items (scan-api.test.ts) ---

#[sqlx::test]
async fn delete_missing_removes_the_missing_and_waits_for_a_running_scan(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin(&app).await;
    let guest = guest(&app).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("book", "Weg")
        },
    )
    .await;
    let kept = seed(app.db(), Seed::new("book", "Da")).await;

    let response = app
        .call(Method::DELETE, "/items/missing", None, Some(&guest))
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .call(Method::DELETE, "/items/missing", None, Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, json!({ "removed": 1 }));
    assert_eq!(the_item(app.db()).await, kept);

    assert!(app.state.scan.try_start());
    let response = app
        .call(Method::DELETE, "/items/missing", None, Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["detail"],
        "Es läuft gerade ein Scan"
    );
}
