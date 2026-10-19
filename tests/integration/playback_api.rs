//! Streaming, sessions, progress, continue listening, stats and preferences over the API (legacy
//! stream.test.ts, playback.test.ts, progress-continue-api.test.ts, stats-api.test.ts and the
//! preferences half of favorites-bookmarks-prefs.test.ts).

use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{
    Seed, TestApp, body_json, body_text, header_str, seed, seed_progress, seed_track,
};

async fn signed_in(db: PgPool) -> (TestApp, String, i32) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    (app, token, user)
}

async fn request(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: &str,
    range: Option<&str>,
    body: Option<Value>,
) -> Response<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(range) = range {
        builder = builder.header(header::RANGE, range);
    }
    let body = match body {
        Some(json) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    app.send(builder.body(body).unwrap()).await
}

async fn put_json(app: &TestApp, uri: &str, token: &str, body: Value) -> Response<Body> {
    request(app, Method::PUT, uri, token, None, Some(body)).await
}

async fn detail(response: Response<Body>) -> String {
    body_json(response).await["detail"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A 1000-byte "mp3" whose byte `i` is `i % 256`, as a track of a new album.
async fn track_with_file(app: &TestApp) -> (tempfile::TempDir, i32) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.mp3");
    let content: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    std::fs::write(&path, content).unwrap();
    let album = seed(app.db(), Seed::new("album", "X")).await;
    let track = seed_track(app.db(), album, 1, path.to_str().unwrap(), 10.0).await;
    (dir, track)
}

// --- GET /tracks/{id}/stream ----------------------------------------------------------------

#[sqlx::test]
async fn stream_serves_206_with_the_exact_bytes(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (_dir, track) = track_with_file(&app).await;
    let response = request(
        &app,
        Method::GET,
        &format!("/tracks/{track}/stream"),
        &token,
        Some("bytes=100-199"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        header_str(&response, header::CONTENT_RANGE),
        "bytes 100-199/1000"
    );
    assert_eq!(header_str(&response, header::CONTENT_LENGTH), "100");
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "audio/mpeg");
    assert_eq!(header_str(&response, header::ACCEPT_RANGES), "bytes");
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(bytes.len(), 100);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(i, b)| *b == ((100 + i) % 256) as u8)
    );
}

#[sqlx::test]
async fn stream_without_range_sends_the_whole_file(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (_dir, track) = track_with_file(&app).await;
    let response = app
        .get(&format!("/tracks/{track}/stream"), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_LENGTH), "1000");
    assert_eq!(header_str(&response, header::ACCEPT_RANGES), "bytes");
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(bytes.len(), 1000);
}

