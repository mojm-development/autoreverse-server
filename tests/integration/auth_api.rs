//! `POST /auth/login`, `POST /auth/logout`, `GET /me` and `perform_login`
//! (legacy tests/integration/auth.test.ts plus the route contracts).

use autoreverse_server::auth::login::perform_login;
use autoreverse_server::auth::throttle::LoginThrottle;
use axum::http::{StatusCode, header};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::common::{TestApp, body_json, header_str};

// --- perform_login --------------------------------------------------------------------------

#[sqlx::test]
async fn perform_login_returns_a_token_on_correct_credentials(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let (throttle, semaphore) = (LoginThrottle::new(), Semaphore::new(4));
    let token = perform_login(app.db(), &throttle, &semaphore, "oliver", "hunter2hunter2")
        .await
        .unwrap();
    assert!(token.len() > 30);
}

#[sqlx::test]
async fn perform_login_rejects_a_wrong_password_with_401(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let (throttle, semaphore) = (LoginThrottle::new(), Semaphore::new(4));
    let err = perform_login(app.db(), &throttle, &semaphore, "oliver", "wrong")
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    assert_eq!(err.detail, "Anmeldung fehlgeschlagen");
    // The attempt still counts.
    assert_eq!(throttle.size(), 1);
}

#[sqlx::test]
async fn perform_login_throttles_after_ten_attempts_and_always_releases_the_semaphore(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let (throttle, semaphore) = (LoginThrottle::new(), Semaphore::new(4));
    for _ in 0..10 {
        let _ = perform_login(app.db(), &throttle, &semaphore, "oliver", "wrong").await;
    }
    let err = perform_login(app.db(), &throttle, &semaphore, "oliver", "wrong")
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        err.detail,
        "Zu viele Fehlversuche, bitte später erneut versuchen"
    );
    assert!(err.retry_after.is_some_and(|s| s > 0 && s <= 300));
    assert_eq!(semaphore.available_permits(), 4);
}

#[sqlx::test]
async fn perform_login_counts_names_case_insensitively(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let (throttle, semaphore) = (LoginThrottle::new(), Semaphore::new(4));
    for _ in 0..10 {
        let _ = perform_login(app.db(), &throttle, &semaphore, "Oliver", "wrong").await;
    }
    let err = perform_login(app.db(), &throttle, &semaphore, "OLIVER", "hunter2hunter2")
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn perform_login_answers_503_when_the_semaphore_is_exhausted_without_touching_the_throttle(
    db: PgPool,
) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let (throttle, semaphore) = (LoginThrottle::new(), Semaphore::new(1));
    let _held = semaphore.try_acquire().unwrap();
    let err = perform_login(app.db(), &throttle, &semaphore, "oliver", "hunter2hunter2")
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        err.detail,
        "Gerade zu viele gleichzeitige Anmeldeversuche, bitte kurz erneut versuchen"
    );
    assert_eq!(err.retry_after, Some(1));
    assert_eq!(throttle.size(), 0);
}

// --- POST /auth/login -------------------------------------------------------------------------

