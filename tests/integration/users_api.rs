//! `/users*`, `/me/password`, the directory functions, `set-password` and the login throttle
//! around password changes (legacy tests/integration/users.test.ts, tests/unit/auth-directory.test.ts
//! plus the route contracts of users/+server.ts, users/[id]/+server.ts, me/password/+server.ts).

use autoreverse_server::auth::directory::{
    DirectoryError, delete_user, list_users, set_admin, set_password_or_create,
};
use autoreverse_server::auth::passwords::authenticate;
use autoreverse_server::cli;
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{TestApp, body_json};

async fn admin_token(app: &TestApp) -> (i32, String) {
    let id = app.user("oliver", true).await;
    (id, app.token(id).await)
}

async fn token_count(app: &TestApp, user: i32) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM auth_tokens WHERE user_id = $1")
        .bind(user)
        .fetch_one(app.db())
        .await
        .unwrap()
}

async fn token_exists(app: &TestApp, token: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM auth_tokens WHERE value = encode(sha256(convert_to($1, 'UTF8')), 'hex'))")
        .bind(token)
        .fetch_one(app.db())
        .await
        .unwrap()
}

async fn detail(response: axum::http::Response<axum::body::Body>) -> String {
    body_json(response).await["detail"]
        .as_str()
        .unwrap()
        .to_owned()
}

// --- directory --------------------------------------------------------------------------------

#[sqlx::test]
async fn lists_users_case_insensitively_without_password_hashes(db: PgPool) {
    let app = TestApp::new(db);
    app.user("zoe", false).await;
    app.user("Anna", false).await;
    let rows = list_users(app.db()).await.unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Anna", "zoe"]);
}

#[sqlx::test]
async fn refuses_to_demote_the_last_admin(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await; // first user: admin anyway
    assert!(matches!(
        set_admin(app.db(), id, false).await,
        Err(DirectoryError::LastAdmin)
    ));
}

#[sqlx::test]
async fn allows_demotion_when_another_admin_exists(db: PgPool) {
    let app = TestApp::new(db);
    let first = app.user("oliver", false).await;
    let second = app.user("mara", false).await;
    assert!(set_admin(app.db(), second, true).await.unwrap());
    assert!(!set_admin(app.db(), first, false).await.unwrap());
}

#[sqlx::test]
async fn set_admin_on_an_unknown_user_is_not_found(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    assert!(matches!(
        set_admin(app.db(), 999, true).await,
        Err(DirectoryError::NotFound)
    ));
}

#[sqlx::test]
async fn delete_refuses_oneself_and_the_last_admin(db: PgPool) {
    let app = TestApp::new(db);
    let admin = app.user("oliver", true).await;
    assert!(matches!(
        delete_user(app.db(), Some(admin), admin).await,
        Err(DirectoryError::SelfDelete)
    ));
    assert!(matches!(
        delete_user(app.db(), None, admin).await,
        Err(DirectoryError::LastAdmin)
    ));
    assert!(matches!(
        delete_user(app.db(), Some(admin), 999).await,
        Err(DirectoryError::NotFound)
    ));
}

#[sqlx::test]
async fn concurrent_demotions_of_the_last_two_admins_leave_one(db: PgPool) {
    let app = TestApp::new(db);
    let a = app.user("a", true).await;
    let b = app.user("b", true).await;
    let (ra, rb) = tokio::join!(set_admin(app.db(), a, false), set_admin(app.db(), b, false));
    assert_eq!([ra.is_ok(), rb.is_ok()].iter().filter(|ok| **ok).count(), 1);
    let admins: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE is_admin")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(admins, 1);
}

// --- GET /users ---------------------------------------------------------------------------------