#[sqlx::test]
async fn stream_404s_for_an_unknown_track(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for uri in ["/tracks/999/stream", "/tracks/abc/stream"] {
        let response = app.get(uri, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(detail(response).await, "Unbekannter Track");
    }
}

#[sqlx::test]
async fn stream_410s_when_the_file_is_gone(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let album = seed(app.db(), Seed::new("album", "X")).await;
    let track = seed_track(app.db(), album, 1, "/nonexistent/x.mp3", 10.0).await;
    let response = app
        .get(&format!("/tracks/{track}/stream"), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::GONE);
    assert_eq!(detail(response).await, "Datei nicht mehr vorhanden");
}

#[sqlx::test]
async fn head_answers_with_headers_only(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (_dir, track) = track_with_file(&app).await;
    let uri = format!("/tracks/{track}/stream");

    let response = request(
        &app,
        Method::HEAD,
        &uri,
        &token,
        Some("bytes=100-199"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        header_str(&response, header::CONTENT_RANGE),
        "bytes 100-199/1000"
    );
    assert_eq!(header_str(&response, header::CONTENT_LENGTH), "100");
    assert!(body_text(response).await.is_empty());

    let response = request(&app, Method::HEAD, &uri, &token, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_LENGTH), "1000");
    assert_eq!(header_str(&response, header::ACCEPT_RANGES), "bytes");
    assert!(body_text(response).await.is_empty());
}

#[sqlx::test]
async fn unsatisfiable_ranges_are_416(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (_dir, track) = track_with_file(&app).await;
    let uri = format!("/tracks/{track}/stream");
    for method in [Method::GET, Method::HEAD] {
        let response = request(&app, method, &uri, &token, Some("bytes=1000-1999"), None).await;
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(header_str(&response, header::CONTENT_RANGE), "bytes */1000");
        assert!(body_text(response).await.is_empty());
    }
}

#[sqlx::test]
async fn stream_accepts_the_session_cookie_and_refuses_strangers(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (_dir, track) = track_with_file(&app).await;
    let uri = format!("/tracks/{track}/stream");
    let response = app
        .get_with_cookie(&uri, &format!("autoreverse_session={token}"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = app.get(&uri, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(detail(response).await, "Kein Token");
}

// --- POST /play/{itemId} --------------------------------------------------------------------

#[sqlx::test]
async fn play_opens_a_session_and_returns_tracks_and_chapters(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let track = seed_track(app.db(), book, 1, "/a", 120.0).await;
    sqlx::query(
        "INSERT INTO chapters (item_id, position, title, start, \"end\") VALUES ($1, 0, 'Eins', 0, 60.5)",
    )
    .bind(book)
    .execute(app.db())
    .await
    .unwrap();

    let response = app
        .post_json(&format!("/play/{book}"), json!({}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = body_json(response).await;
    let session_id = body["session_id"].as_str().unwrap().to_owned();
    assert_eq!(session_id.len(), 36);
    body["session_id"] = json!("…");
    // Byte-for-byte legacy: whole numbers without a fraction.
    assert_eq!(
        body.to_string(),
        json!({
            "session_id": "…",
            "kind": "book",
            "has_cover": false,
            "start_position": 0,
            "tracks": [{"id": track, "position": 1, "title": null, "duration": 120,
                "artist": null, "disc": null}],
            "chapters": [{"title": "Eins", "start": 0, "end": 60.5}]
        })
        .to_string()
    );

    let (owner, item, start): (i32, i32, f64) = sqlx::query_as(
        "SELECT user_id, item_id, start_position FROM playback_sessions WHERE id = $1",
    )
    .bind(&session_id)
    .fetch_one(app.db())
    .await
    .unwrap();
    assert_eq!((owner, item, start), (user, book, 0.0));
}

#[sqlx::test]
async fn play_starts_at_the_saved_position(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    seed_track(app.db(), book, 1, "/a", 120.0).await;
    seed_progress(app.db(), user, book, 42.5, false).await;
    let body = body_json(
        app.post_json(&format!("/play/{book}"), json!({}), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["start_position"], 42.5);
}

#[sqlx::test]
async fn a_finished_item_plays_from_the_start(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let album = seed(app.db(), Seed::new("album", "X")).await;
    seed_track(app.db(), album, 1, "/a", 120.0).await;
    seed_progress(app.db(), user, album, 120.0, true).await;
    let body = body_json(
        app.post_json(&format!("/play/{album}"), json!({}), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["start_position"], 0);
}

#[sqlx::test]
async fn play_409s_for_an_episode_that_is_not_downloaded(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let podcast = seed(app.db(), Seed::new("podcast", "P")).await;
    let episode = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "E")
        },
    )
    .await;
    let response = app
        .post_json(&format!("/play/{episode}"), json!({}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        detail(response).await,
        "Folge ist noch nicht heruntergeladen"
    );
}

#[sqlx::test]
async fn play_takes_the_podcast_cover_for_an_episode(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let podcast = seed(
        app.db(),
        Seed {
            cover_path: Some("/covers/p.jpg"),
            ..Seed::new("podcast", "P")
        },
    )
    .await;
    let episode = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "E")
        },
    )
    .await;
    seed_track(app.db(), episode, 1, "/e.mp3", 60.0).await;
    let body = body_json(
        app.post_json(&format!("/play/{episode}"), json!({}), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["kind"], "episode");
    assert_eq!(body["has_cover"], true);
}

#[sqlx::test]
async fn play_404s_for_an_unknown_item_and_401s_without_a_session(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let response = app.post_json("/play/999", json!({}), Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, "Unbekanntes Item");
    let response = app.post_json("/play/1", json!({}), None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// --- POST /sessions/{id}/close --------------------------------------------------------------

async fn open(app: &TestApp, token: &str, item: i32) -> String {
    let body = body_json(
        app.post_json(&format!("/play/{item}"), json!({}), Some(token))
            .await,
    )
    .await;
    body["session_id"].as_str().unwrap().to_owned()
}

async fn listened(app: &TestApp, user: i32) -> Vec<f64> {
    sqlx::query_scalar("SELECT seconds FROM listen_events WHERE user_id = $1 ORDER BY id")
        .bind(user)
        .fetch_all(app.db())
        .await
        .unwrap()
}

#[sqlx::test]
async fn closing_books_the_distance_travelled_not_the_wall_clock(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let session = open(&app, &token, book).await;
    let response = app
        .post_json(
            &format!("/sessions/{session}/close"),
            json!({"position": 42, "duration": 999}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let closed: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT closed_at FROM playback_sessions WHERE id = $1")
            .bind(&session)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert!(closed.is_some());
    assert_eq!(listened(&app, user).await, [42.0]);
    let saved: f64 =
        sqlx::query_scalar("SELECT position FROM progress WHERE user_id = $1 AND item_id = $2")
            .bind(user)
            .bind(book)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(saved, 42.0);
}

#[sqlx::test]
async fn closing_counts_from_the_start_position_and_never_books_a_rewind(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    seed_progress(app.db(), user, book, 100.0, false).await;

    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 160}),
        Some(&token),
    )
    .await;
    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 20}),
        Some(&token),
    )
    .await;
    assert_eq!(listened(&app, user).await, [60.0]);
    // Closing keeps the finished flag as it was.
    sqlx::query("UPDATE progress SET finished = true WHERE user_id = $1")
        .bind(user)
        .execute(app.db())
        .await
        .unwrap();
    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 30}),
        Some(&token),
    )
    .await;
    let finished: bool =
        sqlx::query_scalar("SELECT finished FROM progress WHERE user_id = $1 AND item_id = $2")
            .bind(user)
            .bind(book)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert!(finished);
}

