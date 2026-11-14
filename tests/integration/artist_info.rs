//! Who an artist is: `GET /artists/{name}/info` and `GET /artists/{name}/pictures/{index}` (new
//! in Rust). Both catalogues are a local [`FakeWeb`]; nothing here reaches the internet.

use std::path::Path;

use axum::http::{StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

use autoreverse_server::Config;

use crate::common::{FakeWeb, Seed, TempLibrary, TestApp, body_json, header_str, seed};

const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];

/// The app with its data in `data` and both catalogues on `web`.
fn app_with(db: PgPool, data: &Path, web: &FakeWeb) -> TestApp {
    let data = data.to_str().unwrap().to_owned();
    let mut config =
        Config::from_lookup(|key| (key == "AUTOREVERSE_DATA").then(|| data.clone())).unwrap();
    config.audiodb_url = web.url("/adb");
    config.music_catalog_url = web.url("/apple");
    TestApp::with_config(db, config)
}

async fn token(app: &TestApp) -> String {
    let user = app.user("ada", false).await;
    app.token(user).await
}

async fn album(db: &PgPool, artist: &str, title: &str) -> i32 {
    seed(
        db,
        Seed {
            artist: Some(artist),
            ..Seed::new("album", title)
        },
    )
    .await
}

async fn track(db: &PgPool, item: i32, position: i32, title: &str, duration: f64) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO tracks (item_id, position, path, duration, title)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(item)
    .bind(position)
    .bind(format!("/music/{item}/{position}.mp3"))
    .bind(duration)
    .bind(title)
    .fetch_one(db)
    .await
    .unwrap()
}

/// What the catalogues say about AC/DC, with the pictures on `web` itself.
fn catalogues_know_acdc(web: &FakeWeb) {
    web.set(
        "/adb/search.php",
        200,
        json!({"artists": [{
            "strArtist": "AC/DC", "strBiography": "A band.", "strBiographyDE": "Eine Band.",
            "strGenre": "Hard Rock", "strCountry": "Sydney, Australia", "intFormedYear": "1973",
            "strArtistThumb": web.url("/pic/thumb"), "strArtistFanart": web.url("/pic/wide"),
        }]})
        .to_string(),
    );
    web.set(
        "/apple/search",
        200,
        json!({"results": [{"artistId": 7, "artistName": "AC/DC"}]}).to_string(),
    );
    web.set(
        "/apple/lookup",
        200,
        json!({"results": [
            {"wrapperType": "artist", "artistName": "AC/DC"},
            {"trackName": "Thunderstruck", "collectionName": "The Razors Edge", "trackTimeMillis": 292000},
            {"trackName": "Big Gun", "collectionName": "Big Gun - Single", "trackTimeMillis": 260000},
        ]})
        .to_string(),
    );
    web.set("/pic/thumb", 200, PNG);
    web.set("/pic/wide", 200, PNG);
}

#[sqlx::test]
async fn the_overview_matches_the_chart_to_the_library_and_asks_the_catalogues_once(db: PgPool) {
    let library = TempLibrary::new();
    let web = FakeWeb::start().await;
    catalogues_know_acdc(&web);
    let app = app_with(db, &library.data(), &web);
    let token = token(&app).await;
    let razors = album(app.db(), "AC/DC", "The Razors Edge").await;
    let live = album(app.db(), "AC/DC", "Live").await;
    track(app.db(), live, 1, "Thunderstruck (Live - 1991)", 395.0).await;
    let studio = track(app.db(), razors, 1, "Thunderstruck", 293.0).await;

    let response = app.get("/artists/AC%2FDC/info?lang=de", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["artist"], "AC/DC");
    assert_eq!(body["found"], true);
    assert_eq!(body["biography"], "Eine Band.");
    assert_eq!(body["biography_language"], "de");
    assert_eq!(body["genre"], "Hard Rock");
    assert_eq!(body["country"], "Sydney, Australia");
    assert_eq!(body["formed_year"], 1973);
    assert_eq!(body["born_year"], Value::Null);
    assert_eq!(
        body["pictures"],
        json!([{"index": 0, "kind": "portrait"}, {"index": 1, "kind": "backdrop"}])
    );
    assert_eq!(
        body["top_tracks"],
        json!([
            {"title": "Thunderstruck", "album": "The Razors Edge", "duration": 292,
             "track": {"id": studio, "title": "Thunderstruck", "artist": null, "duration": 293,
                       "item_id": razors, "item_title": "The Razors Edge", "item_kind": "album",
                       "has_lyrics": false,
            "gain_db": null, "album_gain_db": null, "peak": null}},
            {"title": "Big Gun", "album": "Big Gun", "duration": 260, "track": null},
        ])
    );
    assert!(body["fetched_at"].as_str().unwrap().ends_with('Z'));

    // The second visit reads the database: another language, and no further request.
    let again = body_json(app.get("/artists/AC%2FDC/info?lang=fr", Some(&token)).await).await;
    assert_eq!(again["biography"], "A band.");
    assert_eq!(again["biography_language"], "en");
    assert_eq!(web.hits("/adb/search.php"), 1);
    assert_eq!(web.hits("/apple/lookup"), 1);
}

