//! `GET /items`, `GET /items/{id}`, `GET /items/{id}/cover` (legacy items-api.test.ts,
//! description-durations-played.test.ts, cover.test.ts).

use autoreverse_server::library::paths::set_library_paths;
use axum::http::{StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{
    Seed, TempLibrary, TestApp, body_json, header_str, seed, seed_progress, seed_track,
};

async fn signed_in(db: PgPool) -> (TestApp, String, i32) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    (app, token, user)
}

fn titles(body: &Value) -> Vec<&str> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect()
}

#[sqlx::test]
async fn items_require_a_session(db: PgPool) {
    let app = TestApp::new(db);
    let response = app.get("/items", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["detail"], "Kein Token");
}

#[sqlx::test]
async fn sort_added_returns_the_newest_first(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let at = |s: &str| Some(s.parse::<chrono::DateTime<chrono::Utc>>().unwrap());
    seed(
        app.db(),
        Seed {
            added_at: at("2020-01-01T00:00:00Z"),
            ..Seed::new("album", "Alpha")
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            added_at: at("2026-01-01T00:00:00Z"),
            ..Seed::new("album", "Zeta")
        },
    )
    .await;
    let body = body_json(app.get("/items?kind=album&sort=added", Some(&token)).await).await;
    assert_eq!(titles(&body), ["Zeta", "Alpha"]);
}

#[sqlx::test]
async fn an_unknown_sort_is_refused(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let response = app.get("/items?sort=nonsense", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(response).await["detail"], "Unbekannte Sortierung");
}

#[sqlx::test]
async fn limit_out_of_range_is_a_422(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let response = app.get("/items?limit=501", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(response).await["detail"],
        "limit außerhalb des gültigen Bereichs"
    );
    let response = app.get("/items?offset=-1", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn the_list_has_the_summary_shape_without_cover_path_or_description(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    seed(
        app.db(),
        Seed {
            cover_path: Some("/covers/1.jpg"),
            description: Some("Das Meer schlägt zurück."),
            author: Some("Frank Schätzing"),
            series: Some("Meer"),
            ..Seed::new("book", "Der Schwarm")
        },
    )
    .await;
    let body = body_json(app.get("/items", Some(&token)).await).await;
    let item = &body["items"][0];
    let keys: Vec<_> = item
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut keys = keys;
    keys.sort();
    assert_eq!(
        keys,
        [
            "artist",
            "author",
            "has_cover",
            "id",
            "kind",
            "missing_since",
            "published_at",
            "series",
            "title",
            "year"
        ]
    );
    assert_eq!(item["has_cover"], true);
    assert_eq!(item["kind"], "book");
    assert_eq!(item["author"], "Frank Schätzing");
    assert_eq!(item["missing_since"], Value::Null);
}

#[sqlx::test]
async fn the_list_filters_by_kind_search_and_missing(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    seed(app.db(), Seed::new("album", "Nordlicht")).await;
    seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    seed(app.db(), Seed::new("book", "Eine Billion Dollar")).await;
    let get = |uri: &'static str| {
        let app = &app;
        let token = &token;
        async move { body_json(app.get(uri, Some(token)).await).await }
    };
    assert_eq!(
        titles(&get("/items?kind=book").await),
        ["Eine Billion Dollar"]
    );
    assert_eq!(
        titles(&get("/items?q=billion").await),
        ["Eine Billion Dollar"]
    );
    assert_eq!(
        titles(&get("/items?kind=album&missing=true").await),
        ["Weg"]
    );
    assert_eq!(
        titles(&get("/items?kind=album&missing=false").await),
        ["Nordlicht"]
    );
    assert_eq!(
        titles(&get("/items?kind=album").await),
        ["Nordlicht", "Weg"]
    );
    let missing = get("/items?missing=true").await;
    let stamp = missing["items"][0]["missing_since"].as_str().unwrap();
    assert!(stamp.ends_with('Z') && !stamp.contains('.'), "{stamp}");
}

#[sqlx::test]
async fn sort_played_orders_by_the_callers_progress(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let old = seed(app.db(), Seed::new("album", "Alt")).await;
    let new = seed(app.db(), Seed::new("album", "Neu")).await;
    seed(app.db(), Seed::new("album", "Nie gehört")).await;
    seed_progress(app.db(), user, old, 10.0, false).await;
    seed_progress(app.db(), user, new, 20.0, false).await;
    sqlx::query("UPDATE progress SET updated_at = now() - interval '1 day' WHERE item_id = $1")
        .bind(old)
        .execute(app.db())
        .await
        .unwrap();
    let body = body_json(app.get("/items?kind=album&sort=played", Some(&token)).await).await;
    assert_eq!(titles(&body), ["Neu", "Alt", "Nie gehört"]);
}

#[sqlx::test]
async fn an_unknown_item_is_a_404_with_the_german_message(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for uri in ["/items/999", "/items/abc", "/items/999/cover"] {
        let response = app.get(uri, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(
            body_json(response).await["detail"],
            "Unbekanntes Item",
            "{uri}"
        );
    }
}

#[sqlx::test]
async fn the_detail_carries_description_tracks_and_chapters(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let id = seed(
        app.db(),
        Seed {
            description: Some("Das Meer schlägt zurück."),
            ..Seed::new("book", "Der Schwarm")
        },
    )
    .await;
    seed_track(app.db(), id, 1, "/a", 42.0).await;
    sqlx::query("INSERT INTO chapters (item_id, position, title, start, \"end\") VALUES ($1, 1, 'Prolog', 0, 42)")
        .bind(id)
        .execute(app.db())
        .await
        .unwrap();
    let body = body_json(app.get(&format!("/items/{id}"), Some(&token)).await).await;
    assert_eq!(body["description"], "Das Meer schlägt zurück.");
    assert_eq!(body["title"], "Der Schwarm");
    assert_eq!(body["tracks"][0]["duration"], 42.0);
    assert_eq!(body["tracks"][0]["position"], 1);
    assert_eq!(
        body["chapters"],
        json!([{ "title": "Prolog", "start": 0.0, "end": 42.0 }])
    );
}

#[sqlx::test]
async fn an_item_without_chapters_has_an_empty_list(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let id = seed(app.db(), Seed::new("album", "X")).await;
    seed_track(app.db(), id, 1, "/a", 42.0).await;
    let body = body_json(app.get(&format!("/items/{id}"), Some(&token)).await).await;
    assert_eq!(body["tracks"].as_array().unwrap().len(), 1);
    assert_eq!(body["chapters"], json!([]));
}

async fn cover_app(db: PgPool, library: &TempLibrary) -> (TestApp, String) {
    let app = TestApp::with_data_dir(db, &library.data());
    set_library_paths(
        app.db(),
        &TempLibrary::path_str(&library.books()),
        &TempLibrary::path_str(&library.music()),
    )
    .await
    .unwrap();
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    (app, token)
}

#[sqlx::test]
async fn serves_an_in_root_cover_with_type_and_cache_header(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token) = cover_app(db, &library).await;
    let covers = library.data().join("covers");
    std::fs::create_dir_all(&covers).unwrap();
    let path = covers.join("1.png");
    std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47]).unwrap();
    let id = seed(
        app.db(),
        Seed {
            cover_path: path.to_str(),
            ..Seed::new("album", "X")
        },
    )
    .await;
    let response = app.get(&format!("/items/{id}/cover"), Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert_eq!(
        header_str(&response, header::CACHE_CONTROL),
        "private, max-age=86400"
    );
    assert_eq!(header_str(&response, header::CONTENT_LENGTH), "4");
}

