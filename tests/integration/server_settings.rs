//! Einstellungen → Server and `GET`/`PATCH /server/settings` (new in Rust): what used to be
//! environment variables, changed by admins at runtime.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

use autoreverse_server::settings;

use crate::common::{TestApp, body_json, body_text};

async fn admin_and_user(app: &TestApp) -> (String, String) {
    let admin = app.user("admin", true).await;
    let user = app.user("mara", false).await;
    (app.token(admin).await, app.token(user).await)
}

#[sqlx::test]
async fn only_admins_read_and_change_the_settings(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, user) = admin_and_user(&app).await;

    let body = body_json(app.get("/server/settings", Some(&admin)).await).await;
    assert_eq!(body["scan_hours"], 1.0);
    assert_eq!(body["artist_info_days"], 30.0);
    assert_eq!(body["radio"], true);
    assert_eq!(body["ai"]["provider"], Value::Null);
    assert_eq!(body["ai"]["has_api_key"], false);
    assert_eq!(body["ai"]["enabled"], false);
    assert_eq!(body["ai"]["providers"], json!(["anthropic", "ollama"]));

    for (method, body) in [
        (Method::GET, None),
        (Method::PATCH, Some(json!({"radio": false}))),
    ] {
        let response = app
            .call(
                method.clone(),
                "/server/settings",
                body.clone(),
                Some(&user),
            )
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method}");
        let response = app
            .call(method.clone(), "/server/settings", body, None)
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{method}");
    }
    assert!(app.state.settings.get().radio);
}

#[sqlx::test]
async fn a_change_is_stored_in_effect_at_once_and_the_key_never_leaves(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, user) = admin_and_user(&app).await;

    let response = app
        .call(
            Method::PATCH,
            "/server/settings",
            Some(json!({"radio": false, "scan_hours": 6,
                        "ai": {"provider": "anthropic", "model": "claude-sonnet-5-5", "api_key": "geheim"}})),
            Some(&admin),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(!text.contains("geheim"), "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["radio"], false);
    assert_eq!(body["scan_hours"], 6.0);
    assert_eq!(body["ai"]["has_api_key"], true);
    assert_eq!(body["ai"]["enabled"], true);

    // In effect: radio is gone for everybody, and the server says so.
    let stations = app.get("/radio/stations", Some(&user)).await;
    assert_eq!(stations.status(), StatusCode::NOT_FOUND);
    let info = body_json(app.get("/server/info", Some(&user)).await).await;
    assert_eq!(info["radio"], false);
    assert_eq!(info["radio_directory"], false);
    let ai = body_json(app.get("/ai/status", Some(&user)).await).await;
    assert_eq!(ai["enabled"], true);
    assert_eq!(ai["model"], "claude-sonnet-5-5");

    // Stored: a restart reads the same, and the environment no longer counts.
    let loaded = settings::load(app.db(), |key| {
        (key == "AUTOREVERSE_RADIO").then(|| "1".to_owned())
    })
    .await
    .unwrap();
    assert!(!loaded.radio);
    assert_eq!(loaded.ai.api_key.as_deref(), Some("geheim"));

    // The key stays when not named, and goes with null.
    let body = body_json(
        app.call(
            Method::PATCH,
            "/server/settings",
            Some(json!({"ai": {"provider": null, "api_key": null}})),
            Some(&admin),
        )
        .await,
    )
    .await;
    assert_eq!(body["ai"]["has_api_key"], false);
    assert_eq!(body["ai"]["enabled"], false);
}

#[sqlx::test]
async fn a_change_that_does_not_hold_is_refused_whole(db: PgPool) {
    let app = TestApp::new(db);
    let (admin, _) = admin_and_user(&app).await;
    for (body, detail) in [
        (json!({}), "Keine bekannte Einstellung angegeben"),
        (
            json!({"scan_hours": -1, "radio": false}),
            "Das Scan-Intervall muss zwischen 0 und 168 Stunden liegen",
        ),
        (
            json!({"radio": false, "ai": {"provider": "anthropic"}}),
            "Für Anthropic fehlt der API-Schlüssel",
        ),
    ] {
        let response = app
            .call(Method::PATCH, "/server/settings", Some(body), Some(&admin))
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body_json(response).await["detail"], detail);
    }
    assert!(app.state.settings.get().radio);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM server_settings")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[sqlx::test]
async fn the_first_start_takes_the_former_variables_over_once(db: PgPool) {
    let env = |key: &str| match key {
        "AUTOREVERSE_SCAN_HOURS" => Some("0".to_owned()),
        "AUTOREVERSE_DEVICES" => Some("0".to_owned()),
        "AUTOREVERSE_AI_PROVIDER" => Some("ollama".to_owned()),
        "AUTOREVERSE_AI_MODEL" => Some("llama4".to_owned()),
        _ => None,
    };
    let first = settings::load(&db, env).await.unwrap();
    assert_eq!(first.scan_hours, 0.0);
    assert!(!first.devices);
    assert_eq!(first.ai.model.as_deref(), Some("llama4"));
    // Later starts read the table, whatever the environment says now.
    let later = settings::load(&db, |_| None).await.unwrap();
    assert_eq!(later, first);
}

#[sqlx::test]
async fn the_web_page_shows_and_saves_the_same(db: PgPool) {
    let app = TestApp::new(db);
    let admin = app.user("admin", true).await;
    let cookie = format!("autoreverse_session={}", app.token(admin).await);

    let html = body_text(app.get_with_cookie("/settings/server", &cookie).await).await;
    assert!(html.contains("<h1>Server</h1>"));
    assert!(html.contains(r#"name="scanHours" min="0" max="168" step="any" value="1""#));
    assert!(html.contains(r#"name="radio" checked"#));

    // Unticked boxes are absent from a form: radio goes off, devices stay on.
    let form = "scanHours=2&podcastHours=1&artistInfo=on&artistInfoDays=7&loudness=on&loudnessThreads=0\
                &devices=on&aiProvider=ollama&aiModel=llama4&aiUrl=&aiApiKey=";
    let response = app.post_form("/settings/server", form, Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("Gespeichert"));
    let saved = app.state.settings.get();
    assert_eq!(saved.scan_hours, 2.0);
    assert_eq!(saved.artist_info_days, 7.0);
    assert!(!saved.radio && !saved.radio_directory && saved.devices);
    assert!(saved.ai().is_some());

    let broken =
        "scanHours=2&podcastHours=1&artistInfoDays=7&loudnessThreads=0&aiProvider=anthropic";
    let html = body_text(
        app.post_form("/settings/server", broken, Some(&cookie))
            .await,
    )
    .await;
    assert!(
        html.contains("Für Anthropic fehlt der API-Schlüssel"),
        "{html}"
    );
    assert_eq!(app.state.settings.get(), saved);
}