#[sqlx::test]
async fn a_session_closes_once_and_only_for_its_owner(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let other = app.user("someone", false).await;
    let other_token = app.token(other).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let session = open(&app, &token, book).await;
    let uri = format!("/sessions/{session}/close");

    let response = app
        .post_json(&uri, json!({"position": 5}), Some(&other_token))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, "Unbekannte Sitzung");

    let response = app
        .post_json(&uri, json!({"position": 5}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app
        .post_json(&uri, json!({"position": 5}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = app
        .post_json("/sessions/nope/close", json!({"position": 5}), Some(&token))
        .await;
    assert_eq!(detail(response).await, "Unbekannte Sitzung");
}

#[sqlx::test]
async fn closing_needs_a_numeric_position(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let session = open(&app, &token, book).await;
    let uri = format!("/sessions/{session}/close");
    let response = app.post_json(&uri, json!({}), Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, "position muss eine Zahl sein");
    let response = app.post_raw(&uri, "{nope", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, "Ungültiger JSON-Body");
}

// --- PUT /progress/{itemId} -----------------------------------------------------------------

async fn stored_progress(app: &TestApp, user: i32, item: i32) -> Option<(f64, bool)> {
    sqlx::query_as("SELECT position, finished FROM progress WHERE user_id = $1 AND item_id = $2")
        .bind(user)
        .bind(item)
        .fetch_optional(app.db())
        .await
        .unwrap()
}

#[sqlx::test]
async fn progress_upserts_position_and_finished(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let uri = format!("/progress/{book}");
    let response = put_json(
        &app,
        &uri,
        &token,
        json!({"position": 100, "finished": true}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(stored_progress(&app, user, book).await, Some((100.0, true)));
    put_json(
        &app,
        &uri,
        &token,
        json!({"position": 200, "finished": false}),
    )
    .await;
    assert_eq!(
        stored_progress(&app, user, book).await,
        Some((200.0, false))
    );
    // `finished` defaults to false.
    put_json(
        &app,
        &uri,
        &token,
        json!({"position": 300, "finished": true}),
    )
    .await;
    put_json(&app, &uri, &token, json!({"position": 310.5})).await;
    assert_eq!(
        stored_progress(&app, user, book).await,
        Some((310.5, false))
    );
}

#[sqlx::test]
async fn progress_also_takes_post_for_send_beacon(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let response = app
        .post_json(
            &format!("/progress/{book}"),
            json!({"position": 7}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(stored_progress(&app, user, book).await, Some((7.0, false)));
}

#[sqlx::test]
async fn progress_validates_item_and_position(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let response = put_json(&app, "/progress/999", &token, json!({"position": 1})).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(detail(response).await, "Unbekanntes Item");

    let book = seed(app.db(), Seed::new("book", "X")).await;
    let uri = format!("/progress/{book}");
    for body in [
        json!({}),
        json!({"position": "12"}),
        json!({"position": null}),
    ] {
        let response = put_json(&app, &uri, &token, body.clone()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert_eq!(detail(response).await, "position muss eine Zahl sein");
    }
    let response = put_json(&app, &uri, &token, json!([1, 2])).await;
    assert_eq!(detail(response).await, "Ungültiger JSON-Body");
}

// --- GET /progress/continue -----------------------------------------------------------------

#[sqlx::test]
async fn continue_returns_duration_and_has_cover(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(
        app.db(),
        Seed {
            cover_path: Some("/covers/billion.jpg"),
            ..Seed::new("book", "Eine Billion Dollar")
        },
    )
    .await;
    seed_track(app.db(), book, 1, "/a", 100.0).await;
    seed_track(app.db(), book, 2, "/b", 200.0).await;
    seed_progress(app.db(), user, book, 50.0, false).await;

    let body = body_json(app.get("/progress/continue", Some(&token)).await).await;
    assert_eq!(
        body.to_string(),
        json!({"items": [{
            "id": book, "title": "Eine Billion Dollar", "kind": "book",
            "position": 50, "duration": 300, "has_cover": true
        }]})
        .to_string()
    );
}

#[sqlx::test]
async fn continue_reports_no_cover_and_zero_duration(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "Ohne Bild")).await;
    seed_progress(app.db(), user, book, 5.0, false).await;
    let body = body_json(app.get("/progress/continue", Some(&token)).await).await;
    assert_eq!(body["items"][0]["has_cover"], false);
    assert_eq!(body["items"][0]["duration"], json!(0));
}

#[sqlx::test]
async fn continue_skips_finished_missing_and_foreign_items_newest_first(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let other = app.user("someone", false).await;
    let older = seed(app.db(), Seed::new("book", "Älter")).await;
    let newer = seed(app.db(), Seed::new("album", "Neuer")).await;
    let done = seed(app.db(), Seed::new("book", "Fertig")).await;
    let gone = seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("book", "Weg")
        },
    )
    .await;
    let theirs = seed(app.db(), Seed::new("book", "Fremd")).await;
    seed_progress(app.db(), user, older, 1.0, false).await;
    seed_progress(app.db(), user, done, 1.0, true).await;
    seed_progress(app.db(), user, gone, 1.0, false).await;
    seed_progress(app.db(), other, theirs, 1.0, false).await;
    put_json(
        &app,
        &format!("/progress/{newer}"),
        &token,
        json!({"position": 3}),
    )
    .await;

    let body = body_json(app.get("/progress/continue", Some(&token)).await).await;
    let ids: Vec<i64> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [newer as i64, older as i64]);

    let body = body_json(app.get("/progress/continue?limit=1", Some(&token)).await).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    let response = app.get("/progress/continue?limit=101", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        detail(response).await,
        "limit außerhalb des gültigen Bereichs"
    );
}

// --- GET /me/stats --------------------------------------------------------------------------

async fn listen(app: &TestApp, user: i32, item: i32, seconds: f64, days_ago: i32) {
    sqlx::query(
        "INSERT INTO listen_events (user_id, item_id, seconds, at)
         VALUES ($1, $2, $3, now() - make_interval(days => $4))",
    )
    .bind(user)
    .bind(item)
    .bind(seconds)
    .bind(days_ago)
    .execute(app.db())
    .await
    .unwrap();
}

#[sqlx::test]
async fn stats_sum_per_day_and_over_the_window(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    listen(&app, user, book, 600.0, 0).await;
    listen(&app, user, book, 300.0, 0).await;
    let response = app.get("/me/stats", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let today: String =
        sqlx::query_scalar("SELECT to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD')")
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(
        body.to_string(),
        json!({"total_seconds": 900, "days": {today: 900}}).to_string()
    );
}

#[sqlx::test]
async fn stats_count_only_the_asking_user(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let other = app.user("someone", false).await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    listen(&app, user, book, 100.0, 0).await;
    listen(&app, other, book, 9999.0, 0).await;
    let body = body_json(app.get("/me/stats", Some(&token)).await).await;
    assert_eq!(body["total_seconds"], 100.0);
}

#[sqlx::test]
async fn stats_leave_out_anything_older_than_the_window(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "Der Schwarm")).await;
    listen(&app, user, book, 500.0, 60).await;
    listen(&app, user, book, 42.0, 0).await;
    let body = body_json(app.get("/me/stats?days=7", Some(&token)).await).await;
    assert_eq!(body["total_seconds"], 42.0);
    assert_eq!(body["days"].as_object().unwrap().len(), 1);
    let body = body_json(app.get("/me/stats?days=90", Some(&token)).await).await;
    assert_eq!(body["total_seconds"], 542.0);
}

#[sqlx::test]
async fn stats_are_empty_rather_than_failing(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let body = body_json(app.get("/me/stats", Some(&token)).await).await;
    assert_eq!(
        body.to_string(),
        json!({"total_seconds": 0, "days": {}}).to_string()
    );
}

#[sqlx::test]
async fn stats_refuse_strangers_and_bad_windows(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    assert_eq!(
        app.get("/me/stats", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    for days in ["0", "366", "x"] {
        let response = app
            .get(&format!("/me/stats?days={days}"), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            detail(response).await,
            "days außerhalb des gültigen Bereichs"
        );
    }
}

// --- GET|PUT /me/playback -------------------------------------------------------------------

#[sqlx::test]
async fn playback_preferences_start_at_the_defaults(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let body = body_json(app.get("/me/playback", Some(&token)).await).await;
    assert_eq!(
        body.to_string(),
        r#"{"playback_speed":1,"skip_back":30,"skip_forward":15}"#
    );
}

#[sqlx::test]
async fn put_playback_returns_the_stored_values(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let response = put_json(
        &app,
        "/me/playback",
        &token,
        json!({"playback_speed": 1.25, "skip_back": 30, "skip_forward": 15}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["playback_speed"], 1.25);

    // Fractions of a second are cut off; absent fields go back to their defaults.
    let body = body_json(
        put_json(
            &app,
            "/me/playback",
            &token,
            json!({"playback_speed": 2, "skip_back": 29.9}),
        )
        .await,
    )
    .await;
    assert_eq!(
        body,
        json!({"playback_speed": 2, "skip_back": 29, "skip_forward": 15})
    );
    let body = body_json(app.get("/me/playback", Some(&token)).await).await;
    assert_eq!(body["skip_back"], 29);
}

#[sqlx::test]
async fn put_playback_refuses_values_out_of_range(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for (body, name) in [
        (json!({"playback_speed": 0.25}), "playback_speed"),
        (json!({"playback_speed": "fast"}), "playback_speed"),
        (json!({"skip_back": 0}), "skip_back"),
        (json!({"skip_forward": 301}), "skip_forward"),
        (json!({"skip_forward": null}), "skip_forward"),
    ] {
        let response = put_json(&app, "/me/playback", &token, body.clone()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert_eq!(
            detail(response).await,
            format!("{name} außerhalb des gültigen Bereichs")
        );
    }
}
