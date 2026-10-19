//! Settings → Nutzer, Sicherheit and Über (legacy settings/users, settings/security,
//! settings/about), and the promise that no link of the web UI ends in a 404.

use std::collections::{BTreeSet, VecDeque};

use autoreverse_server::auth::passwords::authenticate;
use axum::http::{StatusCode, header};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_text, header_str, seed, seed_track};

async fn cookie(app: &TestApp, name: &str, is_admin: bool) -> (String, i32, String) {
    let id = app.user(name, is_admin).await;
    let token = app.token(id).await;
    (format!("autoreverse_session={token}"), id, token)
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> (StatusCode, String) {
    let response = app.get_with_cookie(uri, cookie).await;
    let status = response.status();
    (status, body_text(response).await)
}

async fn form(app: &TestApp, uri: &str, body: &str, cookie: &str) -> String {
    let response = app.post_form(uri, body, Some(cookie)).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri} {body}");
    body_text(response).await
}

async fn token_exists(app: &TestApp, token: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM auth_tokens WHERE value = encode(sha256(convert_to($1, 'UTF8')), 'hex'))")
        .bind(token)
        .fetch_one(app.db())
        .await
        .unwrap()
}

async fn user_id(app: &TestApp, name: &str) -> Option<i32> {
    sqlx::query_scalar("SELECT id FROM users WHERE name = $1")
        .bind(name)
        .fetch_optional(app.db())
        .await
        .unwrap()
}

// --- access -------------------------------------------------------------------------------------

#[sqlx::test]
async fn the_pages_need_a_session_and_users_needs_an_admin(db: PgPool) {
    let app = TestApp::new(db);
    for uri in ["/settings/users", "/settings/security", "/settings/about"] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(header_str(&response, header::LOCATION), "/login", "{uri}");
    }
    app.user("oliver", true).await;
    let (cookie, _, _) = cookie(&app, "mara", false).await;
    let response = app.get_with_cookie("/settings/users", &cookie).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&response, header::LOCATION), "/library");
    let response = app
        .post_form(
            "/settings/users?/createUser",
            "name=x&password=hunter2hunter2",
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(user_id(&app, "x").await, None);
    for uri in ["/settings/security", "/settings/about"] {
        let (status, _) = page(&app, uri, &cookie).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
    }
}

// --- Nutzer -------------------------------------------------------------------------------------