#[sqlx::test]
async fn serves_a_cover_from_inside_the_books_root(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token) = cover_app(db, &library).await;
    let path = library.books().join("A/B/cover.jpg");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, [0xff, 0xd8, 0xff]).unwrap();
    let id = seed(
        app.db(),
        Seed {
            cover_path: path.to_str(),
            ..Seed::new("book", "B")
        },
    )
    .await;
    let response = app.get(&format!("/items/{id}/cover"), Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/jpeg");
}

#[sqlx::test]
async fn an_episode_without_artwork_gets_its_podcasts(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token) = cover_app(db, &library).await;
    let covers = library.data().join("covers");
    std::fs::create_dir_all(&covers).unwrap();
    let path = covers.join("show.png");
    std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47]).unwrap();
    let podcast = seed(
        app.db(),
        Seed {
            cover_path: path.to_str(),
            ..Seed::new("podcast", "Apfelfunk")
        },
    )
    .await;
    let episode = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Folge 500")
        },
    )
    .await;
    let response = app
        .get(&format!("/items/{episode}/cover"), Some(&token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
}

#[sqlx::test]
async fn refuses_a_cover_outside_the_roots_or_one_that_is_gone(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token) = cover_app(db, &library).await;
    let outside = seed(
        app.db(),
        Seed {
            cover_path: Some("/etc/passwd"),
            ..Seed::new("album", "X")
        },
    )
    .await;
    let escape = format!("{}/../../../etc/passwd", library.books().display());
    let sneaky = seed(
        app.db(),
        Seed {
            cover_path: Some(&escape),
            ..Seed::new("album", "Y")
        },
    )
    .await;
    let gone_path = format!("{}/weg.jpg", library.books().display());
    let gone = seed(
        app.db(),
        Seed {
            cover_path: Some(&gone_path),
            ..Seed::new("album", "Z")
        },
    )
    .await;
    let none = seed(app.db(), Seed::new("album", "Ohne")).await;
    for id in [outside, sneaky, gone, none] {
        let response = app.get(&format!("/items/{id}/cover"), Some(&token)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(response).await["detail"], "Kein Cover");
    }
}
