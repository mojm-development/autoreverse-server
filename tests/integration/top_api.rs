//! Listening time per track (`POST /sessions/{id}/close` with `tracks`, `POST /me/listens`) and
//! what is read out of it: `GET /me/top`, `/me/on-repeat`, `/me/repeat-rewind`, `/me/review`,
//! and that the older readers of the ledger count a session of several tracks once.

use axum::http::StatusCode;
use chrono::{Datelike, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_json, body_text, seed, seed_progress, seed_track};

async fn signed_in(db: PgPool) -> (TestApp, String, i32) {
    let app = TestApp::new(db);
    let user = app.user("oskar", false).await;
    let token = app.token(user).await;
    (app, token, user)
}

async fn get(app: &TestApp, token: &str, uri: &str) -> Value {
    let response = app.get(uri, Some(token)).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_json(response).await
}

/// An album with `tracks` titled tracks of 200 s; returns the album and its track ids.
async fn album(
    app: &TestApp,
    title: &str,
    artist: &str,
    genre: Option<&str>,
    tracks: &[&str],
) -> (i32, Vec<i32>) {
    let id = seed(
        app.db(),
        Seed {
            artist: Some(artist),
            ..Seed::new("album", title)
        },
    )
    .await;
    sqlx::query("UPDATE items SET genre = $2 WHERE id = $1")
        .bind(id)
        .bind(genre)
        .execute(app.db())
        .await
        .unwrap();
    let mut ids = Vec::new();
    for (index, name) in tracks.iter().enumerate() {
        let track = seed_track(
            app.db(),
            id,
            index as i32 + 1,
            &format!("/m/{title}/{name}.mp3"),
            200.0,
        )
        .await;
        sqlx::query("UPDATE tracks SET title = $2 WHERE id = $1")
            .bind(track)
            .bind(name)
            .execute(app.db())
            .await
            .unwrap();
        ids.push(track);
    }
    (id, ids)
}

/// One row of the ledger, `ago` before now (a Postgres interval).
async fn listen(app: &TestApp, user: i32, item: i32, track: Option<i32>, seconds: f64, ago: &str) {
    sqlx::query(
        "INSERT INTO listen_events (user_id, item_id, track_id, seconds, at)
         VALUES ($1, $2, $3, $4, now() - $5::text::interval)",
    )
    .bind(user)
    .bind(item)
    .bind(track)
    .bind(seconds)
    .bind(ago)
    .execute(app.db())
    .await
    .unwrap();
}

/// One row at a fixed moment (`at` as an ISO timestamp).
async fn listen_at(
    app: &TestApp,
    user: i32,
    item: i32,
    track: Option<i32>,
    seconds: f64,
    at: &str,
) {
    sqlx::query(
        "INSERT INTO listen_events (user_id, item_id, track_id, seconds, at)
         VALUES ($1, $2, $3, $4, $5::text::timestamptz)",
    )
    .bind(user)
    .bind(item)
    .bind(track)
    .bind(seconds)
    .bind(at)
    .execute(app.db())
    .await
    .unwrap();
}

async fn open(app: &TestApp, token: &str, item: i32) -> String {
    let body = body_json(
        app.post_json(&format!("/play/{item}"), json!({}), Some(token))
            .await,
    )
    .await;
    body["session_id"].as_str().unwrap().to_owned()
}

