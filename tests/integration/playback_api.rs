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
    let user = app.user("oskar", false).await;
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
                "artist": null, "disc": null, "has_lyrics": false,
            "gain_db": null, "album_gain_db": null, "peak": null}],
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
    // A position that is there has to be a number; one that is not there is another matter
    // (see below).
    for body in [json!({"position": "12"}), json!({"position": [1]})] {
        let response = app.post_json(&uri, body, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(detail(response).await, "position muss eine Zahl sein");
    }
    let response = app.post_raw(&uri, "{nope", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, "Ungültiger JSON-Body");
    // None of these closed the session.
    let response = app
        .post_json(&uri, json!({"position": 1}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

async fn progress_rows(app: &TestApp, user: i32) -> Vec<(i32, f64, bool)> {
    sqlx::query_as(
        "SELECT item_id, position, finished FROM progress WHERE user_id = $1 ORDER BY item_id",
    )
    .bind(user)
    .fetch_all(app.db())
    .await
    .unwrap()
}

#[sqlx::test]
async fn closing_without_a_position_books_the_time_and_leaves_the_progress_alone(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let fresh = seed(app.db(), Seed::new("album", "Nie begonnen")).await;
    let begun = seed(app.db(), Seed::new("album", "Mittendrin")).await;
    seed_progress(app.db(), user, begun, 300.0, false).await;
    let before: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT updated_at FROM progress WHERE user_id = $1")
            .bind(user)
            .fetch_one(app.db())
            .await
            .unwrap();

    // `null` and an absent field mean the same.
    for (item, body) in [
        (fresh, json!({"position": null, "listened": 30})),
        (begun, json!({"listened": 45})),
    ] {
        let session = open(&app, &token, item).await;
        let response = app
            .post_json(&format!("/sessions/{session}/close"), body, Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let closed: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT closed_at FROM playback_sessions WHERE id = $1")
                .bind(&session)
                .fetch_one(app.db())
                .await
                .unwrap();
        assert!(closed.is_some());
    }
    assert_eq!(listened(&app, user).await, [30.0, 45.0]);
    // No row for the album never begun, and the other one exactly as it was.
    assert_eq!(progress_rows(&app, user).await, [(begun, 300.0, false)]);
    let after: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT updated_at FROM progress WHERE user_id = $1")
            .bind(user)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(after, before);
    // So "Weiter hören" still shows only what was there before.
    let body = body_json(app.get("/progress/continue", Some(&token)).await).await;
    let ids: Vec<i64> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [i64::from(begun)]);
}

#[sqlx::test]
async fn closing_without_position_and_listened_books_nothing(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let album = seed(app.db(), Seed::new("album", "X")).await;
    seed_progress(app.db(), user, album, 100.0, false).await;
    let session = open(&app, &token, album).await;
    let uri = format!("/sessions/{session}/close");
    let response = app.post_json(&uri, json!({}), Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(listened(&app, user).await.is_empty());
    assert_eq!(progress_rows(&app, user).await, [(album, 100.0, false)]);
    // Closed all the same: a second close finds nothing.
    let response = app.post_json(&uri, json!({}), Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn opening_a_session_writes_no_progress(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let album = seed(app.db(), Seed::new("album", "X")).await;
    seed_track(app.db(), album, 1, "/x/1.mp3", 100.0).await;
    open(&app, &token, album).await;
    assert!(progress_rows(&app, user).await.is_empty());
    assert!(listened(&app, user).await.is_empty());
    let body = body_json(app.get("/progress/continue", Some(&token)).await).await;
    assert_eq!(body["items"], json!([]));
}

#[sqlx::test]
async fn closing_books_what_the_client_measured_whatever_the_position_did(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    seed_progress(app.db(), user, book, 1000.0, false).await;

    // A jump ahead by two chapters: the position moved 5000 s, 30 s were audible.
    let session = open(&app, &token, book).await;
    let response = app
        .post_json(
            &format!("/sessions/{session}/close"),
            json!({"position": 6000, "listened": 30}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    // A jump back, which the distance would book as nothing.
    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 10, "listened": 45.5}),
        Some(&token),
    )
    .await;
    // Nothing audible is no event, not an event of zero seconds.
    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 900, "listened": 0}),
        Some(&token),
    )
    .await;
    assert_eq!(listened(&app, user).await, [30.0, 45.5]);
    assert_eq!(
        stored_progress(&app, user, book).await,
        Some((900.0, false))
    );
}

#[sqlx::test]
async fn closing_cuts_a_measured_time_to_what_the_wall_clock_allows(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;

    // Opened just now: a minute of slack at the fastest speed (4×) is the most there can be.
    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 0, "listened": 1e12}),
        Some(&token),
    )
    .await;
    let booked = listened(&app, user).await;
    assert!((240.0..250.0).contains(&booked[0]), "{booked:?}");

    // Open for an hour: 50 minutes heard are believed, and so are 3 hours of a book at 3×.
    for claimed in [3000.0, 10_800.0] {
        let session = open(&app, &token, book).await;
        sqlx::query(
            "UPDATE playback_sessions SET started_at = now() - interval '1 hour' WHERE id = $1",
        )
        .bind(&session)
        .execute(app.db())
        .await
        .unwrap();
        app.post_json(
            &format!("/sessions/{session}/close"),
            json!({"position": 0, "listened": claimed}),
            Some(&token),
        )
        .await;
    }
    assert_eq!(listened(&app, user).await[1..], [3000.0, 10_800.0]);

    // Negative is nothing.
    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 50, "listened": -20}),
        Some(&token),
    )
    .await;
    assert_eq!(listened(&app, user).await.len(), 3);
}