#[sqlx::test]
async fn a_picture_is_fetched_once_and_then_served_from_the_servers_disk(db: PgPool) {
    let library = TempLibrary::new();
    let web = FakeWeb::start().await;
    catalogues_know_acdc(&web);
    let app = app_with(db, &library.data(), &web);
    let token = token(&app).await;
    album(app.db(), "AC/DC", "The Razors Edge").await;

    // Before anybody asked about the artist there is no list to count in.
    let early = app.get("/artists/AC%2FDC/pictures/0", Some(&token)).await;
    assert_eq!(early.status(), StatusCode::NOT_FOUND);
    app.get("/artists/AC%2FDC/info", Some(&token)).await;

    for _ in 0..2 {
        let response = app.get("/artists/AC%2FDC/pictures/1", Some(&token)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    }
    assert_eq!(web.hits("/pic/wide"), 1);
    assert_eq!(web.hits("/pic/thumb"), 0);
    let stored: Vec<_> = std::fs::read_dir(library.data().join("artists/info"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(stored.len(), 1);
    assert!(stored[0].ends_with("-1.png"), "{stored:?}");

    let beyond = app.get("/artists/AC%2FDC/pictures/2", Some(&token)).await;
    assert_eq!(beyond.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(beyond).await["detail"], "Kein Bild");
    let nonsense = app.get("/artists/AC%2FDC/pictures/x", Some(&token)).await;
    assert_eq!(nonsense.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn a_picture_address_outside_the_catalogue_is_never_fetched(db: PgPool) {
    let library = TempLibrary::new();
    let web = FakeWeb::start().await;
    let elsewhere = FakeWeb::start().await;
    elsewhere.set("/secret", 200, PNG);
    catalogues_know_acdc(&web);
    web.set(
        "/adb/search.php",
        200,
        json!({"artists": [{"strArtist": "AC/DC", "strArtistThumb": elsewhere.url("/secret")}]})
            .to_string(),
    );
    let app = app_with(db, &library.data(), &web);
    let token = token(&app).await;
    album(app.db(), "AC/DC", "The Razors Edge").await;
    app.get("/artists/AC%2FDC/info", Some(&token)).await;

    let response = app.get("/artists/AC%2FDC/pictures/0", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(elsewhere.hits("/secret"), 0);
}

#[sqlx::test]
async fn an_unreachable_catalogue_is_no_error_and_is_asked_again(db: PgPool) {
    let library = TempLibrary::new();
    let web = FakeWeb::start().await;
    web.set("/adb/search.php", 500, "kaputt");
    let app = app_with(db, &library.data(), &web);
    let token = token(&app).await;
    album(app.db(), "AC/DC", "The Razors Edge").await;

    let body = body_json(app.get("/artists/AC%2FDC/info", Some(&token)).await).await;
    assert_eq!(body["found"], false);
    assert_eq!(body["biography"], Value::Null);
    assert_eq!(body["top_tracks"], json!([]));
    assert_eq!(body["fetched_at"], Value::Null);
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM artist_info")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(kept, 0);

    catalogues_know_acdc(&web);
    let body = body_json(app.get("/artists/AC%2FDC/info", Some(&token)).await).await;
    assert_eq!(body["found"], true);
}

#[sqlx::test]
async fn an_artist_nobody_knows_is_remembered_as_unknown(db: PgPool) {
    let library = TempLibrary::new();
    let web = FakeWeb::start().await;
    web.set("/adb/search.php", 200, json!({"artists": null}).to_string());
    web.set("/apple/search", 200, json!({"results": []}).to_string());
    let app = app_with(db, &library.data(), &web);
    let token = token(&app).await;
    album(app.db(), "Die Unbekannten", "Erstes").await;

    for _ in 0..2 {
        let response = app
            .get("/artists/Die%20Unbekannten/info", Some(&token))
            .await;
        let body = body_json(response).await;
        assert_eq!(body["found"], false);
        assert_eq!(body["pictures"], json!([]));
    }
    assert_eq!(web.hits("/adb/search.php"), 1);
}

#[sqlx::test]
async fn only_artists_of_the_library_are_looked_up_and_only_when_signed_in(db: PgPool) {
    let library = TempLibrary::new();
    let web = FakeWeb::start().await;
    catalogues_know_acdc(&web);
    let app = app_with(db, &library.data(), &web);
    let token = token(&app).await;

    let stranger = app.get("/artists/AC%2FDC/info", Some(&token)).await;
    assert_eq!(stranger.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(stranger).await["detail"],
        "Interpret nicht gefunden"
    );
    album(app.db(), "AC/DC", "The Razors Edge").await;
    let anonymous = app.get("/artists/AC%2FDC/info", None).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(web.hits("/adb/search.php"), 0);
}

#[sqlx::test]
async fn switched_off_the_server_asks_nobody(db: PgPool) {
    let library = TempLibrary::new();
    let web = FakeWeb::start().await;
    catalogues_know_acdc(&web);
    let data = library.data().to_str().unwrap().to_owned();
    let mut config = Config::from_lookup(|key| match key {
        "AUTOREVERSE_DATA" => Some(data.clone()),
        "AUTOREVERSE_ARTIST_INFO" => Some("0".into()),
        _ => None,
    })
    .unwrap();
    config.audiodb_url = web.url("/adb");
    config.music_catalog_url = web.url("/apple");
    let app = TestApp::with_config(db, config);
    let token = token(&app).await;
    album(app.db(), "AC/DC", "The Razors Edge").await;

    let body = body_json(app.get("/artists/AC%2FDC/info", Some(&token)).await).await;
    assert_eq!(body["found"], false);
    assert_eq!(web.hits("/adb/search.php"), 0);
}