/// A session of `item`, open for an hour, closed with `body`.
async fn close(app: &TestApp, token: &str, item: i32, body: Value) -> StatusCode {
    let session = open(app, token, item).await;
    sqlx::query(
        "UPDATE playback_sessions SET started_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(&session)
    .execute(app.db())
    .await
    .unwrap();
    app.post_json(&format!("/sessions/{session}/close"), body, Some(token))
        .await
        .status()
}

/// The ledger as `(track_id, seconds)` in the order it was written.
async fn ledger(app: &TestApp, user: i32) -> Vec<(Option<i32>, f64)> {
    sqlx::query_as("SELECT track_id, seconds FROM listen_events WHERE user_id = $1 ORDER BY id")
        .bind(user)
        .fetch_all(app.db())
        .await
        .unwrap()
}

// --- booking per track ----------------------------------------------------------------------

#[sqlx::test]
async fn closing_with_tracks_books_a_row_per_track(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (one, tracks) = album(&app, "Eins", "Band", None, &["a", "b", "c"]).await;
    let (_, foreign) = album(&app, "Zwei", "Band", None, &["x"]).await;

    let status = close(
        &app,
        &token,
        one,
        json!({"position": 500, "tracks": [
            {"track_id": tracks[0], "seconds": 200},
            {"track_id": tracks[1], "seconds": 150.5},
            // Of another album, unknown, and three that are no share at all: all dropped.
            {"track_id": foreign[0], "seconds": 99},
            {"track_id": 987654, "seconds": 99},
            {"track_id": tracks[2], "seconds": 0},
            {"track_id": "x", "seconds": 5},
            {"seconds": 5},
            // The same track again is added to its row.
            {"track_id": tracks[0], "seconds": 20},
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        ledger(&app, user).await,
        [(Some(tracks[0]), 220.0), (Some(tracks[1]), 150.5)]
    );
    // The rows of one session share their moment.
    let moments: i64 = sqlx::query_scalar("SELECT count(DISTINCT at) FROM listen_events")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(moments, 1);
    // The position is saved as always.
    let position: f64 = sqlx::query_scalar("SELECT position FROM progress WHERE item_id = $1")
        .bind(one)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(position, 500.0);
}

#[sqlx::test]
async fn tracks_and_listened_together_follow_one_rule(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (one, tracks) = album(&app, "Eins", "Band", None, &["a", "b"]).await;
    let shares = json!([
        {"track_id": tracks[0], "seconds": 100},
        {"track_id": tracks[1], "seconds": 50},
    ]);

    // `listened` above the sum: the rest is booked without a track.
    close(
        &app,
        &token,
        one,
        json!({"listened": 180, "tracks": shares}),
    )
    .await;
    assert_eq!(
        ledger(&app, user).await,
        [
            (Some(tracks[0]), 100.0),
            (Some(tracks[1]), 50.0),
            (None, 30.0)
        ]
    );
    // `listened` below the sum: the sum counts.
    close(&app, &token, one, json!({"listened": 10, "tracks": shares})).await;
    assert_eq!(
        ledger(&app, user).await[3..],
        [(Some(tracks[0]), 100.0), (Some(tracks[1]), 50.0)]
    );
    // Nothing usable in `tracks`: as if it were not there.
    close(
        &app,
        &token,
        one,
        json!({"listened": 77, "tracks": [{"track_id": 987654, "seconds": 5}]}),
    )
    .await;
    close(&app, &token, one, json!({"listened": 78, "tracks": []})).await;
    close(&app, &token, one, json!({"listened": 79, "tracks": null})).await;
    assert_eq!(
        ledger(&app, user).await[5..],
        [(None, 77.0), (None, 78.0), (None, 79.0)]
    );
    // More than can have been heard in an hour and a minute at four times the speed is cut,
    // every share by the same factor, and no rest is left.
    close(
        &app,
        &token,
        one,
        json!({"listened": 1e9, "tracks": [
            {"track_id": tracks[0], "seconds": 30_000},
            {"track_id": tracks[1], "seconds": 10_000},
        ]}),
    )
    .await;
    let cut = &ledger(&app, user).await[8..];
    assert_eq!(cut.len(), 2);
    let total = cut[0].1 + cut[1].1;
    assert!((14_640.0..14_700.0).contains(&total), "{total}");
    assert!((cut[0].1 / cut[1].1 - 3.0).abs() < 1e-9);
}

#[sqlx::test]
async fn tracks_must_be_a_list(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (one, _) = album(&app, "Eins", "Band", None, &["a"]).await;
    let session = open(&app, &token, one).await;
    for bad in [json!("viele"), json!({"track_id": 1}), json!(5)] {
        let response = app
            .post_json(
                &format!("/sessions/{session}/close"),
                json!({"listened": 5, "tracks": bad}),
                Some(&token),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body_json(response).await,
            json!({"detail": "tracks muss eine Liste sein"})
        );
    }
    // The refused requests closed nothing.
    assert!(ledger(&app, user).await.is_empty());
    let response = app
        .post_json(
            &format!("/sessions/{session}/close"),
            json!({"listened": 5}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn a_reported_listen_keeps_its_track(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (one, tracks) = album(&app, "Eins", "Band", None, &["a"]).await;
    let started = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    let response = app
        .post_json(
            "/me/listens",
            json!({"listens": [
                {"key": "k1", "track_id": tracks[0], "seconds": 120, "started_at": started},
                {"key": "k2", "item_id": one, "seconds": 60, "started_at": started},
            ]}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        ledger(&app, user).await,
        [(Some(tracks[0]), 120.0), (None, 60.0)]
    );
}

#[sqlx::test]
async fn a_session_of_three_tracks_is_counted_once_by_the_older_readers(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (one, tracks) = album(&app, "Eins", "Band", None, &["a", "b", "c"]).await;
    let (two, _) = album(&app, "Zwei", "Band", None, &["x"]).await;
    close(&app, &token, two, json!({"listened": 40})).await;
    close(
        &app,
        &token,
        one,
        json!({"listened": 400, "tracks": [
            {"track_id": tracks[0], "seconds": 200},
            {"track_id": tracks[1], "seconds": 100},
            {"track_id": tracks[2], "seconds": 60},
        ]}),
    )
    .await;

    // The history folds the session's rows into one entry with all of its seconds …
    let history = get(&app, &token, "/me/history").await;
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["item"]["id"], one);
    assert_eq!(entries[0]["seconds"], 400);
    assert_eq!(entries[1]["seconds"], 40);
    // … and the statistics add them up: nothing is there twice.
    let stats = get(&app, &token, "/me/stats").await;
    assert_eq!(stats["total_seconds"], 440);
    // The album was played once, each of its tracks once.
    let albums = get(&app, &token, "/me/top?kind=albums").await;
    assert_eq!(albums["entries"][0]["item"]["id"], one);
    assert_eq!(albums["entries"][0]["seconds"], 400);
    assert_eq!(albums["entries"][0]["plays"], 1);
    let top = get(&app, &token, "/me/top?kind=tracks").await;
    let plays: Vec<i64> = top["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["plays"].as_i64().unwrap())
        .collect();
    assert_eq!(plays, [1, 1, 1]);
    // The feed's and the track radio's taste read sums per item; both still answer.
    assert_eq!(
        app.get("/ai/feed", Some(&token)).await.status(),
        StatusCode::OK
    );
    let radio = app
        .get(&format!("/radio?item_id={two}"), Some(&token))
        .await;
    assert_eq!(radio.status(), StatusCode::OK);
}

// --- GET /me/top ----------------------------------------------------------------------------

/// Two albums and a book with listening spread over the last year.
async fn heard(app: &TestApp, user: i32) -> (i32, Vec<i32>, i32, Vec<i32>) {
    let (rock, rock_tracks) =
        album(app, "Rockplatte", "Die Band", Some("Rock"), &["Auf", "Ab"]).await;
    let (jazz, jazz_tracks) = album(app, "Jazzplatte", "Das Trio", Some("Jazz"), &["Blau"]).await;
    let book = seed(app.db(), Seed::new("book", "Ein Buch")).await;
    // Rock: 3 plays of "Auf" this week, one of "Ab" three weeks ago, and old untracked time.
    listen(app, user, rock, Some(rock_tracks[0]), 200.0, "1 day").await;
    listen(app, user, rock, Some(rock_tracks[0]), 200.0, "2 days").await;
    listen(app, user, rock, Some(rock_tracks[0]), 10.0, "3 days").await;
    listen(app, user, rock, Some(rock_tracks[1]), 150.0, "20 days").await;
    listen(app, user, rock, None, 1000.0, "200 days").await;
    // Jazz: one long listen two months ago.
    listen(app, user, jazz, Some(jazz_tracks[0]), 900.0, "60 days").await;
    // A book is no music.
    listen(app, user, book, None, 5000.0, "1 day").await;
    (rock, rock_tracks, jazz, jazz_tracks)
}

#[sqlx::test]
async fn top_tracks_have_seconds_and_plays_and_the_track_summary(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (rock, rock_tracks, jazz, jazz_tracks) = heard(&app, user).await;

    let week = get(&app, &token, "/me/top?kind=tracks&period=week").await;
    assert_eq!(week["kind"], "tracks");
    assert_eq!(
        week["entries"],
        json!([{
            "track": {
                "id": rock_tracks[0], "title": "Auf", "artist": null, "duration": 200,
                "item_id": rock, "item_title": "Rockplatte", "item_kind": "album",
                "has_lyrics": false,
            "gain_db": null, "album_gain_db": null, "peak": null
            },
            // The ten seconds count as time, not as a play.
            "seconds": 410,
            "plays": 2
        }])
    );
    // The period is told back: seven days up to now.
    let from = chrono::DateTime::parse_from_rfc3339(week["from"].as_str().unwrap()).unwrap();
    let to = chrono::DateTime::parse_from_rfc3339(week["to"].as_str().unwrap()).unwrap();
    assert_eq!((to - from).num_days(), 7);
    assert!((Utc::now() - to.with_timezone(&Utc)).num_seconds() < 60);

    // The default is the last 30 days; longest first.
    let month = get(&app, &token, "/me/top?kind=tracks").await;
    let ids = |body: &Value| -> Vec<i64> {
        body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["track"]["id"].as_i64().unwrap())
            .collect()
    };
    assert_eq!(ids(&month), [rock_tracks[0] as i64, rock_tracks[1] as i64]);
    let all = get(&app, &token, "/me/top?kind=tracks&period=all").await;
    assert_eq!(all["from"], Value::Null);
    assert_eq!(
        ids(&all),
        [
            jazz_tracks[0] as i64,
            rock_tracks[0] as i64,
            rock_tracks[1] as i64
        ]
    );
    let one = get(&app, &token, "/me/top?kind=tracks&period=all&limit=1").await;
    assert_eq!(ids(&one), [jazz_tracks[0] as i64]);

    // A missing album's tracks are left out.
    sqlx::query("UPDATE items SET missing_since = now() WHERE id = $1")
        .bind(jazz)
        .execute(app.db())
        .await
        .unwrap();
    let all = get(&app, &token, "/me/top?kind=tracks&period=all").await;
    assert_eq!(ids(&all), [rock_tracks[0] as i64, rock_tracks[1] as i64]);
}

#[sqlx::test]
async fn top_albums_artists_and_genres_count_untracked_time_too(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (rock, rock_tracks, jazz, _) = heard(&app, user).await;
    // A guest on the rock album: the track's own artist counts for that track.
    sqlx::query("UPDATE tracks SET artist = 'Der Gast' WHERE id = $1")
        .bind(rock_tracks[1])
        .execute(app.db())
        .await
        .unwrap();
    // The jazz album has a cover, so its artist has a picture.
    sqlx::query("UPDATE items SET cover_path = '/covers/1.jpg' WHERE id = $1")
        .bind(jazz)
        .execute(app.db())
        .await
        .unwrap();

    let albums = get(&app, &token, "/me/top?kind=albums&period=all").await;
    assert_eq!(
        albums["entries"],
        json!([
            {
                "item": {
                    "id": rock, "kind": "album", "title": "Rockplatte", "has_cover": false,
                    "author": null, "artist": "Die Band", "series": null, "series_index": null,
                    "year": null, "missing_since": null, "published_at": null
                },
                "seconds": 1560,
                "plays": 4
            },
            {
                "item": {
                    "id": jazz, "kind": "album", "title": "Jazzplatte", "has_cover": true,
                    "author": null, "artist": "Das Trio", "series": null, "series_index": null,
                    "year": null, "missing_since": null, "published_at": null
                },
                "seconds": 900,
                "plays": 1
            },
        ])
    );
    let artists = get(&app, &token, "/me/top?kind=artists&period=all").await;
    assert_eq!(
        artists["entries"],
        json!([
            {"artist": "Die Band", "seconds": 1410, "plays": 3, "has_image": false},
            {"artist": "Das Trio", "seconds": 900, "plays": 1, "has_image": true},
            {"artist": "Der Gast", "seconds": 150, "plays": 1, "has_image": false},
        ])
    );
    let genres = get(&app, &token, "/me/top?kind=genres&period=all").await;
    assert_eq!(
        genres["entries"],
        json!([
            {"genre": "Rock", "seconds": 1560},
            {"genre": "Jazz", "seconds": 900},
        ])
    );
    // Only this week: the jazz and the old rock time are outside.
    let genres = get(&app, &token, "/me/top?kind=genres&period=week").await;
    assert_eq!(
        genres["entries"],
        json!([{"genre": "Rock", "seconds": 410}])
    );
}

#[sqlx::test]
async fn a_calendar_year_goes_before_the_period(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (rock, tracks) = album(&app, "Rockplatte", "Die Band", Some("Rock"), &["Auf"]).await;
    listen_at(
        &app,
        user,
        rock,
        Some(tracks[0]),
        100.0,
        "2024-12-31T23:59:59Z",
    )
    .await;
    listen_at(
        &app,
        user,
        rock,
        Some(tracks[0]),
        200.0,
        "2025-01-01T00:00:00Z",
    )
    .await;
    listen_at(
        &app,
        user,
        rock,
        Some(tracks[0]),
        300.0,
        "2025-12-31T23:59:59Z",
    )
    .await;
    listen_at(
        &app,
        user,
        rock,
        Some(tracks[0]),
        400.0,
        "2026-01-01T00:00:00Z",
    )
    .await;

    let body = get(&app, &token, "/me/top?kind=tracks&year=2025&period=week").await;
    assert_eq!(body["from"], "2025-01-01T00:00:00Z");
    assert_eq!(body["to"], "2026-01-01T00:00:00Z");
    assert_eq!(body["entries"][0]["seconds"], 500);
    assert_eq!(body["entries"][0]["plays"], 2);
    // A year without listening is an empty list.
    let body = get(&app, &token, "/me/top?kind=albums&year=2019").await;
    assert_eq!(body["entries"], json!([]));
}

#[sqlx::test]
async fn top_checks_its_parameters_and_needs_a_login(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for (uri, detail) in [
        (
            "/me/top",
            "kind muss tracks, albums, artists oder genres sein",
        ),
        (
            "/me/top?kind=books",
            "kind muss tracks, albums, artists oder genres sein",
        ),
        (
            "/me/top?kind=tracks&period=decade",
            "period muss week, month, year oder all sein",
        ),
        (
            "/me/top?kind=tracks&limit=0",
            "limit außerhalb des gültigen Bereichs",
        ),
        (
            "/me/top?kind=tracks&limit=101",
            "limit außerhalb des gültigen Bereichs",
        ),
        (
            "/me/top?kind=tracks&year=1999",
            "year außerhalb des gültigen Bereichs",
        ),
        (
            "/me/top?kind=tracks&year=abc",
            "year außerhalb des gültigen Bereichs",
        ),
        (
            "/me/review?year=2101",
            "year außerhalb des gültigen Bereichs",
        ),
        (
            "/me/on-repeat?limit=0",
            "limit außerhalb des gültigen Bereichs",
        ),
        (
            "/me/repeat-rewind?limit=101",
            "limit außerhalb des gültigen Bereichs",
        ),
    ] {
        let response = app.get(uri, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
        assert_eq!(
            body_json(response).await,
            json!({"detail": detail}),
            "{uri}"
        );
    }
    for uri in [
        "/me/top?kind=tracks",
        "/me/on-repeat",
        "/me/repeat-rewind",
        "/me/review",
        "/me/review/years",
    ] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
    // Nothing heard yet: every list is there and empty.
    for kind in ["tracks", "albums", "artists", "genres"] {
        let body = get(&app, &token, &format!("/me/top?kind={kind}")).await;
        assert_eq!(body["kind"], kind);
        assert_eq!(body["entries"], json!([]));
    }
}

#[sqlx::test]
async fn top_lists_are_each_users_own(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    heard(&app, user).await;
    let other = app.user("gast", false).await;
    let other_token = app.token(other).await;
    for uri in [
        "/me/top?kind=tracks&period=all",
        "/me/top?kind=albums&period=all",
        "/me/top?kind=artists&period=all",
        "/me/top?kind=genres&period=all",
    ] {
        assert_ne!(get(&app, &token, uri).await["entries"], json!([]), "{uri}");
        assert_eq!(
            get(&app, &other_token, uri).await["entries"],
            json!([]),
            "{uri}"
        );
    }
    assert_eq!(
        get(&app, &other_token, "/me/review/years").await,
        json!({"years": []})
    );
}

// --- on repeat ------------------------------------------------------------------------------

#[sqlx::test]
async fn on_repeat_and_repeat_rewind_split_at_thirty_days(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (one, t) = album(
        &app,
        "Eins",
        "Band",
        None,
        &["oft", "öfter", "einmal", "früher", "beides"],
    )
    .await;
    let book = seed(app.db(), Seed::new("book", "Buch")).await;
    let chapter = seed_track(app.db(), book, 1, "/b/1.mp3", 3600.0).await;

    assert_eq!(
        get(&app, &token, "/me/on-repeat").await,
        json!({"tracks": []})
    );
    assert_eq!(
        get(&app, &token, "/me/repeat-rewind").await,
        json!({"tracks": []})
    );

    for ago in ["1 day", "2 days"] {
        listen(&app, user, one, Some(t[0]), 100.0, ago).await;
    }
    for ago in ["1 day", "2 days", "3 days"] {
        listen(&app, user, one, Some(t[1]), 60.0, ago).await;
    }
    // Once is not on repeat, and neither is skipping through twice.
    listen(&app, user, one, Some(t[2]), 200.0, "1 day").await;
    listen(&app, user, one, Some(t[2]), 12.0, "2 days").await;
    // Often, long ago, and not since.
    for ago in ["40 days", "50 days", "60 days"] {
        listen(&app, user, one, Some(t[3]), 100.0, ago).await;
    }
    // Often long ago, but heard again last week: no rewind.
    for ago in ["40 days", "50 days", "5 days"] {
        listen(&app, user, one, Some(t[4]), 100.0, ago).await;
    }
    // A book's file is no song.
    for ago in ["1 day", "2 days", "40 days", "50 days"] {
        listen(&app, user, book, Some(chapter), 900.0, ago).await;
    }

    let ids = |body: &Value| -> Vec<i64> {
        body["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|track| track["id"].as_i64().unwrap())
            .collect()
    };
    let repeat = get(&app, &token, "/me/on-repeat").await;
    assert_eq!(ids(&repeat), [t[1] as i64, t[0] as i64]);
    assert_eq!(
        repeat["tracks"][0],
        json!({
            "id": t[1], "title": "öfter", "artist": null, "duration": 200, "item_id": one,
            "item_title": "Eins", "item_kind": "album", "has_lyrics": false,
            "gain_db": null, "album_gain_db": null, "peak": null
        })
    );
    assert_eq!(
        ids(&get(&app, &token, "/me/on-repeat?limit=1").await),
        [t[1] as i64]
    );
    let rewind = get(&app, &token, "/me/repeat-rewind").await;
    assert_eq!(ids(&rewind), [t[3] as i64]);
}

// --- GET /me/review -------------------------------------------------------------------------

#[sqlx::test]
async fn a_year_without_listening_is_zeros_not_a_404(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    assert_eq!(
        get(&app, &token, "/me/review?year=2021").await,
        json!({
            "year": 2021,
            "seconds": {"total": 0, "music": 0, "books": 0, "podcasts": 0},
            "days_listened": 0,
            "longest_day": null,
            "longest_streak_days": 0,
            "months": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "top_tracks": [],
            "top_albums": [],
            "top_artists": [],
            "top_genres": [],
            "books_finished": [],
            "new_artists": 0,
            "first_listen": null
        })
    );
    // Without a year it is the current one.
    let body = get(&app, &token, "/me/review").await;
    assert_eq!(body["year"], Utc::now().year());
    assert_eq!(
        get(&app, &token, "/me/review/years").await,
        json!({"years": []})
    );
}

#[sqlx::test]
async fn the_review_adds_up_a_calendar_year(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (rock, rock_tracks) =
        album(&app, "Rockplatte", "Die Band", Some("Rock"), &["Auf", "Ab"]).await;
    let (jazz, jazz_tracks) = album(&app, "Jazzplatte", "Das Trio", Some("Jazz"), &["Blau"]).await;
    let book = seed(
        app.db(),
        Seed {
            author: Some("Die Autorin"),
            ..Seed::new("book", "Ein Buch")
        },
    )
    .await;
    let open_book = seed(app.db(), Seed::new("book", "Angefangen")).await;
    let podcast = seed(app.db(), Seed::new("podcast", "Der Podcast")).await;
    let episode = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Folge 1")
        },
    )
    .await;

    // 2024: the band was heard before, so it is no new artist of 2025.
    listen_at(&app, user, rock, None, 500.0, "2024-06-01T10:00:00Z").await;
    // 2025.
    listen_at(&app, user, book, None, 3000.0, "2025-01-30T22:00:00Z").await;
    listen_at(
        &app,
        user,
        rock,
        Some(rock_tracks[0]),
        200.0,
        "2025-01-31T08:00:00Z",
    )
    .await;
    listen_at(
        &app,
        user,
        rock,
        Some(rock_tracks[1]),
        100.0,
        "2025-01-31T08:00:00Z",
    )
    .await;
    listen_at(
        &app,
        user,
        jazz,
        Some(jazz_tracks[0]),
        900.0,
        "2025-02-01T09:00:00Z",
    )
    .await;
    listen_at(&app, user, episode, None, 1800.0, "2025-02-01T12:00:00Z").await;
    listen_at(
        &app,
        user,
        rock,
        Some(rock_tracks[0]),
        200.0,
        "2025-07-10T18:00:00Z",
    )
    .await;
    // 2026 is another year.
    listen_at(
        &app,
        user,
        jazz,
        Some(jazz_tracks[0]),
        7000.0,
        "2026-01-01T00:00:00Z",
    )
    .await;
    // One book finished in 2025, one still open, one finished in another year.
    seed_progress(app.db(), user, book, 3000.0, true).await;
    seed_progress(app.db(), user, open_book, 10.0, false).await;
    sqlx::query(
        "UPDATE progress SET updated_at = '2025-03-05T20:00:00Z',
             finished_at = CASE WHEN finished THEN '2025-03-05T20:00:00Z'::timestamptz END",
    )
    .execute(app.db())
    .await
    .unwrap();

    let body = get(&app, &token, "/me/review?year=2025").await;
    assert_eq!(body["year"], 2025);
    assert_eq!(
        body["seconds"],
        json!({"total": 6200, "music": 1400, "books": 3000, "podcasts": 1800})
    );
    assert_eq!(body["days_listened"], 4);
    assert_eq!(
        body["longest_day"],
        json!({"date": "2025-01-30", "seconds": 3000})
    );
    // 30 January to 1 February.
    assert_eq!(body["longest_streak_days"], 3);
    assert_eq!(
        body["months"],
        json!([3300, 2700, 0, 0, 0, 0, 200, 0, 0, 0, 0, 0])
    );
    let top = &body["top_tracks"];
    assert_eq!(top.as_array().unwrap().len(), 3);
    assert_eq!(top[0]["track"]["id"], jazz_tracks[0]);
    assert_eq!(top[1]["track"]["id"], rock_tracks[0]);
    assert_eq!(top[1]["seconds"], 400);
    assert_eq!(top[1]["plays"], 2);
    assert_eq!(body["top_albums"][0]["item"]["id"], jazz);
    // The morning of two songs is one play of the album.
    assert_eq!(body["top_albums"][1]["item"]["id"], rock);
    assert_eq!(body["top_albums"][1]["seconds"], 500);
    assert_eq!(body["top_albums"][1]["plays"], 2);
    assert_eq!(
        body["top_artists"],
        json!([
            {"artist": "Das Trio", "seconds": 900, "plays": 1, "has_image": false},
            {"artist": "Die Band", "seconds": 500, "plays": 2, "has_image": false},
        ])
    );
    assert_eq!(
        body["top_genres"],
        json!([{"genre": "Jazz", "seconds": 900}, {"genre": "Rock", "seconds": 500}])
    );
    assert_eq!(body["books_finished"].as_array().unwrap().len(), 1);
    assert_eq!(body["books_finished"][0]["id"], book);
    assert_eq!(body["books_finished"][0]["author"], "Die Autorin");
    assert_eq!(body["new_artists"], 1);
    assert_eq!(body["first_listen"]["at"], "2025-01-30T22:00:00Z");
    assert_eq!(body["first_listen"]["item"]["id"], book);
    assert_eq!(body["first_listen"]["item"]["kind"], "book");

    // 2024 saw the band for the first time.
    let before = get(&app, &token, "/me/review?year=2024").await;
    assert_eq!(before["new_artists"], 1);
    assert_eq!(before["seconds"]["total"], 500);
    assert_eq!(before["top_tracks"], json!([]));
    assert_eq!(before["top_albums"][0]["item"]["id"], rock);

    assert_eq!(
        get(&app, &token, "/me/review/years").await,
        json!({"years": [2026, 2025, 2024]})
    );
}

// --- the web pages --------------------------------------------------------------------------

async fn page(app: &TestApp, uri: &str, token: &str) -> String {
    let response = app
        .get_with_cookie(uri, &format!("autoreverse_session={token}"))
        .await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

#[sqlx::test]
async fn the_statistics_page_lists_what_was_heard_most(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (rock, rock_tracks, _, _) = heard(&app, user).await;

    let html = page(&app, "/library/stats", &token).await;
    assert!(html.contains("<h1>Statistik</h1>"));
    assert!(html.contains("Meistgehört"));
    // The last 30 days are the default and marked.
    assert!(html.contains(
        r#"<a href="/library/stats?period=month" class="pill" aria-current="true">30 Tage</a>"#
    ));
    // The tracks are a queue the player can start.
    assert!(html.contains(r#"data-playlist-id="top-tracks""#));
    assert!(html.contains(&format!(
        r#"data-entry-item="{rock}" data-entry-track="{}" data-entry-kind="album""#,
        rock_tracks[0]
    )));
    assert!(html.contains(r#"data-action="play-playlist""#));
    assert!(html.contains(&format!(r#"href="/library/albums/{rock}""#)));
    assert!(html.contains("Die Band"));
    assert!(html.contains(r#"href="/library/artists/Die%20Band""#));
    assert!(html.contains(r#"href="/library/albums?genre=Rock""#));
    assert!(!html.contains("Jazzplatte"));
    // The sidebar leads here, and the page on to the year in review.
    assert!(html.contains(r#"<a href="/library/stats" class="home" aria-current="true">"#));
    assert!(html.contains(&format!(
        r#"href="/library/review?year={}""#,
        Utc::now().year()
    )));

    let all = page(&app, "/library/stats?period=all", &token).await;
    assert!(all.contains("Jazzplatte"));
    // An unknown period is the default, not an error page.
    assert!(
        page(&app, "/library/stats?period=x", &token)
            .await
            .contains("30 Tage")
    );

    let other = app.user("gast", false).await;
    let other_token = app.token(other).await;
    let empty = page(&app, "/library/stats", &other_token).await;
    assert!(empty.contains("In diesem Zeitraum wurde noch keine Musik gehört."));

    let response = app.get("/library/stats", None).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

#[sqlx::test]
async fn the_review_page_shows_a_year_and_offers_the_others(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (rock, tracks) = album(&app, "Rockplatte", "Die Band", Some("Rock"), &["Auf"]).await;
    let book = seed(app.db(), Seed::new("book", "Ein Buch")).await;
    listen_at(
        &app,
        user,
        rock,
        Some(tracks[0]),
        7200.0,
        "2025-03-01T10:00:00Z",
    )
    .await;
    listen_at(&app, user, book, None, 3600.0, "2025-03-02T10:00:00Z").await;
    seed_progress(app.db(), user, book, 3600.0, true).await;
    sqlx::query(
        "UPDATE progress SET updated_at = '2025-03-02T11:00:00Z',
             finished_at = '2025-03-02T11:00:00Z'",
    )
    .execute(app.db())
    .await
    .unwrap();

    let html = page(&app, "/library/review?year=2025", &token).await;
    assert!(html.contains("<title>Rückblick 2025 · Autoreverse</title>"));
    assert!(html.contains(r#"<span class="number">3</span>"#));
    assert!(html.contains("Stunden gehört"));
    assert!(html.contains("Tage gehört"));
    assert!(html.contains("Längste Serie"));
    assert!(html.contains("2 Tage"));
    assert!(html.contains("Neue Interpreten"));
    assert!(html.contains("Monat für Monat"));
    assert!(html.contains("Rockplatte"));
    assert!(html.contains("Beendete Hörbücher"));
    assert!(html.contains(&format!(r#"href="/library/books/{book}""#)));
    assert!(html.contains("Das Jahr begann am 01.03.2025 mit"));
    assert!(html.contains(
        r#"<a href="/library/review?year=2025" class="pill" aria-current="true">2025</a>"#
    ));
    // The current year is on offer too, and empty.
    let this_year = Utc::now().year();
    assert!(html.contains(&format!(r#"href="/library/review?year={this_year}""#)));
    let now = page(&app, "/library/review", &token).await;
    assert!(now.contains(&format!("{this_year} wurde noch nichts gehört.")));
    // A year that is none falls back to the current one.
    assert!(
        page(&app, "/library/review?year=abc", &token)
            .await
            .contains(&format!("Rückblick {this_year}"))
    );
}

#[sqlx::test]
async fn discover_has_the_repeat_rows_and_the_player_a_lyrics_tab(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (one, _) = album(&app, "Eins", "Band", None, &["a"]).await;
    let book = seed(app.db(), Seed::new("book", "Buch")).await;
    seed_track(app.db(), book, 1, "/b/1.mp3", 60.0).await;

    let html = page(&app, "/library/discover", &token).await;
    assert!(
        html.contains(r#"data-section="on-repeat" data-playlist-id="discover-on-repeat" hidden"#)
    );
    assert!(html.contains(
        r#"data-section="repeat-rewind" data-playlist-id="discover-repeat-rewind" hidden"#
    ));
    assert!(html.contains("<h2>On Repeat</h2>"));
    assert!(html.contains("<h2>Repeat Rewind</h2>"));

    let player = page(&app, &format!("/library/{one}/player"), &token).await;
    assert!(player.contains(
        r#"<button class="tab" data-tab="lyrics" data-np-lyrics-tab hidden>Songtext</button>"#
    ));
    assert!(player.contains(r#"<div class="lyrics" aria-label="Songtext" data-np-lyrics></div>"#));
    // Spoken word has no lyrics.
    let player = page(&app, &format!("/library/{book}/player"), &token).await;
    assert!(!player.contains("data-np-lyrics"));
}

#[sqlx::test]
async fn a_finished_book_counts_in_the_year_it_was_finished(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "Ein Buch")).await;
    let this_year = Utc::now().year();
    let uri = format!("/progress/{book}");
    let response = app
        .post_json(
            &uri,
            json!({"position": 3000, "finished": true}),
            Some(&token),
        )
        .await;
    assert!(response.status().is_success(), "{}", response.status());
    // Finished a year ago …
    sqlx::query(
        "UPDATE progress SET finished_at = finished_at - interval '1 year' WHERE user_id = $1",
    )
    .bind(user)
    .execute(app.db())
    .await
    .unwrap();
    // … and today a client syncs it once more, still finished.
    let response = app
        .post_json(
            &uri,
            json!({"position": 3000, "finished": true}),
            Some(&token),
        )
        .await;
    assert!(response.status().is_success());

    let last = get(&app, &token, &format!("/me/review?year={}", this_year - 1)).await;
    assert_eq!(last["books_finished"][0]["id"], book);
    let now = get(&app, &token, &format!("/me/review?year={this_year}")).await;
    assert_eq!(now["books_finished"], json!([]));

    // Begun again: no longer finished, in no year; finished anew: this year.
    app.post_json(
        &uri,
        json!({"position": 10, "finished": false}),
        Some(&token),
    )
    .await;
    let last = get(&app, &token, &format!("/me/review?year={}", this_year - 1)).await;
    assert_eq!(last["books_finished"], json!([]));
    app.post_json(
        &uri,
        json!({"position": 3000, "finished": true}),
        Some(&token),
    )
    .await;
    let now = get(&app, &token, &format!("/me/review?year={this_year}")).await;
    assert_eq!(now["books_finished"][0]["id"], book);
}