#[sqlx::test]
async fn closing_without_a_measured_time_stays_as_it_was(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    // `null` is a client that has the field and nothing to put in it.
    let session = open(&app, &token, book).await;
    app.post_json(
        &format!("/sessions/{session}/close"),
        json!({"position": 42, "listened": null}),
        Some(&token),
    )
    .await;
    assert_eq!(listened(&app, user).await, [42.0]);

    // Anything else that is not a number is refused, and the session stays open for a retry.
    let session = open(&app, &token, book).await;
    let uri = format!("/sessions/{session}/close");
    let response = app
        .post_json(
            &uri,
            json!({"position": 50, "listened": "viel"}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, "listened muss eine Zahl sein");
    let response = app
        .post_json(&uri, json!({"position": 50, "listened": 8}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(listened(&app, user).await, [42.0, 8.0]);
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

// --- GET /me/history ------------------------------------------------------------------------

async fn listen_at(app: &TestApp, user: i32, item: i32, seconds: f64, minutes_ago: i32) {
    sqlx::query(
        "INSERT INTO listen_events (user_id, item_id, seconds, at)
         VALUES ($1, $2, $3, date_trunc('second', now()) - make_interval(mins => $4))",
    )
    .bind(user)
    .bind(item)
    .bind(seconds)
    .bind(minutes_ago)
    .execute(app.db())
    .await
    .unwrap();
}

#[sqlx::test]
async fn history_folds_what_follows_one_another_and_leaves_out_what_is_missing(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let other = app.user("anna", false).await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Neu!"),
            cover_path: Some("/c/1.jpg"),
            ..Seed::new("album", "Neu! 75")
        },
    )
    .await;
    let book = seed(app.db(), Seed::new("book", "Momo")).await;
    let gone = seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    // Oldest to newest: album, book, book, (missing), book, album — and someone else's.
    listen_at(&app, user, album, 100.0, 60).await;
    listen_at(&app, user, book, 10.0, 50).await;
    listen_at(&app, user, book, 20.5, 40).await;
    listen_at(&app, user, gone, 999.0, 30).await;
    listen_at(&app, user, book, 30.0, 20).await;
    listen_at(&app, user, album, 5.0, 10).await;
    listen_at(&app, other, book, 777.0, 1).await;

    let response = app.get("/me/history", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let entries = body["entries"].as_array().unwrap();
    let brief: Vec<(i64, f64)> = entries
        .iter()
        .map(|e| {
            (
                e["item"]["id"].as_i64().unwrap(),
                e["seconds"].as_f64().unwrap(),
            )
        })
        .collect();
    // The missing album between the book's sessions does not split them.
    assert_eq!(
        brief,
        [
            (i64::from(album), 5.0),
            (i64::from(book), 60.5),
            (i64::from(album), 100.0)
        ]
    );
    // The newest time of a run, in the server's ISO format; the item as every list has it.
    let newest: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT max(at) FROM listen_events WHERE user_id = $1 AND item_id = $2")
            .bind(user)
            .bind(book)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(
        entries[1]["listened_at"],
        json!(newest.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    );
    assert_eq!(
        entries[0]["item"],
        json!({
            "id": album, "kind": "album", "title": "Neu! 75", "has_cover": true,
            "author": null, "artist": "Neu!", "series": null, "series_index": null,
            "year": null, "missing_since": null, "published_at": null
        })
    );
    assert_eq!(entries[0]["seconds"].to_string(), "5");

    let body = body_json(app.get("/me/history?limit=2", Some(&token)).await).await;
    assert_eq!(body["entries"].as_array().unwrap().len(), 2);
    assert_eq!(body["entries"][1]["item"]["id"], json!(book));
}

#[sqlx::test]
async fn history_is_empty_without_listening_and_checks_its_limit(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let body = body_json(app.get("/me/history", Some(&token)).await).await;
    assert_eq!(body, json!({"entries": []}));
    for bad in ["0", "201", "x"] {
        let response = app
            .get(&format!("/me/history?limit={bad}"), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            detail(response).await,
            "limit außerhalb des gültigen Bereichs"
        );
    }
    let response = app.get("/me/history", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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

// --- POST /me/listens -----------------------------------------------------------------------

fn iso_ago(seconds: i64) -> String {
    autoreverse_server::serialize::to_iso(chrono::Utc::now() - chrono::Duration::seconds(seconds))
}

async fn events(app: &TestApp, user: i32) -> Vec<(i32, f64, Option<String>)> {
    sqlx::query_as(
        "SELECT item_id, seconds, client_key FROM listen_events WHERE user_id = $1 ORDER BY id",
    )
    .bind(user)
    .fetch_all(app.db())
    .await
    .unwrap()
}

#[sqlx::test]
async fn listens_are_booked_for_an_item_or_the_item_of_a_track(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "Offline gehört")).await;
    let album = seed(app.db(), Seed::new("album", "Aus einer Playlist")).await;
    let track = seed_track(app.db(), album, 1, "/a", 200.0).await;

    let response = app
        .post_json(
            "/me/listens",
            json!({"listens": [
                {"key": "a-1", "item_id": book, "seconds": 1800, "started_at": iso_ago(7200)},
                {"key": "a-2", "track_id": track, "seconds": 187.5, "started_at": iso_ago(600)},
                {"key": "a-3", "item_id": album, "track_id": track, "seconds": 60,
                 "started_at": iso_ago(300)},
            ]}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await.to_string(),
        json!({"results": [
            {"key": "a-1", "status": "booked", "seconds": 1800},
            {"key": "a-2", "status": "booked", "seconds": 187.5},
            {"key": "a-3", "status": "booked", "seconds": 60},
        ]})
        .to_string()
    );
    assert_eq!(
        events(&app, user).await,
        [
            (book, 1800.0, Some("a-1".to_owned())),
            (album, 187.5, Some("a-2".to_owned())),
            (album, 60.0, Some("a-3".to_owned())),
        ]
    );
    let body = body_json(app.get("/me/stats", Some(&token)).await).await;
    assert_eq!(body["total_seconds"], json!(2047.5));
}

#[sqlx::test]
async fn a_listen_counts_on_the_day_it_began(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let started = chrono::Utc::now() - chrono::Duration::days(3);
    app.post_json(
        "/me/listens",
        json!({"listens": [{"key": "k", "item_id": book, "seconds": 900,
            "started_at": started.to_rfc3339()}]}),
        Some(&token),
    )
    .await;
    let body = body_json(app.get("/me/stats", Some(&token)).await).await;
    let day = started.format("%Y-%m-%d").to_string();
    assert_eq!(
        body.to_string(),
        json!({"total_seconds": 900, "days": {day: 900}}).to_string()
    );
    // Outside a window that ends before it.
    let body = body_json(app.get("/me/stats?days=2", Some(&token)).await).await;
    assert_eq!(body["total_seconds"], json!(0));
    let _ = user;
}

#[sqlx::test]
async fn a_listen_sent_twice_is_booked_once(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let other = app.user("someone", false).await;
    let other_token = app.token(other).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let entry =
        json!({"key": "3f2c9a", "item_id": book, "seconds": 600, "started_at": iso_ago(3600)});

    // Twice in one request and once more in a retry.
    let body = body_json(
        app.post_json(
            "/me/listens",
            json!({"listens": [entry, entry]}),
            Some(&token),
        )
        .await,
    )
    .await;
    assert_eq!(body["results"][0]["status"], "booked");
    assert_eq!(
        body["results"][1].to_string(),
        json!({"key": "3f2c9a", "status": "duplicate"}).to_string()
    );
    let body = body_json(
        app.post_json("/me/listens", json!({"listens": [entry]}), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["results"][0]["status"], "duplicate");
    assert_eq!(events(&app, user).await.len(), 1);

    // A key is the client's own: somebody else's same key is another listen.
    let body = body_json(
        app.post_json(
            "/me/listens",
            json!({"listens": [entry]}),
            Some(&other_token),
        )
        .await,
    )
    .await;
    assert_eq!(body["results"][0]["status"], "booked");
    assert_eq!(events(&app, other).await.len(), 1);
}

#[sqlx::test]
async fn listens_are_cut_to_what_is_plausible(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let body = body_json(
        app.post_json(
            "/me/listens",
            json!({"listens": [
                // Began ten minutes ago: (600 s + 300 s slack) × 4 at most.
                {"key": "fast", "item_id": book, "seconds": 99_999, "started_at": iso_ago(600)},
                // Began long ago: a day at most.
                {"key": "long", "item_id": book, "seconds": 500_000,
                 "started_at": iso_ago(30 * 86_400)},
                // A clock two minutes ahead is still a clock.
                {"key": "skew", "item_id": book, "seconds": 30, "started_at": iso_ago(-120)},
            ]}),
            Some(&token),
        )
        .await,
    )
    .await;
    let fast = body["results"][0]["seconds"].as_f64().unwrap();
    assert!((3600.0..3620.0).contains(&fast), "{fast}");
    assert_eq!(body["results"][1]["seconds"], json!(86_400));
    assert_eq!(body["results"][2]["seconds"], json!(30));
    // Nothing is dated into the future, or the statistics would hide it.
    let ahead: i64 =
        sqlx::query_scalar("SELECT count(*) FROM listen_events WHERE user_id = $1 AND at > now()")
            .bind(user)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(ahead, 0);
}

#[sqlx::test]
async fn a_bad_listen_is_rejected_on_its_own(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let album = seed(app.db(), Seed::new("album", "Y")).await;
    let track = seed_track(app.db(), album, 1, "/a", 200.0).await;
    let now = iso_ago(60);
    let cases = [
        (
            json!({"item_id": book, "seconds": 5, "started_at": now}),
            "key muss 1–100 Zeichen haben",
        ),
        (
            json!({"key": " ", "item_id": book, "seconds": 5, "started_at": now}),
            "key muss 1–100 Zeichen haben",
        ),
        (
            json!({"key": "x".repeat(101), "item_id": book, "seconds": 5, "started_at": now}),
            "key muss 1–100 Zeichen haben",
        ),
        (
            json!({"key": "a", "item_id": book, "started_at": now}),
            "seconds muss eine positive Zahl sein",
        ),
        (
            json!({"key": "b", "item_id": book, "seconds": 0, "started_at": now}),
            "seconds muss eine positive Zahl sein",
        ),
        (
            json!({"key": "c", "item_id": book, "seconds": "5", "started_at": now}),
            "seconds muss eine positive Zahl sein",
        ),
        (
            json!({"key": "d", "item_id": book, "seconds": 5}),
            "started_at muss ein ISO-8601-Zeitpunkt sein",
        ),
        (
            json!({"key": "e", "item_id": book, "seconds": 5, "started_at": "gestern"}),
            "started_at muss ein ISO-8601-Zeitpunkt sein",
        ),
        (
            json!({"key": "f", "item_id": book, "seconds": 5, "started_at": iso_ago(-3600)}),
            "started_at liegt in der Zukunft",
        ),
        (
            json!({"key": "g", "item_id": book, "seconds": 5, "started_at": iso_ago(366 * 86_400)}),
            "started_at liegt zu weit zurück",
        ),
        (
            json!({"key": "h", "seconds": 5, "started_at": now}),
            "item_id oder track_id fehlt",
        ),
        (
            json!({"key": "i", "item_id": 999_999, "seconds": 5, "started_at": now}),
            "Unbekanntes Item",
        ),
        (
            json!({"key": "j", "track_id": 999_999, "seconds": 5, "started_at": now}),
            "Unbekannter Track",
        ),
        (
            json!({"key": "k", "item_id": book, "track_id": track, "seconds": 5, "started_at": now}),
            "track_id gehört nicht zu item_id",
        ),
        (
            json!({"key": "l", "item_id": "7", "seconds": 5, "started_at": now}),
            "item_id muss eine ganze Zahl sein",
        ),
        (
            json!({"key": "m", "track_id": 1.5, "seconds": 5, "started_at": now}),
            "track_id muss eine ganze Zahl sein",
        ),
        (json!("kein Objekt"), "key muss 1–100 Zeichen haben"),
    ];
    let mut listens: Vec<Value> = cases.iter().map(|(entry, _)| entry.clone()).collect();
    // One good entry among them still gets through.
    listens.push(json!({"key": "gut", "item_id": book, "seconds": 5, "started_at": now}));
    let response = app
        .post_json("/me/listens", json!({"listens": listens}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), cases.len() + 1);
    for ((entry, expected), result) in cases.iter().zip(results) {
        assert_eq!(result["status"], "rejected", "{entry}");
        assert_eq!(result["detail"], *expected, "{entry}");
        assert!(result.get("seconds").is_none());
    }
    assert_eq!(results[0]["key"], Value::Null);
    assert_eq!(results[3]["key"], "a");
    assert_eq!(results[cases.len()]["status"], "booked");
    assert_eq!(
        events(&app, user).await,
        [(book, 5.0, Some("gut".to_owned()))]
    );
}

#[sqlx::test]
async fn listens_refuse_strangers_and_a_body_that_is_no_list(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let response = app
        .post_json("/me/listens", json!({"listens": []}), None)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .post_json("/me/listens", json!({"listens": []}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await.to_string(), r#"{"results":[]}"#);

    for body in [json!({}), json!({"listens": {"key": "a"}})] {
        let response = app.post_json("/me/listens", body, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(detail(response).await, "listens muss eine Liste sein");
    }
    let response = app.post_raw("/me/listens", "[]", Some(&token)).await;
    assert_eq!(detail(response).await, "Ungültiger JSON-Body");

    let many: Vec<Value> = (0..501).map(|_| json!({})).collect();
    let response = app
        .post_json("/me/listens", json!({"listens": many}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(detail(response).await, "Höchstens 500 Einträge je Anfrage");
}