#[sqlx::test]
async fn login_returns_a_token(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let response = app
        .post_json(
            "/auth/login",
            json!({ "name": "oliver", "password": "hunter2hunter2" }),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let token = body["token"].as_str().unwrap();
    assert!(token.len() > 30);
    assert_eq!(body.as_object().unwrap().len(), 1);

    // The token works as a bearer.
    let me = app.get("/me", Some(token)).await;
    assert_eq!(me.status(), StatusCode::OK);
}

#[sqlx::test]
async fn login_with_a_wrong_password_is_401_with_detail(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let response = app
        .post_json(
            "/auth/login",
            json!({ "name": "oliver", "password": "nope" }),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        header_str(&response, header::CONTENT_TYPE),
        "application/json"
    );
    assert_eq!(
        body_json(response).await,
        json!({ "detail": "Anmeldung fehlgeschlagen" })
    );
}

#[sqlx::test]
async fn login_for_an_unknown_name_is_the_same_401(db: PgPool) {
    let app = TestApp::new(db);
    let response = app
        .post_json(
            "/auth/login",
            json!({ "name": "nobody", "password": "x" }),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        body_json(response).await["detail"],
        "Anmeldung fehlgeschlagen"
    );
}

#[sqlx::test]
async fn login_is_throttled_with_retry_after(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    for _ in 0..10 {
        app.post_json(
            "/auth/login",
            json!({ "name": "oliver", "password": "nope" }),
            None,
        )
        .await;
    }
    let response = app
        .post_json(
            "/auth/login",
            json!({ "name": "oliver", "password": "nope" }),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = header_str(&response, header::RETRY_AFTER).parse().unwrap();
    assert!(retry_after > 0 && retry_after <= 300);
    assert_eq!(
        body_json(response).await["detail"],
        "Zu viele Fehlversuche, bitte später erneut versuchen"
    );
}

#[sqlx::test]
async fn login_answers_503_with_retry_after_when_too_many_hash_at_once(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", false).await;
    let _all = app
        .state
        .login_semaphore
        .clone()
        .try_acquire_many_owned(4)
        .unwrap();
    let response = app
        .post_json(
            "/auth/login",
            json!({ "name": "oliver", "password": "hunter2hunter2" }),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(header_str(&response, header::RETRY_AFTER), "1");
}

#[sqlx::test]
async fn malformed_login_bodies_are_422(db: PgPool) {
    let app = TestApp::new(db);
    for body in ["{not json", "", "{\"name\":\"oliver\"}", "[]"] {
        let response = app.post_raw("/auth/login", body, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body:?}"
        );
        assert_eq!(
            body_json(response).await,
            json!({ "detail": "Ungültiger JSON-Body" })
        );
    }
}

// --- POST /auth/logout ------------------------------------------------------------------------

#[sqlx::test]
async fn logout_revokes_the_bearer_token_and_clears_the_cookie(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = app.token(id).await;
    let other = app.token(id).await;

    let response = app.post_raw("/auth/logout", "", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let set_cookie = header_str(&response, header::SET_COOKIE);
    assert!(
        set_cookie.starts_with("autoreverse_session=;"),
        "{set_cookie}"
    );
    assert!(set_cookie.contains("Max-Age=0"));

    assert_eq!(
        app.get("/me", Some(&token)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // Only this session ends.
    assert_eq!(app.get("/me", Some(&other)).await.status(), StatusCode::OK);
}

#[sqlx::test]
async fn logout_revokes_the_cookie_session(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = app.token(id).await;
    let cookie = format!("autoreverse_session={token}");
    let request = axum::http::Request::post("/auth/logout")
        .header(header::COOKIE, &cookie)
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(app.send(request).await.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        app.get_with_cookie("/me", &cookie).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn logout_without_a_session_is_401(db: PgPool) {
    let app = TestApp::new(db);
    let response = app.post_raw("/auth/logout", "", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await, json!({ "detail": "Kein Token" }));
}

// --- GET /me ----------------------------------------------------------------------------------

#[sqlx::test]
async fn me_returns_id_name_and_role(db: PgPool) {
    let app = TestApp::new(db);
    let admin = app.user("oliver", true).await;
    let user = app.user("mara", false).await;

    let token = app.token(admin).await;
    let body = body_json(app.get("/me", Some(&token)).await).await;
    assert_eq!(
        body,
        json!({ "id": admin, "name": "oliver", "is_admin": true })
    );

    let token = app.token(user).await;
    let body = body_json(app.get("/me", Some(&token)).await).await;
    assert_eq!(
        body,
        json!({ "id": user, "name": "mara", "is_admin": false })
    );
}

#[sqlx::test]
async fn me_accepts_the_session_cookie(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = app.token(id).await;
    let response = app
        .get_with_cookie("/me", &format!("autoreverse_session={token}"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test]
async fn me_without_or_with_an_unknown_token_is_401(db: PgPool) {
    let app = TestApp::new(db);
    for token in [None, Some("not-a-real-token")] {
        let response = app.get("/me", token).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(body_json(response).await, json!({ "detail": "Kein Token" }));
    }
}

#[sqlx::test]
async fn unknown_api_paths_are_404_with_detail(db: PgPool) {
    let app = TestApp::new(db);
    let response = app.get("/does-not-exist", None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(body_json(response).await["detail"].is_string());
}
