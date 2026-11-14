//! `/ai/*`, with a stand-in for the language model.

use autoreverse_server::Config;
use autoreverse_server::auth::tokens::issue_token;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::post;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_json, body_text, seed};

async fn signed_in(app: &TestApp) -> String {
    let id = app.user("mara", false).await;
    issue_token(app.db(), id).await.unwrap()
}

/// Answers every request as the Messages API would, with `payload` as the model's output.
async fn fake_model(payload: Value) -> String {
    let answer = json!({"stop_reason": "end_turn",
        "content": [{"type": "text", "text": payload.to_string()}]});
    let router = Router::new().route(
        "/v1/messages",
        post(move || {
            let answer = answer.clone();
            async move { axum::Json(answer) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{address}")
}

fn with_model(db: PgPool, url: String) -> TestApp {
    let config = Config::from_lookup(|key| match key {
        "AUTOREVERSE_AI_PROVIDER" => Some("anthropic".into()),
        "AUTOREVERSE_AI_API_KEY" => Some("test".into()),
        "AUTOREVERSE_AI_URL" => Some(url.clone()),
        _ => None,
    })
    .unwrap();
    TestApp::with_config(db, config)
}

async fn track(app: &TestApp, item: i32, title: &str) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO tracks (item_id, position, path, duration, title)
         VALUES ($1, 1, $2, 180, $2) RETURNING id",
    )
    .bind(item)
    .bind(title)
    .fetch_one(app.db())
    .await
    .unwrap()
}

#[sqlx::test]
async fn needs_a_login(db: PgPool) {
    let app = TestApp::new(db);
    let response = app.get("/ai/status", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn says_when_no_model_is_set_up(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let status = body_json(app.get("/ai/status", Some(&token)).await).await;
    assert_eq!(status["enabled"], false);
    let response = app
        .post_json("/ai/playlist", json!({"prompt": "ruhig"}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[sqlx::test]
async fn rejects_an_empty_wish(db: PgPool) {
    let app = with_model(db, "http://127.0.0.1:9".into());
    let token = signed_in(&app).await;
    let response = app
        .post_json("/ai/playlist", json!({"prompt": "  "}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn a_playlist_contains_only_tracks_of_the_library(db: PgPool) {
    let probe = TestApp::new(db.clone());
    let album = seed(probe.db(), Seed::new("album", "Autobahn")).await;
    let id = track(&probe, album, "Kometenmelodie").await;
    let url = fake_model(json!({"name": "Fahrt", "ids": [album, id, 987654]})).await;
    let app = with_model(db, url);
    let token = signed_in(&app).await;
    let response = app
        .post_json("/ai/playlist", json!({"prompt": "Autofahrt"}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["name"], "Fahrt");
    assert_eq!(body["tracks"].as_array().unwrap().len(), 1);
    assert_eq!(body["tracks"][0]["id"], id);
    assert_eq!(body["tracks"][0]["has_lyrics"], false);
}

#[sqlx::test]
async fn a_playlist_says_which_tracks_have_lyrics(db: PgPool) {
    let probe = TestApp::new(db.clone());
    let album = seed(probe.db(), Seed::new("album", "Autobahn")).await;
    let id = track(&probe, album, "Kometenmelodie").await;
    sqlx::query("UPDATE tracks SET has_lyrics = true WHERE id = $1")
        .bind(id)
        .execute(probe.db())
        .await
        .unwrap();
    let url = fake_model(json!({"name": "Fahrt", "ids": [album, id]})).await;
    let app = with_model(db, url);
    let token = signed_in(&app).await;
    let body = body_json(
        app.post_json("/ai/playlist", json!({"prompt": "Autofahrt"}), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["tracks"][0]["has_lyrics"], true);
}

#[sqlx::test]
async fn suggestions_skip_invented_albums(db: PgPool) {
    let probe = TestApp::new(db.clone());
    let album = seed(probe.db(), Seed::new("album", "Autobahn")).await;
    let url = fake_model(json!({"albums": [
        {"id": album, "reason": "Passt"}, {"id": 987654, "reason": "Erfunden"}]}))
    .await;
    let app = with_model(db, url);
    let token = signed_in(&app).await;
    let body = body_json(
        app.post_json("/ai/suggestions", json!({}), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["albums"].as_array().unwrap().len(), 1);
    assert_eq!(body["albums"][0]["reason"], "Passt");
}

#[sqlx::test]
async fn limits_requests_per_user(db: PgPool) {
    let app = with_model(db, fake_model(json!({"albums": []})).await);
    let token = signed_in(&app).await;
    for _ in 0..10 {
        let response = app
            .post_json("/ai/suggestions", json!({}), Some(&token))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = app
        .post_json("/ai/suggestions", json!({}), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn the_web_page_renders_with_and_without_a_model(db: PgPool) {
    let app = TestApp::new(db.clone());
    let token = signed_in(&app).await;
    let response = app.get("/library/discover", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let without = body_text(response).await;
    assert!(!without.contains("data-ai-playlist-form"));
    let app = with_model(db, "http://127.0.0.1:9".into());
    let response = app.get("/library/discover", Some(&token)).await;
    let with = body_text(response).await;
    assert!(with.contains("data-ai-playlist-form"));
}

/// Polls until the background run has stored something.
async fn settled_feed(app: &TestApp, token: &str) -> Value {
    for _ in 0..100 {
        let body = body_json(app.get("/ai/feed", Some(token)).await).await;
        if !body["refreshing"].as_bool().unwrap() && body["generated_at"].is_string() {
            return body;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the feed never settled");
}

#[sqlx::test]
async fn the_feed_shows_library_rails_without_a_model(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let mut heard = Seed::new("album", "Autobahn");
    heard.artist = Some("Kraftwerk");
    let heard = seed(app.db(), heard).await;
    let mut new = Seed::new("album", "Computerwelt");
    new.artist = Some("Kraftwerk");
    seed(app.db(), new).await;
    sqlx::query(
        "INSERT INTO musicbrainz_artists (artist, albums) VALUES ('Kraftwerk', '[]'::jsonb)",
    )
    .execute(app.db())
    .await
    .unwrap();
    listen(&app, heard, 3600.0).await;
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["refreshing"], false);
    assert_eq!(body["fresh"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn the_feed_fills_itself_and_keeps_only_real_entries(db: PgPool) {
    let probe = TestApp::new(db.clone());
    let album = seed(probe.db(), Seed::new("album", "Autobahn")).await;
    let id = track(&probe, album, "Kometenmelodie").await;
    let mut known = Seed::new("album", "Schon gehört");
    known.artist = Some("Kraftwerk");
    let heard = seed(probe.db(), known).await;
    sqlx::query(
        "INSERT INTO musicbrainz_artists (artist, albums) VALUES ('Kraftwerk', '[]'::jsonb)",
    )
    .execute(probe.db())
    .await
    .unwrap();
    let url = fake_model(json!({
        "albums": [
            {"id": album, "reason": "Passt"},
            {"id": heard, "reason": "Kennt man schon"},
            {"id": 987654, "reason": "Erfunden"},
        ],
        "mixes": [{"name": "Fahrt", "description": "Unterwegs", "ids": [album, 987654]}],
        "artists": [{"name": "Neu!", "reason": "Klar", "albums": []}],
    }))
    .await;
    let app = with_model(db, url);
    let token = signed_in(&app).await;
    listen(&app, heard, 3600.0).await;
    let first = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(first["refreshing"], true);
    let body = settled_feed(&app, &token).await;
    assert_eq!(body["suggestions"].as_array().unwrap().len(), 1);
    assert_eq!(body["mixes"][0]["tracks"][0]["id"], id);
    assert_eq!(body["mixes"][0]["tracks"][0]["has_lyrics"], false);
    assert_eq!(body["artists"][0]["name"], "Neu!");

    // A scan finds a `.lrc` after the model ran: the stored mix says so on the next view.
    sqlx::query("UPDATE tracks SET has_lyrics = true WHERE id = $1")
        .bind(id)
        .execute(app.db())
        .await
        .unwrap();
    let again = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(again["mixes"][0]["tracks"][0]["has_lyrics"], true);
}

/// A feed stored before `has_lyrics` existed, holding a track a rescan has since removed.
#[sqlx::test]
async fn a_stored_mix_from_before_the_lyrics_flag_still_loads(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let user: i32 = sqlx::query_scalar("SELECT id FROM users WHERE name = 'mara'")
        .fetch_one(app.db())
        .await
        .unwrap();
    let album = seed(app.db(), Seed::new("album", "Autobahn")).await;
    let kept = track(&app, album, "Kometenmelodie").await;
    sqlx::query("UPDATE tracks SET has_lyrics = true WHERE id = $1")
        .bind(kept)
        .execute(app.db())
        .await
        .unwrap();
    let old = |id: i32| {
        json!({"id": id, "title": "Alt", "artist": null, "item_id": album,
               "item_title": "Autobahn", "duration": 180.0})
    };
    let payload = json!({"mixes": [{"name": "Fahrt", "description": "", "cover_ids": [album],
                                    "tracks": [old(kept), old(987654)]}]});
    sqlx::query("INSERT INTO ai_feed (user_id, payload) VALUES ($1, $2)")
        .bind(user)
        .bind(payload)
        .execute(app.db())
        .await
        .unwrap();
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    let tracks = body["mixes"][0]["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["id"], kept);
    assert_eq!(tracks[0]["has_lyrics"], true);
}

#[sqlx::test]
async fn a_failing_model_is_not_asked_again_on_every_view(db: PgPool) {
    let app = with_model(db, "http://127.0.0.1:9".into());
    let token = signed_in(&app).await;
    let first = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(first["refreshing"], true);
    for _ in 0..100 {
        let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
        if !body["refreshing"].as_bool().unwrap() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("kept retrying");
}

async fn listen(app: &TestApp, item: i32, seconds: f64) {
    sqlx::query(
        "INSERT INTO listen_events (user_id, item_id, seconds)
         SELECT id, $1, $2 FROM users WHERE name = 'mara'",
    )
    .bind(item)
    .bind(seconds)
    .execute(app.db())
    .await
    .unwrap();
}

#[sqlx::test]
async fn without_a_history_no_model_is_asked(db: PgPool) {
    let probe = TestApp::new(db.clone());
    seed(probe.db(), Seed::new("album", "Autobahn")).await;
    // Nothing listens here: asking it would leave generated_at empty.
    let app = with_model(db, "http://127.0.0.1:9".into());
    let token = signed_in(&app).await;
    app.get("/ai/feed", Some(&token)).await;
    let body = settled_feed(&app, &token).await;
    assert_eq!(body["has_history"], false);
    assert_eq!(body["suggestions"], json!([]));
    assert_eq!(body["fresh"], json!([]));
}

#[sqlx::test]
async fn wanted_albums_come_from_musicbrainz_for_artists_the_user_hears(db: PgPool) {
    let probe = TestApp::new(db.clone());
    let mut heard = Seed::new("album", "Autobahn");
    heard.artist = Some("Kraftwerk");
    let album = seed(probe.db(), heard).await;
    let mut other = Seed::new("album", "Nicht gehört");
    other.artist = Some("Sparrow Sleeps");
    seed(probe.db(), other).await;
    for (artist, albums) in [
        (
            "Kraftwerk",
            r#"[{"title":"Autobahn","year":1974},{"title":"Computerwelt","year":1981}]"#,
        ),
        (
            "Sparrow Sleeps",
            r#"[{"title":"Wiegenlieder","year":2025}]"#,
        ),
    ] {
        sqlx::query("INSERT INTO musicbrainz_artists (artist, albums) VALUES ($1, $2::jsonb)")
            .bind(artist)
            .bind(albums)
            .execute(probe.db())
            .await
            .unwrap();
    }
    let url = fake_model(json!({
        "albums": [{"nr": 0, "reason": "Weil du Autobahn hörst"}, {"nr": 42, "reason": "Erfunden"}],
        "mixes": [],
        "artists": [],
    }))
    .await;
    let app = with_model(db, url);
    let token = signed_in(&app).await;
    listen(&app, album, 3600.0).await;
    app.get("/ai/feed", Some(&token)).await;
    let body = settled_feed(&app, &token).await;
    assert_eq!(body["wanted"].as_array().unwrap().len(), 1);
    assert_eq!(body["wanted"][0]["title"], "Computerwelt");
    assert_eq!(body["wanted"][0]["reason"], "Weil du Autobahn hörst");
}

async fn long_album(app: &TestApp, title: &str, tracks: i32) -> i32 {
    let album = seed(app.db(), Seed::new("album", title)).await;
    for position in 1..=tracks {
        sqlx::query(
            "INSERT INTO tracks (item_id, position, path, duration, title)
             VALUES ($1, $2, $3, 247, $3)",
        )
        .bind(album)
        .bind(position)
        .bind(format!("{title} {position}"))
        .execute(app.db())
        .await
        .unwrap();
    }
    album
}

fn running_time(tracks: &Value) -> f64 {
    tracks
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["duration"].as_f64().unwrap())
        .sum()
}

#[sqlx::test]
async fn a_short_answer_still_makes_a_two_hour_playlist(db: PgPool) {
    let probe = TestApp::new(db.clone());
    let album = long_album(&probe, "Lang", 40).await;
    let url = fake_model(json!({"name": "Lang", "minutes": 0, "ids": [album]})).await;
    let app = with_model(db, url);
    let token = signed_in(&app).await;
    let body = body_json(
        app.post_json("/ai/playlist", json!({"prompt": "ruhig"}), Some(&token))
            .await,
    )
    .await;
    assert!(running_time(&body["tracks"]) >= 7200.0);
}

#[sqlx::test]
async fn mixes_run_for_two_hours(db: PgPool) {
    let probe = TestApp::new(db.clone());
    let album = long_album(&probe, "Lang", 40).await;
    let heard = seed(probe.db(), Seed::new("album", "Gehört")).await;
    let url = fake_model(json!({
        "albums": [],
        "mixes": [{"name": "Lang", "description": "", "ids": [album]}],
        "artists": [],
    }))
    .await;
    let app = with_model(db, url);
    let token = signed_in(&app).await;
    listen(&app, heard, 3600.0).await;
    app.get("/ai/feed", Some(&token)).await;
    let body = settled_feed(&app, &token).await;
    assert!(running_time(&body["mixes"][0]["tracks"]) >= 7200.0);
}

// --- Spoken word in the feed ----------------------------------------------------------------

async fn episode(app: &TestApp, podcast: i32, title: &str, days_ahead: i32) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO items (kind, parent_id, title, sort_title, published_at, feed_duration)
         VALUES ('episode', $1, $2, lower($2), now() + make_interval(days => $3), 1500)
         RETURNING id",
    )
    .bind(podcast)
    .bind(title)
    .bind(days_ahead)
    .fetch_one(app.db())
    .await
    .unwrap()
}

fn ids(list: &Value) -> Vec<i64> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_i64().unwrap())
        .collect()
}

fn volume<'a>(title: &'a str, series: &'a str, index: f64) -> Seed<'a> {
    Seed {
        author: Some("Ursula K. Le Guin"),
        series: Some(series),
        series_index: Some(index),
        ..Seed::new("book", title)
    }
}

async fn progress(app: &TestApp, user: i32, item: i32, position: f64, finished: bool, days: i32) {
    sqlx::query(
        "INSERT INTO progress (user_id, item_id, position, finished, updated_at)
         VALUES ($1, $2, $3, $4, now() - make_interval(days => $5))",
    )
    .bind(user)
    .bind(item)
    .bind(position)
    .bind(finished)
    .bind(days)
    .execute(app.db())
    .await
    .unwrap();
}

#[sqlx::test]
async fn an_empty_library_still_has_every_row_of_the_feed(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    for row in [
        "suggestions",
        "mixes",
        "artists",
        "wanted",
        "rediscover",
        "fresh",
        "new_episodes",
        "unstarted_books",
        "continue_series",
        "rediscover_books",
    ] {
        assert_eq!(body[row], json!([]), "{row}");
    }
}

#[sqlx::test]
async fn the_feed_lists_unheard_episodes_like_the_podcast_page(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let user: i32 = sqlx::query_scalar("SELECT id FROM users WHERE name = 'mara'")
        .fetch_one(app.db())
        .await
        .unwrap();
    let mut show = Seed::new("podcast", "Zeitzeichen");
    show.cover_path = Some("/covers/z.jpg");
    let show = seed(app.db(), show).await;
    // From before the subscription: back catalogue, not news.
    episode(&app, show, "Archiv", -30).await;
    let heard = episode(&app, show, "Gehört", 1).await;
    let begun = episode(&app, show, "Angefangen", 2).await;
    let newest = episode(&app, show, "Ganz neu", 3).await;
    progress(&app, user, heard, 1500.0, true, 0).await;
    progress(&app, user, begun, 300.0, false, 0).await;

    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(ids(&body["new_episodes"]), [newest as i64, begun as i64]);
    let entry = &body["new_episodes"][1];
    assert_eq!(entry["title"], "Angefangen");
    assert_eq!(entry["podcast_id"], show);
    assert_eq!(entry["podcast_title"], "Zeitzeichen");
    assert_eq!(entry["has_cover"], true);
    assert_eq!(entry["downloaded"], false);
    assert_eq!(entry["duration"], json!(1500));
    assert_eq!(entry["position"], json!(300));
    assert!(entry["published_at"].as_str().unwrap().ends_with('Z'));
    // The same entries, in the same shape, as the route the podcast page uses.
    let page = body_json(app.get("/podcasts/new", Some(&token)).await).await;
    assert_eq!(body["new_episodes"], page["episodes"]);

    // An episode that arrives shows up on the next view, without waiting for a stored feed to
    // age; one that is finished leaves.
    let arrived = episode(&app, show, "Eben erschienen", 4).await;
    app.call(
        axum::http::Method::PUT,
        &format!("/progress/{newest}"),
        Some(json!({"position": 1500, "finished": true})),
        Some(&token),
    )
    .await;
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(ids(&body["new_episodes"]), [arrived as i64, begun as i64]);

    // Twelve at most.
    for n in 0..15 {
        episode(&app, show, &format!("Folge {n}"), 5).await;
    }
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(body["new_episodes"].as_array().unwrap().len(), 12);
}

#[sqlx::test]
async fn the_feed_lists_books_nobody_began_newest_first(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let user: i32 = sqlx::query_scalar("SELECT id FROM users WHERE name = 'mara'")
        .fetch_one(app.db())
        .await
        .unwrap();
    let other = app.user("someone", false).await;
    let days_ago = |days: i64| Some(chrono::Utc::now() - chrono::Duration::days(days));
    let older = seed(
        app.db(),
        Seed {
            added_at: days_ago(20),
            ..Seed::new("book", "Älter")
        },
    )
    .await;
    let newer = seed(
        app.db(),
        Seed {
            added_at: days_ago(2),
            author: Some("Frank Schätzing"),
            narrator: Some("Stefan Kaminski"),
            series: Some("Tiefsee"),
            series_index: Some(2.0),
            cover_path: Some("/covers/s.jpg"),
            ..Seed::new("book", "Der Schwarm")
        },
    )
    .await;
    crate::common::seed_track(app.db(), newer, 1, "/s1", 100.0).await;
    crate::common::seed_track(app.db(), newer, 2, "/s2", 250.5).await;
    let begun = seed(app.db(), Seed::new("book", "Begonnen")).await;
    let done = seed(app.db(), Seed::new("book", "Fertig")).await;
    // Opened and closed at once: a progress row, but nothing heard.
    let touched = seed(
        app.db(),
        Seed {
            added_at: days_ago(40),
            ..Seed::new("book", "Nur geöffnet")
        },
    )
    .await;
    let theirs = seed(
        app.db(),
        Seed {
            added_at: days_ago(50),
            ..Seed::new("book", "Von anderen gehört")
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("book", "Weg")
        },
    )
    .await;
    seed(app.db(), Seed::new("album", "Kein Buch")).await;
    progress(&app, user, begun, 12.0, false, 0).await;
    progress(&app, user, done, 0.0, true, 0).await;
    progress(&app, user, touched, 0.0, false, 0).await;
    progress(&app, other, theirs, 900.0, false, 0).await;

    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(
        ids(&body["unstarted_books"]),
        [newer, older, touched, theirs].map(i64::from)
    );
    // An item summary as `/items` sends it, plus what a tile needs.
    assert_eq!(
        body["unstarted_books"][0].to_string(),
        json!({
            "id": newer, "kind": "book", "title": "Der Schwarm", "has_cover": true,
            "author": "Frank Schätzing", "artist": null, "series": "Tiefsee", "year": null,
            "missing_since": null, "published_at": null,
            "narrator": "Stefan Kaminski", "series_index": 2, "duration": 350.5, "position": 0
        })
        .to_string()
    );
    let listed = body_json(app.get("/items?kind=book&q=Schwarm", Some(&token)).await).await;
    for (field, value) in listed["items"][0].as_object().unwrap() {
        assert_eq!(&body["unstarted_books"][0][field], value, "{field}");
    }

    // Beginning a book takes it off the row at once.
    app.call(
        axum::http::Method::PUT,
        &format!("/progress/{newer}"),
        Some(json!({"position": 30})),
        Some(&token),
    )
    .await;
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(
        ids(&body["unstarted_books"]),
        [older, touched, theirs].map(i64::from)
    );
}

#[sqlx::test]
async fn the_feed_offers_the_next_volume_of_a_series_someone_is_in(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let user: i32 = sqlx::query_scalar("SELECT id FROM users WHERE name = 'mara'")
        .fetch_one(app.db())
        .await
        .unwrap();
    // Erdsee: 1 skipped, 2 finished, 3 begun, 4 and 5 untouched — 4 is next, not 1.
    seed(app.db(), volume("Der Magier der Erdsee", "Erdsee", 1.0)).await;
    let second = seed(app.db(), volume("Die Gräber von Atuan", "Erdsee", 2.0)).await;
    let third = seed(app.db(), volume("Das ferne Ufer", "Erdsee", 3.0)).await;
    let fourth = seed(app.db(), volume("Tehanu", "Erdsee", 4.0)).await;
    seed(app.db(), volume("Das andere Ufer", "Erdsee", 5.0)).await;
    progress(&app, user, second, 0.0, true, 10).await;
    progress(&app, user, third, 500.0, false, 1).await;
    // Hainish: finished more recently, so it comes first. Without volume numbers the titles
    // decide, "Band 10" after "Band 2".
    let mut two = volume("Band 2", "Hainish", 0.0);
    two.series_index = None;
    let mut ten = volume("Band 10", "Hainish", 0.0);
    ten.series_index = None;
    let two = seed(app.db(), two).await;
    let ten = seed(app.db(), ten).await;
    progress(&app, user, two, 0.0, true, 2).await;
    // The same series name by another author is another series, and nothing of it is finished.
    let mut foreign = volume("Fremdes Erdsee 3", "Erdsee", 3.0);
    foreign.author = Some("Jemand anderes");
    seed(app.db(), foreign).await;
    // A series nothing was finished in, and one that is finished to the end.
    let begun = seed(app.db(), volume("Anfang", "Unberührt", 1.0)).await;
    seed(app.db(), volume("Fortsetzung", "Unberührt", 2.0)).await;
    progress(&app, user, begun, 40.0, false, 0).await;
    let last = seed(app.db(), volume("Schluss", "Zu Ende", 1.0)).await;
    progress(&app, user, last, 0.0, true, 0).await;

    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(ids(&body["continue_series"]), [ten, fourth].map(i64::from));
    assert_eq!(body["continue_series"][1]["series"], "Erdsee");
    assert_eq!(body["continue_series"][1]["series_index"], json!(4));
    assert_eq!(body["continue_series"][0]["series_index"], Value::Null);

    // Once the offered volume is begun, the one after it takes its place.
    app.call(
        axum::http::Method::PUT,
        &format!("/progress/{fourth}"),
        Some(json!({"position": 5})),
        Some(&token),
    )
    .await;
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(body["continue_series"][1]["title"], "Das andere Ufer");
}

#[sqlx::test]
async fn the_feed_brings_back_books_put_down_long_ago(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let user: i32 = sqlx::query_scalar("SELECT id FROM users WHERE name = 'mara'")
        .fetch_one(app.db())
        .await
        .unwrap();
    let long_ago = seed(app.db(), Seed::new("book", "Vor einem Jahr")).await;
    let a_while = seed(app.db(), Seed::new("book", "Vor zwei Monaten")).await;
    let lately = seed(app.db(), Seed::new("book", "Letzte Woche")).await;
    let done = seed(app.db(), Seed::new("book", "Fertig")).await;
    let album = seed(app.db(), Seed::new("album", "Musik")).await;
    progress(&app, user, long_ago, 100.0, false, 365).await;
    progress(&app, user, a_while, 2000.5, false, 60).await;
    progress(&app, user, lately, 100.0, false, 7).await;
    progress(&app, user, done, 100.0, true, 200).await;
    progress(&app, user, album, 100.0, false, 200).await;

    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(
        ids(&body["rediscover_books"]),
        [a_while, long_ago].map(i64::from)
    );
    assert_eq!(body["rediscover_books"][0]["position"], json!(2000.5));
    // They are begun, so they are in no row of books to begin.
    assert_eq!(body["unstarted_books"], json!([]));
}

#[sqlx::test]
async fn a_reported_listen_counts_as_history_for_the_feed(db: PgPool) {
    let app = TestApp::new(db);
    let token = signed_in(&app).await;
    let album = seed(app.db(), Seed::new("album", "Offline gehört")).await;
    let track = track(&app, album, "Titel").await;
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(body["has_history"], false);
    let started = chrono::Utc::now() - chrono::Duration::days(120);
    let response = app
        .post_json(
            "/me/listens",
            json!({"listens": [{"key": "k1", "track_id": track, "seconds": 2400,
                "started_at": started.to_rfc3339()}]}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(app.get("/ai/feed", Some(&token)).await).await;
    assert_eq!(body["has_history"], true);
    // Dated to when it was heard, four months ago: long enough to be worth hearing again.
    assert_eq!(ids(&body["rediscover"]), [album as i64]);
}

#[sqlx::test]
async fn the_web_page_has_the_rows_of_spoken_word_without_a_model(db: PgPool) {
    let app = TestApp::new(db.clone());
    let token = signed_in(&app).await;
    let without = body_text(app.get("/library/discover", Some(&token)).await).await;
    let app = with_model(db, "http://127.0.0.1:9".into());
    let with = body_text(app.get("/library/discover", Some(&token)).await).await;
    for page in [&without, &with] {
        for (row, heading, accent) in [
            ("episodes", "Neue Folgen", "podcast"),
            ("series", "Weiter in der Serie", "book"),
            ("unstarted", "Noch nicht gehört", "book"),
            ("paused", "Wieder aufnehmen", "book"),
        ] {
            // Hidden until static/js/discover.js has something to put in, in its own colour.
            let section = format!(r#"data-section="{row}" style="--a: var(--{accent})" hidden>"#);
            assert!(page.contains(&section), "{row}");
            assert!(page.contains(&format!("<h2>{heading}</h2>")), "{heading}");
        }
    }
}
