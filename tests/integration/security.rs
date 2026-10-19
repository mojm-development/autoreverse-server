//! Browser hardening beyond legacy: security headers, cross-site requests without a form body,
//! podcast changes only for admins.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::json;
use sqlx::PgPool;

use crate::common::{TestApp, body_json, header_str};

#[sqlx::test]
async fn every_response_carries_the_security_headers(db: PgPool) {
    let app = TestApp::new(db);
    for path in ["/login", "/css/app.css", "/gibt-es-nicht"] {
        let response = app.get(path, None).await;
        assert_eq!(
            header_str(&response, header::X_CONTENT_TYPE_OPTIONS),
            "nosniff",
            "{path}"
        );
        assert_eq!(header_str(&response, header::X_FRAME_OPTIONS), "DENY");
        assert_eq!(
            header_str(&response, header::REFERRER_POLICY),
            "same-origin"
        );
        let csp = header_str(&response, header::CONTENT_SECURITY_POLICY);
        assert!(csp.contains("script-src 'self';"), "{csp}");
        assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    }
}

#[sqlx::test]
async fn the_page_has_no_inline_script(db: PgPool) {
    let app = TestApp::new(db);
    let html = crate::common::body_text(app.get("/login", None).await).await;
    for (index, _) in html.match_indices("<script") {
        assert!(
            html[index..].starts_with("<script src="),
            "inline script: {}",
            &html[index..(index + 60).min(html.len())]
        );
    }
}

fn cookie_post(uri: &str, cookie: &str, site: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::COOKIE, cookie);
    if let Some(site) = site {
        request = request.header("sec-fetch-site", site);
    }
    request.body(Body::empty()).unwrap()
}

#[sqlx::test]
async fn a_body_less_post_from_another_site_is_refused(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = app.token(id).await;
    let cookie = format!("autoreverse_session={token}");
    for site in ["cross-site", "same-site"] {
        let response = app
            .send(cookie_post("/auth/logout", &cookie, Some(site)))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{site}");
        assert_eq!(
            body_json(response).await["detail"],
            "Anfragen von fremden Seiten sind nicht erlaubt"
        );
    }
    // Still logged in.
    assert_eq!(app.get("/me", Some(&token)).await.status(), StatusCode::OK);
}

#[sqlx::test]
async fn own_pages_and_native_clients_are_not_affected(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = app.token(id).await;
    let cookie = format!("autoreverse_session={token}");
    let response = app
        .send(cookie_post(
            "/favorites/items/999",
            &cookie,
            Some("same-origin"),
        ))
        .await;
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
    // A bearer token is no ambient credential: a page elsewhere cannot attach it.
    let request = Request::builder()
        .method(Method::POST)
        .uri("/auth/logout")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.send(request).await.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn only_admins_change_podcasts_through_the_api(db: PgPool) {
    let app = TestApp::new(db);
    app.user("admin", true).await;
    let id = app.user("oliver", false).await;
    let token = app.token(id).await;
    let body = json!({ "feed_url": "http://127.0.0.1:9/feed.xml" });
    for (method, uri) in [
        (Method::POST, "/podcasts"),
        (Method::POST, "/podcasts/preview"),
        (Method::DELETE, "/podcasts/1"),
        (Method::POST, "/podcasts/1/refresh"),
    ] {
        let response = app
            .call(method.clone(), uri, Some(body.clone()), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
    }
}

#[sqlx::test]
async fn a_feed_url_must_be_http(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("admin", true).await;
    let token = app.token(id).await;
    for url in ["file:///etc/passwd", "kein link", "ftp://x/feed"] {
        let response = app
            .post_json(
                "/podcasts/preview",
                json!({ "feed_url": url }),
                Some(&token),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{url}");
    }
}
