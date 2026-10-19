//! Favourites, bookmarks and search over the API (legacy favorites-bookmarks-prefs.test.ts,
//! search-api.test.ts, and the search/favourite cases of queries-core/queries-aggregate).

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

use autoreverse_server::library::bookmarks::{add_bookmark, count_bookmarks};
use autoreverse_server::library::favorites::{add_item_favorite, is_item_favorite};
use autoreverse_server::library::queries::{search_artists, search_items};

use crate::common::{Seed, TestApp, seed, seed_track};

async fn signed_in(db: PgPool) -> (TestApp, String, i32) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    (app, token, user)
}

async fn status_and_json(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: &str,
) -> (StatusCode, Value) {
    let response = app.call(method, uri, body, Some(token)).await;
    let status = response.status();
    let text = crate::common::body_text(response).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

#[sqlx::test]
async fn every_route_needs_a_user(db: PgPool) {
    let app = TestApp::new(db);
    for (method, uri) in [
        (Method::GET, "/favorites"),
        (Method::POST, "/favorites/items/1"),
        (Method::DELETE, "/favorites/items/1"),
        (Method::POST, "/favorites/tracks/1"),
        (Method::DELETE, "/favorites/tracks/1"),
        (Method::GET, "/bookmarks"),
        (Method::POST, "/bookmarks"),
        (Method::DELETE, "/bookmarks/1"),
        (Method::GET, "/search?q=x"),
    ] {
        let response = app.call(method.clone(), uri, None, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
    }
}

// --- favourites --------------------------------------------------------------------------------

#[sqlx::test]
async fn favoriting_twice_is_idempotent_unfavoriting_a_non_favorite_is_a_no_op(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let album = seed(app.db(), Seed::new("album", "X")).await;
    let uri = format!("/favorites/items/{album}");
    for _ in 0..2 {
        let (status, _) = status_and_json(&app, Method::POST, &uri, None, &token).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (_, body) = status_and_json(&app, Method::GET, "/favorites", None, &token).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    for uri in [
        "/favorites/items/999999",
        "/favorites/items/abc",
        "/favorites/tracks/999999",
    ] {
        let (status, _) = status_and_json(&app, Method::DELETE, uri, None, &token).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{uri}");
    }
}

#[sqlx::test]
async fn favoriting_something_unknown_is_404(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for (uri, detail) in [
        ("/favorites/items/999999", "Unbekanntes Item"),
        ("/favorites/items/abc", "Unbekanntes Item"),
        ("/favorites/tracks/999999", "Unbekannter Track"),
        ("/favorites/tracks/abc", "Unbekannter Track"),
    ] {
        let (status, body) = status_and_json(&app, Method::POST, uri, None, &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body["detail"], detail, "{uri}");
    }
}

#[sqlx::test]
async fn the_list_has_items_and_tracks_in_title_order_without_missing_ones(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let other = app.user("anna", false).await;
    let ten = seed(
        app.db(),
        Seed {
            artist: Some("Kraftwerk"),
            cover_path: Some("/c.jpg"),
            ..Seed::new("album", "Teil 10")
        },
    )
    .await;
    let two = seed(app.db(), Seed::new("book", "Teil 2")).await;
    let gone = seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    let track = seed_track(app.db(), ten, 1, "/k/1.mp3", 120.0).await;
    sqlx::query("UPDATE tracks SET title = 'Autobahn' WHERE id = $1")
        .bind(track)
        .execute(app.db())
        .await
        .unwrap();
    let gone_track = seed_track(app.db(), gone, 1, "/w/1.mp3", 60.0).await;
    for uri in [
        format!("/favorites/items/{ten}"),
        format!("/favorites/items/{two}"),
        format!("/favorites/items/{gone}"),
        format!("/favorites/tracks/{track}"),
        format!("/favorites/tracks/{gone_track}"),
    ] {
        let (status, _) = status_and_json(&app, Method::POST, &uri, None, &token).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    // Someone else's favourite stays theirs.
    add_item_favorite(app.db(), other, gone).await.unwrap();

    let (status, body) = status_and_json(&app, Method::GET, "/favorites", None, &token).await;
    assert_eq!(status, StatusCode::OK);
    let titles: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Teil 2", "Teil 10"]);
    assert_eq!(
        body["items"][1],
        json!({
            "id": ten, "kind": "album", "title": "Teil 10", "has_cover": true, "author": null,
            "artist": "Kraftwerk", "series": null, "year": null, "missing_since": null,
            "published_at": null
        })
    );
    assert_eq!(
        body["tracks"],
        json!([{
            "id": track, "title": "Autobahn", "artist": null, "duration": 120, "item_id": ten,
            "item_title": "Teil 10", "item_kind": "album"
        }])
    );

    let (status, _) = status_and_json(
        &app,
        Method::DELETE,
        &format!("/favorites/tracks/{track}"),
        None,
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = status_and_json(&app, Method::GET, "/favorites", None, &token).await;
    assert_eq!(body["tracks"], json!([]));
}

#[sqlx::test]
async fn is_item_favorite_detects_favorited_items(db: PgPool) {
    let (app, _, user) = signed_in(db).await;
    let item = seed(app.db(), Seed::new("album", "X")).await;
    assert!(!is_item_favorite(app.db(), user, item).await.unwrap());
    add_item_favorite(app.db(), user, item).await.unwrap();
    assert!(is_item_favorite(app.db(), user, item).await.unwrap());
}

// --- bookmarks ---------------------------------------------------------------------------------

#[sqlx::test]
async fn post_bookmarks_422s_on_out_of_bounds_position_or_title_201s_otherwise(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let long = "x".repeat(201);
    // 101 emoji are 202 UTF-16 units — JS `length`, which legacy checked.
    let emoji = "🎧".repeat(101);
    for (body, detail) in [
        (
            json!({"item_id": book, "position": -1, "title": "x"}),
            "position muss ≥ 0 sein",
        ),
        (
            json!({"item_id": book, "title": "x"}),
            "position muss ≥ 0 sein",
        ),
        (
            json!({"item_id": book, "position": "3", "title": "x"}),
            "position muss ≥ 0 sein",
        ),
        (
            json!({"item_id": book, "position": 1, "title": ""}),
            "title muss 1–200 Zeichen haben",
        ),
        (
            json!({"item_id": book, "position": 1, "title": long}),
            "title muss 1–200 Zeichen haben",
        ),
        (
            json!({"item_id": book, "position": 1, "title": emoji}),
            "title muss 1–200 Zeichen haben",
        ),
        (
            json!({"item_id": book, "position": 1}),
            "title muss 1–200 Zeichen haben",
        ),
        (
            json!({"item_id": "1", "position": 1, "title": "x"}),
            "item_id muss eine Zahl sein",
        ),
        (
            json!({"position": 1, "title": "x"}),
            "item_id muss eine Zahl sein",
        ),
    ] {
        let (status, out) =
            status_and_json(&app, Method::POST, "/bookmarks", Some(body.clone()), &token).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(out["detail"], detail, "{body}");
    }
    let response = app.post_raw("/bookmarks", "{", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let (status, out) = status_and_json(
        &app,
        Method::POST,
        "/bookmarks",
        Some(json!({"item_id": book, "position": 12.5, "title": "Kapitel 3"})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(out["item_id"], book);
    assert_eq!(out["position"], 12.5);
    assert_eq!(out["title"], "Kapitel 3");
    assert!(out["id"].as_i64().is_some());

    let (status, _) = status_and_json(
        &app,
        Method::POST,
        "/bookmarks",
        Some(json!({"item_id": book, "position": 0, "title": "🎧".repeat(100)})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[sqlx::test]
async fn bookmarking_an_unknown_item_is_404(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for item_id in [json!(999999), json!(1.5)] {
        let (status, out) = status_and_json(
            &app,
            Method::POST,
            "/bookmarks",
            Some(json!({"item_id": item_id, "position": 1, "title": "x"})),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{item_id}");
        assert_eq!(out["detail"], "Unbekanntes Item");
    }
}

#[sqlx::test]
async fn the_same_position_twice_renames_the_bookmark(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let mut ids = Vec::new();
    for title in ["Alt", "Neu"] {
        let (_, out) = status_and_json(
            &app,
            Method::POST,
            "/bookmarks",
            Some(json!({"item_id": book, "position": 60, "title": title})),
            &token,
        )
        .await;
        ids.push(out["id"].clone());
        assert_eq!(out["title"], title);
    }
    assert_eq!(ids[0], ids[1]);
    assert_eq!(count_bookmarks(app.db(), user).await.unwrap(), 1);
}

#[sqlx::test]
async fn get_bookmarks_lists_all_or_one_item_and_only_ones_own(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let other = app.user("anna", false).await;
    let b = seed(app.db(), Seed::new("book", "Beta")).await;
    let a = seed(app.db(), Seed::new("book", "alpha")).await;
    add_bookmark(app.db(), user, b, 5.0, "b5").await.unwrap();
    add_bookmark(app.db(), user, a, 90.0, "a90").await.unwrap();
    add_bookmark(app.db(), user, a, 30.0, "a30").await.unwrap();
    add_bookmark(app.db(), other, a, 1.0, "fremd")
        .await
        .unwrap();

    let (status, body) = status_and_json(&app, Method::GET, "/bookmarks", None, &token).await;
    assert_eq!(status, StatusCode::OK);
    let titles: Vec<&str> = body["bookmarks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["a30", "a90", "b5"]);
    let first = &body["bookmarks"][0];
    assert_eq!(first["item_id"], a);
    assert_eq!(first["position"], json!(30));
    assert_eq!(first.as_object().unwrap().len(), 4);

    let (_, body) = status_and_json(
        &app,
        Method::GET,
        &format!("/bookmarks?item_id={b}"),
        None,
        &token,
    )
    .await;
    assert_eq!(body["bookmarks"].as_array().unwrap().len(), 1);
    assert_eq!(body["bookmarks"][0]["title"], "b5");

    let (status, body) =
        status_and_json(&app, Method::GET, "/bookmarks?item_id=abc", None, &token).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"], "item_id muss eine Zahl sein");
}

#[sqlx::test]
async fn delete_bookmark_404s_uniformly_for_missing_and_not_owned(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let other = app.user("anna", false).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    let foreign = add_bookmark(app.db(), other, book, 1.0, "fremd")
        .await
        .unwrap();
    let own = add_bookmark(app.db(), user, book, 2.0, "meins")
        .await
        .unwrap();
    for uri in [
        "/bookmarks/999999".to_owned(),
        "/bookmarks/abc".to_owned(),
        format!("/bookmarks/{}", foreign.id),
    ] {
        let (status, body) = status_and_json(&app, Method::DELETE, &uri, None, &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body["detail"], "Unbekanntes Lesezeichen");
    }
    let uri = format!("/bookmarks/{}", own.id);
    let (status, _) = status_and_json(&app, Method::DELETE, &uri, None, &token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = status_and_json(&app, Method::DELETE, &uri, None, &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(count_bookmarks(app.db(), other).await.unwrap(), 1);
}

#[sqlx::test]
async fn count_bookmarks_counts(db: PgPool) {
    let (app, _, user) = signed_in(db).await;
    let book = seed(app.db(), Seed::new("book", "X")).await;
    assert_eq!(count_bookmarks(app.db(), user).await.unwrap(), 0);
    add_bookmark(app.db(), user, book, 10.0, "Chapter 1")
        .await
        .unwrap();
    assert_eq!(count_bookmarks(app.db(), user).await.unwrap(), 1);
    add_bookmark(app.db(), user, book, 20.0, "Chapter 2")
        .await
        .unwrap();
    assert_eq!(count_bookmarks(app.db(), user).await.unwrap(), 2);
}

// --- search ------------------------------------------------------------------------------------

#[sqlx::test]
async fn search_groups_results_by_kind_podcasts_include_episodes(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    seed(app.db(), Seed::new("book", "Eine Billion Dollar")).await;
    let podcast = seed(app.db(), Seed::new("podcast", "eine Sendung")).await;
    seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "eine Folge")
        },
    )
    .await;
    let (status, body) = status_and_json(&app, Method::GET, "/search?q=eine", None, &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["books"].as_array().unwrap().len(), 1);
    assert_eq!(body["podcasts"].as_array().unwrap().len(), 2);
    assert_eq!(body["albums"], json!([]));
    assert_eq!(body["tracks"], json!([]));
}

#[sqlx::test]
async fn search_matches_author_and_artist_finds_tracks_and_hides_missing_items(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    seed(
        app.db(),
        Seed {
            author: Some("Andreas Eschbach"),
            ..Seed::new("book", "Das Jesus-Video")
        },
    )
    .await;
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Eschbach Ensemble"),
            ..Seed::new("album", "Live")
        },
    )
    .await;
    let gone = seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("book", "Eschbach weg")
        },
    )
    .await;
    let track = seed_track(app.db(), album, 1, "/l/1.mp3", 61.5).await;
    let gone_track = seed_track(app.db(), gone, 1, "/w/1.mp3", 1.0).await;
    for (id, title) in [(track, "Eschbach-Suite"), (gone_track, "Eschbach weg")] {
        sqlx::query("UPDATE tracks SET title = $2 WHERE id = $1")
            .bind(id)
            .bind(title)
            .execute(app.db())
            .await
            .unwrap();
    }
    let (_, body) = status_and_json(&app, Method::GET, "/search?q=ESCHBACH", None, &token).await;
    assert_eq!(body["books"][0]["title"], "Das Jesus-Video");
    assert_eq!(body["books"].as_array().unwrap().len(), 1);
    assert_eq!(body["albums"][0]["title"], "Live");
    assert_eq!(
        body["tracks"],
        json!([{
            "id": track, "title": "Eschbach-Suite", "artist": null, "duration": 61.5, "item_id": album,
            "item_title": "Live", "item_kind": "album"
        }])
    );
}

#[sqlx::test]
async fn search_validates_q_and_limit_and_escapes_wildcards(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    for i in 0..3 {
        seed(app.db(), Seed::new("book", &format!("Buch {i}"))).await;
    }
    let long = "x".repeat(201);
    for (uri, detail) in [
        ("/search".to_owned(), "q muss 1–200 Zeichen haben"),
        ("/search?q=".to_owned(), "q muss 1–200 Zeichen haben"),
        (format!("/search?q={long}"), "q muss 1–200 Zeichen haben"),
        (
            "/search?q=Buch&limit=0".to_owned(),
            "limit außerhalb des gültigen Bereichs",
        ),
        (
            "/search?q=Buch&limit=101".to_owned(),
            "limit außerhalb des gültigen Bereichs",
        ),
    ] {
        let (status, body) = status_and_json(&app, Method::GET, &uri, None, &token).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
        assert_eq!(body["detail"], detail, "{uri}");
    }
    let (_, body) =
        status_and_json(&app, Method::GET, "/search?q=Buch&limit=2", None, &token).await;
    let titles: Vec<&str> = body["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Buch 0", "Buch 1"]);
    // `%` and `_` are literal text, not wildcards.
    for q in ["%25", "_"] {
        let (_, body) =
            status_and_json(&app, Method::GET, &format!("/search?q={q}"), None, &token).await;
        assert_eq!(body["books"], json!([]), "{q}");
    }
}

#[sqlx::test]
async fn search_artists_groups_album_artists_and_book_authors_separately(db: PgPool) {
    let app = TestApp::new(db);
    seed(
        app.db(),
        Seed {
            artist: Some("Ansa Volt"),
            ..Seed::new("album", "X")
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            author: Some("Ansa Volt"),
            ..Seed::new("book", "Y")
        },
    )
    .await;
    seed(
        app.db(),
        Seed {
            author: Some("Ansa Volt"),
            ..Seed::new("book", "Z")
        },
    )
    .await;
    let rows = search_artists(app.db(), "ansa", 20).await.unwrap();
    let mut roles: Vec<(&str, i32)> = rows
        .iter()
        .map(|r| (r.role.as_str(), r.work_count))
        .collect();
    roles.sort();
    assert_eq!(roles, [("artist", 1), ("author", 2)]);
}

#[sqlx::test]
async fn search_items_hides_items_that_vanished_from_disk(db: PgPool) {
    let app = TestApp::new(db);
    seed(
        app.db(),
        Seed {
            missing: true,
            ..Seed::new("album", "Weg")
        },
    )
    .await;
    seed(app.db(), Seed::new("album", "Da")).await;
    assert!(
        search_items(app.db(), "Weg", &["album"], 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        search_items(app.db(), "Da", &["album"], 10)
            .await
            .unwrap()
            .len(),
        1
    );
}