#[sqlx::test]
async fn the_users_page_lists_accounts_with_role_and_actions(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _, _) = cookie(&app, "oliver", true).await;
    app.user("Mara", false).await;
    let (status, html) = page(&app, "/settings/users", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Nutzer · Autoreverse</title>"));
    assert!(html.contains(r#"<h1>Nutzer <span class="count mono">2</span></h1>"#));
    assert!(html.contains("<summary class=\"outline\">Nutzer anlegen</summary>"));
    assert!(html.contains("<th>Konto</th><th>Rolle</th><th>Zuletzt gesehen</th>"));
    // Sorted by name, case-insensitively; the own row says "du" and has no delete button.
    let mara = html.find("<td>Mara</td>").unwrap();
    let oliver = html
        .find("<td>oliver <span class=\"you mono\">du</span></td>")
        .unwrap();
    assert!(mara < oliver);
    assert_eq!(html.matches(">Löschen</button>").count(), 1);
    assert!(html.contains(">Zu Verwalter machen</button>"));
    assert!(html.contains(">Verwalter entfernen</button>"));
    assert_eq!(
        html.matches("<summary class=\"outline\">Passwort setzen</summary>")
            .count(),
        2
    );
    assert!(html.contains(r#"aria-label="Neues Passwort für Mara""#));
    assert!(html.contains("hx-confirm=\"„Mara“ löschen?"));
    // Mara never logged in; oliver was seen by this very request.
    // Plain text like legacy's toLocaleDateString cell, not monospace (design parity).
    assert!(html.contains("<td>—</td>"));
    assert!(!html.contains(r#"<td class="mono">"#));
}

#[sqlx::test]
async fn creating_a_user_from_the_form(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _, _) = cookie(&app, "oliver", true).await;
    let html = form(
        &app,
        "/settings/users?/createUser",
        "name=Mara&password=hunter2hunter2",
        &cookie,
    )
    .await;
    assert!(html.contains(r#"<p class="success" role="status">„Mara“ angelegt.</p>"#));
    assert!(html.contains("<td>Mara</td>"));
    let mara = user_id(&app, "Mara").await.unwrap();
    assert_eq!(
        authenticate(app.db(), "Mara", "hunter2hunter2")
            .await
            .unwrap(),
        Some(mara)
    );

    let html = form(
        &app,
        "/settings/users?/createUser",
        "name=Ben&password=hunter2hunter2&isAdmin=on",
        &cookie,
    )
    .await;
    assert!(html.contains("„Ben“ angelegt."));
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE name = 'Ben'")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert!(is_admin);

    // Refusals keep the form open with the name.
    let html = form(
        &app,
        "/settings/users?/createUser",
        "name=Mara&password=hunter2hunter2",
        &cookie,
    )
    .await;
    assert!(html.contains(r#"<p class="error" role="alert">Nutzername ist bereits vergeben.</p>"#));
    assert!(html.contains(r#"<details class="create" open>"#));
    assert!(html.contains(r#"<input name="name" value="Mara""#));
    let html = form(
        &app,
        "/settings/users?/createUser",
        "name=Zoe&password=kurz",
        &cookie,
    )
    .await;
    assert!(html.contains("Das Passwort muss mindestens 8 Zeichen haben."));
    let html = form(
        &app,
        "/settings/users?/createUser",
        "name=&password=",
        &cookie,
    )
    .await;
    assert!(html.contains("Name und Passwort (min. 8 Zeichen) erforderlich."));
    let long = "x".repeat(101);
    let html = form(
        &app,
        "/settings/users?/createUser",
        &format!("name={long}&password=hunter2hunter2"),
        &cookie,
    )
    .await;
    assert!(html.contains("Der Name darf höchstens 100 Zeichen haben."));
    assert_eq!(user_id(&app, "Zoe").await, None);
}

#[sqlx::test]
async fn toggling_the_role_and_the_last_admin(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, me, _) = cookie(&app, "oliver", true).await;
    let mara = app.user("Mara", false).await;
    let html = form(
        &app,
        "/settings/users?/toggleAdmin",
        &format!("userId={mara}&isAdmin=true"),
        &cookie,
    )
    .await;
    assert!(html.contains("„Mara“ ist jetzt Verwalter."));
    let html = form(
        &app,
        "/settings/users?/toggleAdmin",
        &format!("userId={mara}&isAdmin=false"),
        &cookie,
    )
    .await;
    assert!(html.contains("„Mara“ ist jetzt Nutzer."));
    let html = form(
        &app,
        "/settings/users?/toggleAdmin",
        &format!("userId={me}&isAdmin=false"),
        &cookie,
    )
    .await;
    assert!(html.contains(
        r#"<p class="error" role="alert">Der letzte Verwalter kann nicht herabgestuft werden.</p>"#
    ));
    let html = form(
        &app,
        "/settings/users?/toggleAdmin",
        "userId=abc&isAdmin=true",
        &cookie,
    )
    .await;
    assert!(html.contains("Unbekannter Nutzer."));
}

#[sqlx::test]
async fn setting_someone_elses_password_logs_them_out(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _, own) = cookie(&app, "oliver", true).await;
    let (_, mara, mara_token) = self::cookie(&app, "Mara", false).await;
    let html = form(
        &app,
        "/settings/users?/setPassword",
        &format!("userId={mara}&password=neues-passwort"),
        &cookie,
    )
    .await;
    assert!(html.contains("Passwort für „Mara“ gesetzt. Andere Sitzungen wurden abgemeldet."));
    assert!(!token_exists(&app, &mara_token).await);
    assert!(token_exists(&app, &own).await);
    assert_eq!(
        authenticate(app.db(), "Mara", "neues-passwort")
            .await
            .unwrap(),
        Some(mara)
    );
    let html = form(
        &app,
        "/settings/users?/setPassword",
        &format!("userId={mara}&password=kurz"),
        &cookie,
    )
    .await;
    assert!(html.contains(
        r#"<p class="error" role="alert">Das Passwort muss mindestens 8 Zeichen haben.</p>"#
    ));
}

#[sqlx::test]
async fn deleting_a_user_but_not_oneself(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, me, _) = cookie(&app, "oliver", true).await;
    let mara = app.user("Mara", false).await;
    let html = form(
        &app,
        "/settings/users?/deleteUser",
        &format!("userId={mara}"),
        &cookie,
    )
    .await;
    assert!(html.contains("„Mara“ gelöscht."));
    assert!(!html.contains("<td>Mara</td>"));
    assert_eq!(user_id(&app, "Mara").await, None);
    let html = form(
        &app,
        "/settings/users?/deleteUser",
        &format!("userId={me}"),
        &cookie,
    )
    .await;
    assert!(html.contains("Das eigene Konto kann nicht gelöscht werden."));
    assert!(user_id(&app, "oliver").await.is_some());
    let html = form(&app, "/settings/users?/deleteUser", "userId=999", &cookie).await;
    assert!(html.contains("Unbekannter Nutzer."));
}

#[sqlx::test]
async fn the_users_forms_are_refused_cross_site(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, _, _) = cookie(&app, "oliver", true).await;
    let request = axum::http::Request::post("/settings/users?/createUser")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::HOST, "localhost:3000")
        .header(header::ORIGIN, "http://evil.example")
        .header(header::COOKIE, &cookie)
        .body(axum::body::Body::from("name=evil&password=hunter2hunter2"))
        .unwrap();
    assert_eq!(app.send(request).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(user_id(&app, "evil").await, None);
}

// --- Sicherheit ---------------------------------------------------------------------------------

#[sqlx::test]
async fn the_security_page_changes_the_own_password(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, me, token) = cookie(&app, "mara", false).await;
    let other = app.token(me).await;
    let (status, html) = page(&app, "/settings/security", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<h1>Sicherheit</h1>"));
    assert!(html.contains(r#"name="currentPassword" type="password""#));
    assert!(html.contains(r#"name="newPassword" type="password" required minlength="8""#));
    assert!(html.contains(">Ändern</button>"));

    let html = form(
        &app,
        "/settings/security",
        "currentPassword=falsch&newPassword=neues-passwort",
        &cookie,
    )
    .await;
    assert!(html.contains(r#"<p class="error" role="alert">Aktuelles Passwort ist falsch.</p>"#));
    let html = form(
        &app,
        "/settings/security",
        "currentPassword=hunter2hunter2&newPassword=kurz",
        &cookie,
    )
    .await;
    assert!(html.contains("Neues Passwort muss mindestens 8 Zeichen haben."));
    assert!(token_exists(&app, &other).await);

    let html = form(
        &app,
        "/settings/security",
        "currentPassword=hunter2hunter2&newPassword=neues-passwort",
        &cookie,
    )
    .await;
    assert!(html.contains(r#"<p class="success" role="status">Passwort geändert.</p>"#));
    assert!(token_exists(&app, &token).await);
    assert!(!token_exists(&app, &other).await);
    assert_eq!(
        authenticate(app.db(), "mara", "neues-passwort")
            .await
            .unwrap(),
        Some(me)
    );
    // Still logged in with the same cookie.
    let (status, _) = page(&app, "/settings/security", &cookie).await;
    assert_eq!(status, StatusCode::OK);
}

// --- Über ---------------------------------------------------------------------------------------

#[sqlx::test]
async fn the_about_page_shows_version_and_server_info(db: PgPool) {
    let app = TestApp::new(db);
    let (admin_cookie, _, _) = cookie(&app, "oliver", true).await;
    let (cookie, _, _) = cookie(&app, "mara", false).await;
    seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    let (status, html) = page(&app, "/settings/about", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Über · Autoreverse</title>"));
    assert!(html.contains("<h1>Über Autoreverse</h1>"));
    assert!(html.contains(&format!(
        r#"<p class="version mono">autoreverse {}</p>"#,
        env!("CARGO_PKG_VERSION")
    )));
    assert!(html.contains(r#"<dt>Hörbücher</dt><dd class="mono">1</dd>"#));
    assert!(!html.contains("<dt>Nutzer</dt>"));
    assert!(!html.contains("<dt>Datenverzeichnis</dt>"));
    let (_, html) = page(&app, "/settings/about", &admin_cookie).await;
    assert!(html.contains(r#"<dt>Nutzer</dt><dd class="mono">2</dd>"#));
    assert!(html.contains("<dt>Datenverzeichnis</dt>"));
}

// --- no dead links ------------------------------------------------------------------------------

/// Every `href="/…"` in a page, with the escaped `&` decoded.
fn internal_links(html: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("href=\"") {
        rest = &rest[start + 6..];
        let end = rest.find('"').unwrap();
        let href = rest[..end].replace("&amp;", "&").replace("&#38;", "&");
        rest = &rest[end..];
        if href.starts_with('/') && !href.starts_with("//") {
            links.push(href);
        }
    }
    links
}

/// Follows every internal link from the start page, the settings overview and each settings
/// section, three levels deep (start → list → detail → its links), and fails on any 404 or 5xx. Redirects count as fine (an admin page
/// a non-admin may not open sends them to /library, which is not a dead end).
async fn crawl(app: &TestApp, cookie: &str) -> BTreeSet<String> {
    let mut queue: VecDeque<(String, u8)> = ["/library", "/settings"]
        .iter()
        .map(|s| (s.to_string(), 0))
        .collect();
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    while let Some((uri, depth)) = queue.pop_front() {
        if !seen.insert(uri.clone()) {
            continue;
        }
        let response = app.get_with_cookie(&uri, cookie).await;
        let status = response.status();
        if status == StatusCode::NOT_FOUND || status.is_server_error() {
            failures.push(format!("{uri}: {status}"));
            continue;
        }
        let is_html = header_str(&response, header::CONTENT_TYPE).starts_with("text/html");
        if is_html && depth < 3 && status == StatusCode::OK {
            let html = body_text(response).await;
            for link in internal_links(&html) {
                if !seen.contains(&link) {
                    queue.push_back((link, depth + 1));
                }
            }
        }
    }
    assert!(failures.is_empty(), "dead links: {failures:#?}");
    seen
}

async fn seed_everything(app: &TestApp, user: i32) {
    let book = seed(
        app.db(),
        Seed {
            author: Some("Frank Schätzing"),
            series: Some("Ozean"),
            ..Seed::new("book", "Der Schwarm")
        },
    )
    .await;
    seed_track(app.db(), book, 1, "/b/1.mp3", 60.0).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Kraftwerk"),
            ..Seed::new("album", "Trans Europa Express")
        },
    )
    .await;
    seed_track(app.db(), album, 1, "/k/1.mp3", 60.0).await;
    let podcast = seed(app.db(), Seed::new("podcast", "Lage der Nation")).await;
    seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Folge 1")
        },
    )
    .await;
    sqlx::query("INSERT INTO playlists (user_id, name) VALUES ($1, 'Lauf')")
        .bind(user)
        .execute(app.db())
        .await
        .unwrap();
}

#[sqlx::test]
async fn no_link_of_the_web_ui_leads_to_a_404_for_an_admin(db: PgPool) {
    let app = TestApp::new(db);
    let (cookie, me, _) = cookie(&app, "oliver", true).await;
    seed_everything(&app, me).await;
    let seen = crawl(&app, &cookie).await;
    // Every sidebar entry and every settings section was reached.
    for uri in [
        "/library/albums",
        "/library/artists",
        "/library/playlists",
        "/library/favorites",
        "/library/podcasts",
        "/library/podcasts?filter=new",
        "/library/books",
        "/library/series",
        "/library/bookmarks",
        "/library/search",
        "/settings/libraries",
        "/settings/scan",
        "/settings/users",
        "/settings/playback",
        "/settings/podcasts",
        "/settings/security",
        "/settings/about",
    ] {
        assert!(seen.contains(uri), "{uri} not linked");
    }
}

#[sqlx::test]
async fn no_link_of_the_web_ui_leads_to_a_404_for_a_regular_user(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let (cookie, me, _) = cookie(&app, "mara", false).await;
    seed_everything(&app, me).await;
    let seen = crawl(&app, &cookie).await;
    for uri in [
        "/settings/playback",
        "/settings/security",
        "/settings/about",
    ] {
        assert!(seen.contains(uri), "{uri} not linked");
    }
    // Admin sections are not even offered.
    assert!(!seen.contains("/settings/users"));
}
