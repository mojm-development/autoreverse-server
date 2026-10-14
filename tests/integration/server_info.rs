//! `GET /server/info` (legacy tests/integration/server-info.test.ts).

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use crate::common::{TestApp, body_json};

#[sqlx::test]
async fn includes_user_count_for_an_admin(db: PgPool) {
    let app = TestApp::new(db);
    let admin = app.user("oliver", false).await; // first user, auto-admin
    let token = app.token(admin).await;
    let body = body_json(app.get("/server/info", Some(&token)).await).await;
    assert_eq!(
        body,
        json!({ "book_count": 0, "album_count": 0, "podcast_count": 0, "track_count": 0, "user_count": 1 })
    );
}

#[sqlx::test]
async fn user_count_is_null_for_a_non_admin(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let user = app.user("mara", false).await;
    let token = app.token(user).await;
    let body = body_json(app.get("/server/info", Some(&token)).await).await;
    assert!(body["user_count"].is_null());
}

#[sqlx::test]
async fn counts_only_present_top_level_items(db: PgPool) {
    let app = TestApp::new(db);
    let admin = app.user("oliver", true).await;
    sqlx::query(
        "INSERT INTO items (id, kind, title, sort_title, parent_id, missing_since) VALUES
            (1, 'book', 'A', 'a', NULL, NULL),
            (2, 'book', 'B', 'b', NULL, now()),
            (3, 'album', 'C', 'c', NULL, NULL),
            (4, 'podcast', 'D', 'd', NULL, NULL),
            (5, 'episode', 'E', 'e', 4, NULL)",
    )
    .execute(app.db())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tracks (item_id, position, path, duration) VALUES
            (1, 0, '/a/1.mp3', 10), (1, 1, '/a/2.mp3', 10), (2, 0, '/b/1.mp3', 10), (3, 0, '/c/1.mp3', 10)",
    )
    .execute(app.db())
    .await
    .unwrap();
    let token = app.token(admin).await;
    let body = body_json(app.get("/server/info", Some(&token)).await).await;
    assert_eq!(body["book_count"], 1);
    assert_eq!(body["album_count"], 1);
    assert_eq!(body["podcast_count"], 1);
    assert_eq!(body["track_count"], 3);
}

#[sqlx::test]
async fn requires_a_token(db: PgPool) {
    let app = TestApp::new(db);
    assert_eq!(
        app.get("/server/info", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
