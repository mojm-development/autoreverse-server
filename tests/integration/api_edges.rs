//! Edges of the JSON API where legacy's framework decided the behaviour: trailing slashes,
//! repeated query keys, lookup before body parsing, one transaction per user update.

use axum::http::{Method, StatusCode, header};
use serde_json::json;
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_json, header_str, seed};

async fn signed_in(db: PgPool) -> (TestApp, String, i32) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    (app, token, user)
}

#[sqlx::test]
async fn a_trailing_slash_redirects_permanently_to_the_route(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let response = app.get("/items/?kind=album&limit=5", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        header_str(&response, header::LOCATION),
        "/items?kind=album&limit=5"
    );
    let page = app.get("/library/albums/", None).await;
    assert_eq!(page.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(header_str(&page, header::LOCATION), "/library/albums");
    // Never off-site: a doubled leading slash is a protocol-relative URL.
    let sneaky = app.get("//evil.example/", None).await;
    assert_eq!(header_str(&sneaky, header::LOCATION), "/evil.example");
    // The root itself stays.
    assert_ne!(
        app.get("/", None).await.status(),
        StatusCode::PERMANENT_REDIRECT
    );
}

#[sqlx::test]
async fn a_repeated_query_key_takes_its_first_value(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for title in ["A", "B", "C"] {
        seed(app.db(), Seed::new("album", title)).await;
    }
    let body = body_json(app.get("/items?limit=1&limit=500", Some(&token)).await).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    let refused = app.get("/items?limit=0&limit=5", Some(&token)).await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// A raw (broken) body on a route with a path id.
async fn send_raw(app: &TestApp, method: Method, uri: &str, token: &str) -> StatusCode {
    let request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from("{kaputt"))
        .unwrap();
    app.send(request).await.status()
}

#[sqlx::test]
async fn an_unknown_target_is_404_even_with_a_broken_body(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for (method, uri) in [
        (Method::PUT, "/progress/999999"),
        (Method::POST, "/progress/999999"),
        (Method::PATCH, "/playlists/999999"),
        (Method::POST, "/playlists/999999/entries"),
        (Method::PUT, "/playlists/999999/entries/1"),
    ] {
        assert_eq!(
            send_raw(&app, method.clone(), uri, &token).await,
            StatusCode::NOT_FOUND,
            "{method} {uri}"
        );
    }
    // A known target still refuses the broken body.
    let item = seed(app.db(), Seed::new("book", "B")).await;
    assert_eq!(
        send_raw(&app, Method::PUT, &format!("/progress/{item}"), &token).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let created = body_json(
        app.post_json("/playlists", json!({ "name": "L" }), Some(&token))
            .await,
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    assert_eq!(
        send_raw(&app, Method::PATCH, &format!("/playlists/{id}"), &token).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test]
async fn a_refused_demotion_leaves_the_password_alone(db: PgPool) {
    let app = TestApp::new(db);
    let admin = app.user("oliver", true).await;
    let token = app.token(admin).await;
    let other = app.user("mara", false).await;
    let other_token = app.token(other).await;
    let hash_before: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(admin)
        .fetch_one(app.db())
        .await
        .unwrap();
    let response = app
        .call(
            Method::PATCH,
            &format!("/users/{admin}"),
            Some(json!({ "is_admin": false, "password": "ganzneuespasswort" })),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let hash_after: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(admin)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(hash_before, hash_after);

    // Both at once on another user: role and password change together, sessions end.
    let response = app
        .call(
            Method::PATCH,
            &format!("/users/{other}"),
            Some(json!({ "is_admin": true, "password": "ganzneuespasswort" })),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["is_admin"], true);
    assert_eq!(
        app.get("/me", Some(&other_token)).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
