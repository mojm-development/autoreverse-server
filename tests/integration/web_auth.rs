//! Web login flow: redirects without a session, cookie after login, logout, page shell
//! (legacy login/logout page servers, hooks, e2e login/logout specs at the HTTP level).

use autoreverse_server::Config;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;

use crate::common::{TestApp, body_text, cookie_pair, header_str};

fn location(response: &axum::http::Response<Body>) -> &str {
    header_str(response, header::LOCATION)
}

#[sqlx::test]
async fn pages_redirect_to_login_without_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for path in ["/", "/library"] {
        let response = app.get(path, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{path}");
        assert_eq!(location(&response), "/login", "{path}");
    }
}

#[sqlx::test]
async fn an_invalid_cookie_counts_as_no_session(db: PgPool) {
    let app = TestApp::new(db);
    let response = app
        .get_with_cookie("/library", "autoreverse_session=bogus")
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/login");
}

#[sqlx::test]
async fn the_login_page_shows_the_pitch_the_counts_and_the_warning(db: PgPool) {
    let app = TestApp::new(db);
    let response = app.get("/login", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("<title>Anmelden · Autoreverse</title>"));
    assert!(html.contains("Ein Server. Alles zum Hören."));
    assert!(html.contains("Diese Verbindung ist unverschlüsselt."));
    assert!(html.contains(r#"aria-label="Eingabe anzeigen""#));
    assert!(html.contains(r#"type="password""#));
    assert!(!html.contains("OpenID"));
    assert!(!html.contains(r#"role="alert""#));
}

#[sqlx::test]
async fn logging_in_sets_the_session_cookie_and_opens_the_library(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;

    let response = app
        .post_form("/login", "name=oliver&password=hunter2hunter2", None)
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/library");
    let set_cookie = header_str(&response, header::SET_COOKIE).to_owned();
    assert!(
        set_cookie.starts_with("autoreverse_session="),
        "{set_cookie}"
    );
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(set_cookie.contains("Path=/"));
    assert!(!set_cookie.contains("Secure"));
    let cookie = cookie_pair(&response);

    // With the cookie, / and /login lead to the library ...
    for path in ["/", "/login"] {
        let response = app.get_with_cookie(path, &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{path}");
        assert_eq!(location(&response), "/library", "{path}");
    }

    // ... which renders inside the shell.
    let response = app.get_with_cookie("/library", &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("<title>Start · Autoreverse</title>"));
    assert!(html.contains(", Oliver</h1>"));
    assert!(html.contains(r#"<nav class="sidebar" style="--a: var(--book)">"#));
    assert!(html.contains(r#"class="home" aria-current="true""#));
    assert!(html.contains(r#"href="/library/albums" aria-current="false""#));
    assert!(html.contains(r#"<span class="avatar">OL</span>"#));
    assert!(html.contains(">Verwalter</span>"));
    assert!(html.contains(r#"aria-label="Abmelden""#));
    assert!(html.contains(r#"aria-label="Thema wechseln""#));
    assert!(html.contains(r#"<symbol"#), "icon sprite is inlined");
}

#[sqlx::test]
async fn the_sidebar_says_nutzer_for_a_non_admin(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let id = app.user("mara", false).await;
    let token = app.token(id).await;
    let html = body_text(
        app.get_with_cookie("/library", &format!("autoreverse_session={token}"))
            .await,
    )
    .await;
    assert!(html.contains(">Nutzer</span>"));
}

#[sqlx::test]
async fn a_failed_login_rerenders_the_form_with_the_reason(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let response = app
        .post_form("/login", "name=oliver&password=wrong", None)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(header_str(&response, header::SET_COOKIE), "");
    let html = body_text(response).await;
    assert!(html.contains(r#"<p role="alert">Anmeldung fehlgeschlagen</p>"#));
}

#[sqlx::test]
async fn an_empty_form_is_a_failed_login_not_an_error(db: PgPool) {
    let app = TestApp::new(db);
    let response = app.post_form("/login", "", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn the_web_login_shares_the_throttle(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    for _ in 0..10 {
        app.post_form("/login", "name=oliver&password=wrong", None)
            .await;
    }
    let response = app
        .post_form("/login", "name=Oliver&password=hunter2hunter2", None)
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let html = body_text(response).await;
    assert!(html.contains("Zu viele Fehlversuche, bitte später erneut versuchen"));
}

#[sqlx::test]
async fn the_cookie_is_secure_behind_an_https_origin(db: PgPool) {
    let config =
        Config::from_lookup(|k| (k == "ORIGIN").then(|| "https://audio.example.com".into()))
            .unwrap();
    let app = TestApp::with_config(db, config);
    app.user("oliver", true).await;
    let request = Request::post("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::ORIGIN, "https://audio.example.com")
        .body(Body::from("name=oliver&password=hunter2hunter2"))
        .unwrap();
    let response = app.send(request).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(header_str(&response, header::SET_COOKIE).contains("Secure"));
}

#[sqlx::test]
async fn logging_out_revokes_the_session(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let login = app
        .post_form("/login", "name=oliver&password=hunter2hunter2", None)
        .await;
    let cookie = cookie_pair(&login);

    let response = app.post_form("/logout", "", Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/login");
    assert!(header_str(&response, header::SET_COOKIE).contains("Max-Age=0"));

    // The token is gone server-side, not just the cookie.
    let response = app.get_with_cookie("/library", &cookie).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/login");
}

#[sqlx::test]
async fn logout_without_a_session_goes_to_login(db: PgPool) {
    let app = TestApp::new(db);
    let response = app.post_form("/logout", "", None).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/login");
}

#[sqlx::test]
async fn cross_site_form_posts_are_refused(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let request = Request::post("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::HOST, "localhost:3000")
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::from("name=oliver&password=hunter2hunter2"))
        .unwrap();
    assert_eq!(app.send(request).await.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn static_files_are_served(db: PgPool) {
    let app = TestApp::new(db);
    for path in [
        "/css/app.css",
        "/css/tokens.css",
        "/js/app.js",
        "/favicon.png",
        "/logo.png",
    ] {
        assert_eq!(app.get(path, None).await.status(), StatusCode::OK, "{path}");
    }
}

#[sqlx::test]
async fn unknown_pages_render_the_error_page_for_browsers(db: PgPool) {
    let app = TestApp::new(db);
    let request = Request::get("/nope")
        .header(header::ACCEPT, "text/html")
        .body(Body::empty())
        .unwrap();
    let response = app.send(request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let html = body_text(response).await;
    assert!(html.contains("Zur Bibliothek"));
    assert!(html.contains("<title>Fehler 404 · Autoreverse</title>"));
}