#[sqlx::test]
async fn get_users_lists_accounts_with_last_seen(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let mara = app.user("Mara", false).await;
    app.token(mara).await;
    sqlx::query(
        "UPDATE auth_tokens SET last_seen_at = '2026-10-01T12:00:00.123Z' WHERE user_id = $1",
    )
    .bind(mara)
    .execute(app.db())
    .await
    .unwrap();
    // The admin's own token is seen by this very request.
    let response = app.get("/users", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let users = body["users"].as_array().unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0]["name"], "Mara");
    assert_eq!(users[0]["is_admin"], false);
    assert_eq!(users[0]["last_seen_at"], "2026-10-01T12:00:00Z");
    assert_eq!(users[1]["name"], "oliver");
    assert_eq!(users[1]["is_admin"], true);
    let created = users[1]["created_at"].as_str().unwrap();
    assert!(
        created.ends_with('Z') && !created.contains(".000"),
        "{created}"
    );
    assert!(users[0].get("password_hash").is_none());
    let keys: Vec<&String> = users[0].as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 5);
}

#[sqlx::test]
async fn never_seen_users_have_null_last_seen(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    app.user("mara", false).await;
    let body = body_json(app.get("/users", Some(&token)).await).await;
    assert_eq!(body["users"][0]["name"], "mara");
    assert_eq!(body["users"][0]["last_seen_at"], Value::Null);
}

