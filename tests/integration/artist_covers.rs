//! Choosing an artist's picture: `PUT|DELETE /artists/{name}/cover`, `POST /artists/{name}/image`
//! (the write half of legacy artist-covers.test.ts; reading is in music.rs).

use autoreverse_server::library::artist_covers::{chosen_cover, image_file_name};
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::json;
use sqlx::PgPool;

use crate::common::{Seed, TempLibrary, TestApp, body_json, seed};

const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const BOUNDARY: &str = "XyZBoundary";

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

/// A multipart body with one file field.
pub fn multipart(field: &str, content_type: &str, bytes: &[u8], with_filename: bool) -> Vec<u8> {
    let mut body = Vec::new();
    let disposition = if with_filename {
        format!("form-data; name=\"{field}\"; filename=\"bild\"")
    } else {
        format!("form-data; name=\"{field}\"")
    };
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: {disposition}\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn upload(app: &TestApp, token: &str, artist: &str, body: Vec<u8>) -> (StatusCode, String) {
    let response = app
        .send(
            Request::post(format!("/artists/{artist}/image"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
    let status = response.status();
    let body = body_json(response).await;
    let text = body
        .get("detail")
        .and_then(|d| d.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| body.to_string());
    (status, text)
}

async fn admin(app: &TestApp) -> String {
    let id = app.user("oliver", true).await;
    app.token(id).await
}

#[sqlx::test]
async fn choosing_an_album_stores_the_pick_and_reports_it_back(db: PgPool) {
    let app = TestApp::new(db);
    let token = admin(&app).await;
    let id = album(app.db(), "Die Ärzte", "13").await;

    let response = app
        .call(
            Method::PUT,
            "/artists/Die%20%C3%84rzte/cover",
            Some(json!({ "item_id": id })),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({ "artist": "Die Ärzte", "item_id": id })
    );
    let cover = chosen_cover(app.db(), "Die Ärzte").await.unwrap().unwrap();
    assert_eq!((cover.item_id, cover.image_path), (Some(id), None));
}

#[sqlx::test]
async fn a_new_pick_replaces_the_old_one_and_foreign_albums_are_refused(db: PgPool) {
    let app = TestApp::new(db);
    let token = admin(&app).await;
    let first = album(app.db(), "Die Ärzte", "13").await;
    let second = album(app.db(), "Die Ärzte", "Jazz ist anders").await;
    let foreign = album(app.db(), "Bon Jovi", "Crush").await;
    let put = |id: serde_json::Value| {
        app.call(
            Method::PUT,
            "/artists/Die%20%C3%84rzte/cover",
            Some(json!({ "item_id": id })),
            Some(&token),
        )
    };
    put(json!(first)).await;
    put(json!(second.to_string())).await;
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM artist_covers")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(
        chosen_cover(app.db(), "Die Ärzte")
            .await
            .unwrap()
            .unwrap()
            .item_id,
        Some(second)
    );

    let response = put(json!(foreign)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(response).await["detail"],
        "Album gehört nicht zu diesem Interpreten"
    );
    for bad in [json!(0), json!(-1), json!(1.5), json!("x"), json!(null)] {
        let response = put(bad).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body_json(response).await["detail"], "Ungültige Album-ID");
    }
}

#[sqlx::test]
async fn an_upload_writes_the_file_and_replaces_an_album_pick_and_reset_removes_it(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let token = admin(&app).await;
    let id = album(app.db(), "Die Ärzte", "13").await;
    app.call(
        Method::PUT,
        "/artists/Die%20%C3%84rzte/cover",
        Some(json!({ "item_id": id })),
        Some(&token),
    )
    .await;

    let (status, body) = upload(
        &app,
        &token,
        "Die%20%C3%84rzte",
        multipart("image", "image/png", PNG, true),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body, json!({ "artist": "Die Ärzte" }).to_string());
    let path = library
        .data()
        .join("artists")
        .join(image_file_name("Die Ärzte", ".png"));
    assert_eq!(std::fs::read(&path).unwrap(), PNG);
    let cover = chosen_cover(app.db(), "Die Ärzte").await.unwrap().unwrap();
    assert_eq!(cover.item_id, None);
    assert_eq!(cover.image_path.as_deref(), path.to_str());

    let response = app
        .call(
            Method::DELETE,
            "/artists/Die%20%C3%84rzte/cover",
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(chosen_cover(app.db(), "Die Ärzte").await.unwrap().is_none());
    assert!(!path.exists());
}

#[sqlx::test]
async fn an_album_pick_after_an_upload_deletes_the_file(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let token = admin(&app).await;
    let id = album(app.db(), "Kraftwerk", "Autobahn").await;
    upload(
        &app,
        &token,
        "Kraftwerk",
        multipart("image", "image/png", PNG, true),
    )
    .await;
    let path = library
        .data()
        .join("artists")
        .join(image_file_name("Kraftwerk", ".png"));
    assert!(path.exists());
    app.call(
        Method::PUT,
        "/artists/Kraftwerk/cover",
        Some(json!({ "item_id": id })),
        Some(&token),
    )
    .await;
    assert!(!path.exists());
}

#[sqlx::test]
async fn uploads_are_checked_for_format_size_and_shape(db: PgPool) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let token = admin(&app).await;
    let big = vec![0u8; 5 * 1024 * 1024 + 10];
    for (body, status, detail) in [
        (
            multipart("image", "application/pdf", PNG, true),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Nicht unterstütztes Bildformat",
        ),
        (
            multipart("image", "image/png", &big, true),
            StatusCode::PAYLOAD_TOO_LARGE,
            "Bild ist zu groß (maximal 5 MB)",
        ),
        (
            multipart("image", "image/png", &[], true),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Leere Datei",
        ),
        (
            multipart("bild", "image/png", PNG, true),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Keine Bilddatei im Feld \"image\"",
        ),
        (
            multipart("image", "text/plain", b"kein File", false),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Keine Bilddatei im Feld \"image\"",
        ),
    ] {
        let (got, text) = upload(&app, &token, "X", body).await;
        assert_eq!((got, text.as_str()), (status, detail));
    }
    let response = app
        .post_json("/artists/X/image", json!({}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(response).await["detail"], "Ungültiger Upload");
    assert!(chosen_cover(app.db(), "X").await.unwrap().is_none());
}

#[sqlx::test]
async fn choosing_is_for_verwalter(db: PgPool) {
    let app = TestApp::new(db);
    admin(&app).await;
    let guest = app.user("gast", false).await;
    let token = app.token(guest).await;
    let id = album(app.db(), "Kraftwerk", "Autobahn").await;
    let response = app
        .call(
            Method::PUT,
            "/artists/Kraftwerk/cover",
            Some(json!({ "item_id": id })),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .call(
            Method::DELETE,
            "/artists/Kraftwerk/cover",
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let (status, detail) = upload(
        &app,
        &token,
        "Kraftwerk",
        multipart("image", "image/png", PNG, true),
    )
    .await;
    assert_eq!(
        (status, detail.as_str()),
        (StatusCode::FORBIDDEN, "Nur für Verwalter")
    );
}