#[sqlx::test]
async fn users_routes_are_for_admins_only(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let mara = app.user("mara", false).await;
    let token = app.token(mara).await;
    for (method, uri, body) in [
        (Method::GET, "/users", None),
        (
            Method::POST,
            "/users",
            Some(json!({"name": "x", "password": "hunter2hunter2"})),
        ),
        (Method::PATCH, "/users/1", Some(json!({"is_admin": true}))),
        (Method::DELETE, "/users/1", None),
    ] {
        let response = app
            .call(method.clone(), uri, body.clone(), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(detail(response).await, "Nur für Verwalter");
        let response = app.call(method.clone(), uri, body, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
    }
    // Mara did not promote herself.
    assert!(!set_admin(app.db(), mara, false).await.unwrap());
}

// --- POST /users --------------------------------------------------------------------------------

#[sqlx::test]
async fn post_users_creates_a_regular_user_by_default(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let response = app
        .post_json(
            "/users",
            json!({"name": "mara", "password": "hunter2hunter2"}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_json(response).await;
    assert_eq!(body["name"], "mara");
    assert_eq!(body["is_admin"], false);
    let id = body["id"].as_i64().unwrap() as i32;
    assert_eq!(
        authenticate(app.db(), "mara", "hunter2hunter2")
            .await
            .unwrap(),
        Some(id)
    );

    let body = body_json(
        app.post_json(
            "/users",
            json!({"name": "ben", "password": "hunter2hunter2", "is_admin": true}),
            Some(&token),
        )
        .await,
    )
    .await;
    assert_eq!(body["is_admin"], true);
}

#[sqlx::test]
async fn post_users_returns_409_on_a_duplicate_name(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let body = json!({"name": "mara", "password": "hunter2hunter2"});
    app.post_json("/users", body.clone(), Some(&token)).await;
    let response = app.post_json("/users", body, Some(&token)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(detail(response).await, "Nutzername ist bereits vergeben");
}

#[sqlx::test]
async fn post_users_validates_name_password_and_role(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let long = "x".repeat(101);
    for (body, expected) in [
        (
            json!({"password": "hunter2hunter2"}),
            "name muss 1–100 Zeichen haben",
        ),
        (
            json!({"name": "", "password": "hunter2hunter2"}),
            "name muss 1–100 Zeichen haben",
        ),
        (
            json!({"name": long, "password": "hunter2hunter2"}),
            "name muss 1–100 Zeichen haben",
        ),
        (
            json!({"name": 7, "password": "hunter2hunter2"}),
            "name muss 1–100 Zeichen haben",
        ),
        (
            json!({"name": "mara", "password": "kurz"}),
            "password muss 8–200 Zeichen haben",
        ),
        (
            json!({"name": "mara", "password": "x".repeat(201)}),
            "password muss 8–200 Zeichen haben",
        ),
        (json!({"name": "mara"}), "password muss 8–200 Zeichen haben"),
        (
            json!({"name": "mara", "password": "hunter2hunter2", "is_admin": "ja"}),
            "is_admin muss true oder false sein",
        ),
    ] {
        let response = app.post_json("/users", body.clone(), Some(&token)).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert_eq!(detail(response).await, expected, "{body}");
    }
    let response = app.post_raw("/users", "{kaputt", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, "Ungültiger JSON-Body");
}

// --- PATCH /users/{id} --------------------------------------------------------------------------

#[sqlx::test]
async fn patch_refuses_to_demote_the_last_admin_with_409(db: PgPool) {
    let app = TestApp::new(db);
    let (id, token) = admin_token(&app).await;
    let response = app
        .call(
            Method::PATCH,
            &format!("/users/{id}"),
            Some(json!({"is_admin": false})),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        detail(response).await,
        "Der letzte Verwalter kann nicht herabgestuft werden"
    );
}

#[sqlx::test]
async fn patch_promotes_and_demotes(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let mara = app.user("mara", false).await;
    let uri = format!("/users/{mara}");
    let response = app
        .call(
            Method::PATCH,
            &uri,
            Some(json!({"is_admin": true})),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({"id": mara, "name": "mara", "is_admin": true})
    );
    let body = body_json(
        app.call(
            Method::PATCH,
            &uri,
            Some(json!({"is_admin": false})),
            Some(&token),
        )
        .await,
    )
    .await;
    assert_eq!(body["is_admin"], false);
}

#[sqlx::test]
async fn patch_with_a_password_ends_all_of_the_users_sessions(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let mara = app.user("mara", false).await;
    let old = app.token(mara).await;
    let response = app
        .call(
            Method::PATCH,
            &format!("/users/{mara}"),
            Some(json!({"password": "neues-passwort"})),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!token_exists(&app, &old).await);
    assert_eq!(
        authenticate(app.db(), "mara", "neues-passwort")
            .await
            .unwrap(),
        Some(mara)
    );
    assert_eq!(
        authenticate(app.db(), "mara", "hunter2hunter2")
            .await
            .unwrap(),
        None
    );
    // The admin's own session is untouched.
    assert!(token_exists(&app, &token).await);
}

#[sqlx::test]
async fn patch_with_ones_own_password_keeps_the_calling_session(db: PgPool) {
    let app = TestApp::new(db);
    let (id, token) = admin_token(&app).await;
    let other = app.token(id).await;
    let response = app
        .call(
            Method::PATCH,
            &format!("/users/{id}"),
            Some(json!({"password": "neues-passwort"})),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(token_exists(&app, &token).await);
    assert!(!token_exists(&app, &other).await);
}

#[sqlx::test]
async fn patch_validates_and_404s(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let mara = app.user("mara", false).await;
    let uri = format!("/users/{mara}");
    for (body, expected) in [
        (json!({}), "is_admin oder password angeben"),
        (
            json!({"is_admin": "true"}),
            "is_admin muss true oder false sein",
        ),
        (
            json!({"password": "kurz"}),
            "password muss 8–200 Zeichen haben",
        ),
        (
            json!({"password": 12345678}),
            "password muss 8–200 Zeichen haben",
        ),
    ] {
        let response = app
            .call(Method::PATCH, &uri, Some(body.clone()), Some(&token))
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert_eq!(detail(response).await, expected, "{body}");
    }
    for uri in ["/users/999", "/users/abc", "/users/99999999999"] {
        let response = app
            .call(
                Method::PATCH,
                uri,
                Some(json!({"is_admin": true})),
                Some(&token),
            )
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(detail(response).await, "Unbekannter Nutzer");
    }
}

#[sqlx::test]
async fn a_refused_demotion_does_not_set_the_password_either(db: PgPool) {
    let app = TestApp::new(db);
    let (id, token) = admin_token(&app).await;
    let response = app
        .call(
            Method::PATCH,
            &format!("/users/{id}"),
            Some(json!({"is_admin": false, "password": "neues-passwort"})),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        authenticate(app.db(), "oliver", "hunter2hunter2")
            .await
            .unwrap(),
        Some(id)
    );
}

// --- DELETE /users/{id} -------------------------------------------------------------------------

#[sqlx::test]
async fn delete_removes_the_user_with_tokens_and_data(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let mara = app.user("mara", false).await;
    let mara_token = app.token(mara).await;
    sqlx::query("INSERT INTO playlists (user_id, name) VALUES ($1, 'Lauf')")
        .bind(mara)
        .execute(app.db())
        .await
        .unwrap();
    let response = app
        .call(
            Method::DELETE,
            &format!("/users/{mara}"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(token_count(&app, mara).await, 0);
    let playlists: i64 = sqlx::query_scalar("SELECT count(*) FROM playlists")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(playlists, 0);
    // Her token no longer opens anything.
    let response = app.get("/me", Some(&mara_token)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    // Gone is gone.
    let response = app
        .call(
            Method::DELETE,
            &format!("/users/{mara}"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, "Unbekannter Nutzer");
}

#[sqlx::test]
async fn delete_refuses_ones_own_account(db: PgPool) {
    let app = TestApp::new(db);
    let (id, token) = admin_token(&app).await;
    let response = app
        .call(Method::DELETE, &format!("/users/{id}"), None, Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        detail(response).await,
        "Das eigene Konto kann nicht gelöscht werden"
    );
    let response = app
        .call(Method::DELETE, "/users/abc", None, Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn an_admin_may_delete_another_admin(db: PgPool) {
    let app = TestApp::new(db);
    let (_, token) = admin_token(&app).await;
    let ben = app.user("ben", true).await;
    let response = app
        .call(Method::DELETE, &format!("/users/{ben}"), None, Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

// --- POST /me/password --------------------------------------------------------------------------

#[sqlx::test]
async fn me_password_changes_it_and_revokes_the_other_tokens(db: PgPool) {
    let app = TestApp::new(db);
    let mara = app.user("mara", false).await;
    let token = app.token(mara).await;
    let other = app.token(mara).await;
    let response = app
        .post_json(
            "/me/password",
            json!({"current_password": "hunter2hunter2", "new_password": "neues-passwort"}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(token_exists(&app, &token).await);
    assert!(!token_exists(&app, &other).await);
    assert_eq!(
        authenticate(app.db(), "mara", "neues-passwort")
            .await
            .unwrap(),
        Some(mara)
    );
    // Logging in with the old password fails, with the new one works.
    let response = app
        .post_json(
            "/auth/login",
            json!({"name": "mara", "password": "hunter2hunter2"}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .post_json(
            "/auth/login",
            json!({"name": "mara", "password": "neues-passwort"}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test]
async fn me_password_refuses_a_wrong_current_password(db: PgPool) {
    let app = TestApp::new(db);
    let mara = app.user("mara", false).await;
    let token = app.token(mara).await;
    let other = app.token(mara).await;
    for current in [json!("falsch"), json!(null), json!(42)] {
        let response = app
            .post_json(
                "/me/password",
                json!({"current_password": current, "new_password": "neues-passwort"}),
                Some(&token),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{current}");
        assert_eq!(detail(response).await, "Aktuelles Passwort ist falsch");
    }
    assert!(token_exists(&app, &other).await);
    assert_eq!(
        authenticate(app.db(), "mara", "hunter2hunter2")
            .await
            .unwrap(),
        Some(mara)
    );
}

#[sqlx::test]
async fn me_password_validates_the_new_one_first(db: PgPool) {
    let app = TestApp::new(db);
    let mara = app.user("mara", false).await;
    let token = app.token(mara).await;
    for new in [json!("kurz"), json!("x".repeat(201)), json!(null)] {
        let response = app
            .post_json(
                "/me/password",
                json!({"current_password": "falsch", "new_password": new}),
                Some(&token),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{new}");
        assert_eq!(
            detail(response).await,
            "new_password muss 8–200 Zeichen haben"
        );
    }
    let response = app.post_raw("/me/password", "nope", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = app
        .post_json(
            "/me/password",
            json!({"current_password": "hunter2hunter2", "new_password": "neues-passwort"}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(detail(response).await, "Kein Token");
}

// --- login throttle around passwords ------------------------------------------------------------

#[sqlx::test]
async fn a_successful_login_clears_the_failures_of_that_name(db: PgPool) {
    let app = TestApp::new(db);
    app.user("mara", false).await;
    let login = |password: &'static str| {
        app.post_json(
            "/auth/login",
            json!({"name": "Mara", "password": password}),
            None,
        )
    };
    for _ in 0..9 {
        assert_eq!(login("falsch").await.status(), StatusCode::UNAUTHORIZED);
    }
    // The tenth attempt is still let through — and it is the right password.
    let response = app
        .post_json(
            "/auth/login",
            json!({"name": "mara", "password": "hunter2hunter2"}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // The slate is clean: another nine failures, and the tenth try is not refused either.
    for _ in 0..9 {
        assert_eq!(login("falsch").await.status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(login("falsch").await.status(), StatusCode::UNAUTHORIZED);
    // Now the eleventh in the window is.
    let response = login("hunter2hunter2").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn a_throttled_name_does_not_lock_out_other_names(db: PgPool) {
    let app = TestApp::new(db);
    app.user("mara", false).await;
    app.user("ben", false).await;
    for _ in 0..11 {
        app.post_json(
            "/auth/login",
            json!({"name": "mara", "password": "x"}),
            None,
        )
        .await;
    }
    let response = app
        .post_json(
            "/auth/login",
            json!({"name": "ben", "password": "hunter2hunter2"}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

// --- set-password -------------------------------------------------------------------------------

#[sqlx::test]
async fn set_password_creates_a_missing_user(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let line = cli::set_password(app.db(), "mara", "hunter2hunter2", false)
        .await
        .unwrap();
    assert_eq!(line, "Nutzer \"mara\" angelegt.");
    let id = authenticate(app.db(), "mara", "hunter2hunter2")
        .await
        .unwrap()
        .unwrap();
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert!(!is_admin);
    let line = cli::set_password(app.db(), "ben", "hunter2hunter2", true)
        .await
        .unwrap();
    assert_eq!(line, "Nutzer \"ben\" angelegt (Verwalter).");
}

#[sqlx::test]
async fn set_password_replaces_the_password_promotes_and_logs_out(db: PgPool) {
    let app = TestApp::new(db);
    app.user("oliver", true).await;
    let mara = app.user("mara", false).await;
    app.token(mara).await;
    let line = cli::set_password(app.db(), "mara", "neues-passwort", true)
        .await
        .unwrap();
    assert_eq!(
        line,
        "Passwort für \"mara\" gesetzt (Verwalter). Alle Sitzungen wurden abgemeldet."
    );
    assert_eq!(
        authenticate(app.db(), "mara", "neues-passwort")
            .await
            .unwrap(),
        Some(mara)
    );
    assert_eq!(token_count(&app, mara).await, 0);
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(mara)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert!(is_admin);
    // Without --admin the role stays as it is (no demotion).
    cli::set_password(app.db(), "mara", "noch-ein-passwort", false)
        .await
        .unwrap();
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(mara)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert!(is_admin);
}

#[sqlx::test]
async fn set_password_refuses_short_passwords(db: PgPool) {
    let app = TestApp::new(db);
    let err = cli::set_password(app.db(), "mara", "kurz", false)
        .await
        .unwrap_err();
    assert_eq!(err, "Das Passwort muss 8–200 Zeichen haben.");
    assert!(list_users(app.db()).await.unwrap().is_empty());
    // The directory function itself.
    assert_eq!(
        set_password_or_create(app.db(), "erste", "hunter2hunter2", false)
            .await
            .unwrap(),
        autoreverse_server::auth::directory::SetPasswordOutcome::Created
    );
}
