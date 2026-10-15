//! Playlists over the API and their gapless positions (legacy playlists.test.ts, the playlist
//! case of queries-aggregate.test.ts, and routes/playlists/**).

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

use autoreverse_server::library::playlists::{
    EntryTarget, append_entry, create_playlist, move_entry, playlist_overview, remove_entry,
};

use crate::common::{Seed, TestApp, body_text, seed, seed_track};

async fn signed_in(db: PgPool) -> (TestApp, String, i32) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    (app, token, user)
}

async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: &str,
) -> (StatusCode, Value) {
    let response = app.call(method, uri, body, Some(token)).await;
    let status = response.status();
    let text = body_text(response).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn positions(app: &TestApp, playlist_id: i32) -> Vec<(i32, i32)> {
    sqlx::query_as(
        "SELECT id, position FROM playlist_entries WHERE playlist_id = $1 ORDER BY position",
    )
    .bind(playlist_id)
    .fetch_all(app.db())
    .await
    .unwrap()
}

/// A playlist of `n` entries, all pointing at one album; returns the playlist and entry ids.
async fn playlist_with(app: &TestApp, user: i32, n: usize) -> (i32, Vec<i32>) {
    let playlist = create_playlist(app.db(), user, "X").await.unwrap();
    let album = seed(app.db(), Seed::new("album", "A")).await;
    let mut ids = Vec::new();
    for _ in 0..n {
        let entry = append_entry(app.db(), playlist.id, EntryTarget::Item(album))
            .await
            .unwrap();
        ids.push(entry.id);
    }
    (playlist.id, ids)
}

// --- gapless positions (legacy playlists.test.ts) ----------------------------------------------

#[sqlx::test]
async fn append_assigns_1_2_3(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let (playlist, ids) = playlist_with(&app, user, 3).await;
    assert_eq!(
        positions(&app, playlist).await,
        vec![(ids[0], 1), (ids[1], 2), (ids[2], 3)]
    );
}

#[sqlx::test]
async fn remove_closes_the_gap(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let (playlist, ids) = playlist_with(&app, user, 3).await;
    assert!(remove_entry(app.db(), playlist, ids[1]).await.unwrap());
    assert_eq!(
        positions(&app, playlist).await,
        vec![(ids[0], 1), (ids[2], 2)]
    );
    // Gone already: nothing to remove.
    assert!(!remove_entry(app.db(), playlist, ids[1]).await.unwrap());
}

#[sqlx::test]
async fn move_earlier_shifts_the_range_from_new_to_old_down_the_list(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let (playlist, ids) = playlist_with(&app, user, 3).await;
    assert!(move_entry(app.db(), playlist, ids[2], 1).await.unwrap());
    assert_eq!(
        positions(&app, playlist).await,
        vec![(ids[2], 1), (ids[0], 2), (ids[1], 3)]
    );
}

#[sqlx::test]
async fn move_later_shifts_the_range_from_old_to_new_up_the_list(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let (playlist, ids) = playlist_with(&app, user, 3).await;
    assert!(move_entry(app.db(), playlist, ids[0], 3).await.unwrap());
    assert_eq!(
        positions(&app, playlist).await,
        vec![(ids[1], 1), (ids[2], 2), (ids[0], 3)]
    );
}

#[sqlx::test]
async fn move_to_the_same_position_is_a_no_op(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let (playlist, ids) = playlist_with(&app, user, 1).await;
    assert!(move_entry(app.db(), playlist, ids[0], 1).await.unwrap());
    assert_eq!(positions(&app, playlist).await, vec![(ids[0], 1)]);
}

#[sqlx::test]
async fn remove_after_a_reorder_keeps_positions_gapless_without_a_unique_violation(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let (playlist, ids) = playlist_with(&app, user, 5).await;
    // Physical row order now differs from position order.
    move_entry(app.db(), playlist, ids[4], 2).await.unwrap();
    assert_eq!(
        positions(&app, playlist)
            .await
            .iter()
            .map(|p| p.1)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    remove_entry(app.db(), playlist, ids[2]).await.unwrap();
    assert_eq!(
        positions(&app, playlist).await,
        vec![(ids[0], 1), (ids[4], 2), (ids[1], 3), (ids[3], 4)]
    );
}

#[sqlx::test]
async fn concurrent_appends_get_distinct_positions(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let (playlist, _) = playlist_with(&app, user, 0).await;
    let album = seed(app.db(), Seed::new("album", "B")).await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let db = app.db().clone();
        tasks.spawn(async move { append_entry(&db, playlist, EntryTarget::Item(album)).await });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap().unwrap();
    }
    assert_eq!(
        positions(&app, playlist)
            .await
            .iter()
            .map(|p| p.1)
            .collect::<Vec<_>>(),
        (1..=8).collect::<Vec<_>>()
    );
}

#[sqlx::test]
async fn overview_sums_single_track_and_whole_item_entries(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let playlist = create_playlist(app.db(), user, "Fahrtwind").await.unwrap();
    let album = seed(app.db(), Seed::new("album", "X")).await;
    let track = seed_track(app.db(), album, 1, "/a", 100.0).await;
    seed_track(app.db(), album, 2, "/b", 50.0).await;
    append_entry(app.db(), playlist.id, EntryTarget::Track(track))
        .await
        .unwrap();
    append_entry(app.db(), playlist.id, EntryTarget::Item(album))
        .await
        .unwrap();
    let overview = playlist_overview(app.db(), user).await.unwrap();
    assert_eq!(overview.len(), 1);
    assert_eq!(overview[0].entry_count, 2);
    assert_eq!(overview[0].duration, 250.0);
}

// --- API -----------------------------------------------------------------------------------------

#[sqlx::test]
async fn every_route_needs_a_user(db: PgPool) {
    let app = TestApp::new(db);
    for (method, uri) in [
        (Method::GET, "/playlists"),
        (Method::POST, "/playlists"),
        (Method::GET, "/playlists/1"),
        (Method::PATCH, "/playlists/1"),
        (Method::DELETE, "/playlists/1"),
        (Method::POST, "/playlists/1/entries"),
        (Method::PUT, "/playlists/1/entries/1"),
        (Method::DELETE, "/playlists/1/entries/1"),
    ] {
        let response = app.call(method.clone(), uri, None, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
    }
}

#[sqlx::test]
async fn create_list_rename_and_delete(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let (status, created) = call(
        &app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "Zug"})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["name"], "Zug");
    assert_eq!(created["entry_count"], 0);
    let created_at = created["created_at"].as_str().unwrap();
    assert!(
        created_at.ends_with('Z') && !created_at.contains('.'),
        "{created_at}"
    );
    let id = created["id"].as_i64().unwrap();

    call(
        &app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "auto"})),
        &token,
    )
    .await;
    let (_, list) = call(&app, Method::GET, "/playlists", None, &token).await;
    let names: Vec<&str> = list["playlists"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    // By lower-cased name.
    assert_eq!(names, ["auto", "Zug"]);

    let album = seed(app.db(), Seed::new("album", "A")).await;
    call(
        &app,
        Method::POST,
        &format!("/playlists/{id}/entries"),
        Some(json!({"item_id": album})),
        &token,
    )
    .await;
    let (status, renamed) = call(
        &app,
        Method::PATCH,
        &format!("/playlists/{id}"),
        Some(json!({"name": "Bahn"})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "Bahn");
    assert_eq!(renamed["entry_count"], 1);
    assert_eq!(renamed["created_at"], created["created_at"]);

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/playlists/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = call(&app, Method::GET, &format!("/playlists/{id}"), None, &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["detail"], "Unbekannte Playlist");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM playlist_entries")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(left, 0, "entries go with their playlist");
}

#[sqlx::test]
async fn names_must_be_1_to_100_characters(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let id = call(
        &app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "ok"})),
        &token,
    )
    .await
    .1["id"]
        .as_i64()
        .unwrap();
    for body in [
        json!({}),
        json!({"name": ""}),
        json!({"name": 5}),
        json!({"name": "x".repeat(101)}),
    ] {
        for (method, uri) in [
            (Method::POST, "/playlists".to_owned()),
            (Method::PATCH, format!("/playlists/{id}")),
        ] {
            let (status, out) = call(&app, method.clone(), &uri, Some(body.clone()), &token).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{method} {body}");
            assert_eq!(out["detail"], "name muss 1–100 Zeichen haben");
        }
    }
    let (status, _) = call(
        &app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "x".repeat(100)})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response = app.post_raw("/playlists", "{nope", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_text(response).await,
        r#"{"detail":"Ungültiger JSON-Body"}"#
    );
}

#[sqlx::test]
async fn someone_elses_playlist_is_unknown_even_to_an_admin(db: PgPool) {
    let app = TestApp::new(db);
    let owner = app.user("oliver", false).await;
    let admin = app.user("root", true).await;
    let token = app.token(admin).await;
    let playlist = create_playlist(app.db(), owner, "Privat").await.unwrap();
    let album = seed(app.db(), Seed::new("album", "A")).await;
    let entry = append_entry(app.db(), playlist.id, EntryTarget::Item(album))
        .await
        .unwrap();
    let id = playlist.id;
    for (method, uri, body) in [
        (Method::GET, format!("/playlists/{id}"), None),
        (
            Method::PATCH,
            format!("/playlists/{id}"),
            Some(json!({"name": "Meins"})),
        ),
        (Method::DELETE, format!("/playlists/{id}"), None),
        (
            Method::POST,
            format!("/playlists/{id}/entries"),
            Some(json!({"item_id": album})),
        ),
        (
            Method::PUT,
            format!("/playlists/{id}/entries/{}", entry.id),
            Some(json!({"position": 1})),
        ),
        (
            Method::DELETE,
            format!("/playlists/{id}/entries/{}", entry.id),
            None,
        ),
        (Method::GET, "/playlists/abc".to_owned(), None),
        (Method::GET, "/playlists/999999".to_owned(), None),
    ] {
        let (status, out) = call(&app, method.clone(), &uri, body, &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert_eq!(out["detail"], "Unbekannte Playlist", "{method} {uri}");
    }
    let (_, list) = call(&app, Method::GET, "/playlists", None, &token).await;
    assert_eq!(list["playlists"], json!([]));
    // Untouched.
    let name: String = sqlx::query_scalar("SELECT name FROM playlists WHERE id = $1")
        .bind(id)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(name, "Privat");
    assert_eq!(positions(&app, id).await, vec![(entry.id, 1)]);
}

#[sqlx::test]
async fn entries_name_exactly_one_known_item_or_track(db: PgPool) {
    let (app, token, _) = signed_in(db).await;
    let id = call(
        &app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "P"})),
        &token,
    )
    .await
    .1["id"]
        .as_i64()
        .unwrap();
    let uri = format!("/playlists/{id}/entries");
    let album = seed(
        app.db(),
        Seed {
            artist: Some("Kraftwerk"),
            ..Seed::new("album", "Radio-Aktivität")
        },
    )
    .await;
    let track = seed_track(app.db(), album, 1, "/a.mp3", 120.5).await;
    sqlx::query("UPDATE tracks SET title = 'Antenne' WHERE id = $1")
        .bind(track)
        .execute(app.db())
        .await
        .unwrap();

    for body in [
        json!({}),
        json!({"item_id": null, "track_id": null}),
        json!({"item_id": album, "track_id": track}),
    ] {
        let (status, out) = call(&app, Method::POST, &uri, Some(body.clone()), &token).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(out["detail"], "Genau eines von item_id/track_id angeben");
    }
    for (body, detail) in [
        (json!({"item_id": 999999}), "Unbekanntes Item"),
        (json!({"item_id": "abc"}), "Unbekanntes Item"),
        (json!({"track_id": 999999}), "Unbekannter Track"),
        (json!({"track_id": 1.5}), "Unbekannter Track"),
    ] {
        let (status, out) = call(&app, Method::POST, &uri, Some(body.clone()), &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(out["detail"], detail);
    }

    let (status, item_entry) = call(
        &app,
        Method::POST,
        &uri,
        Some(json!({"item_id": album})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(item_entry["position"], 1);
    assert_eq!(item_entry["track"], Value::Null);
    assert_eq!(item_entry["item"]["id"], album);
    assert_eq!(item_entry["item"]["title"], "Radio-Aktivität");
    assert_eq!(item_entry["item"]["artist"], "Kraftwerk");
    assert_eq!(item_entry["item"]["has_cover"], false);

    let (_, track_entry) = call(
        &app,
        Method::POST,
        &uri,
        Some(json!({"track_id": track, "item_id": null})),
        &token,
    )
    .await;
    assert_eq!(track_entry["position"], 2);
    assert_eq!(track_entry["item"], Value::Null);
    assert_eq!(
        track_entry["track"],
        json!({
            "id": track,
            "title": "Antenne",
            "duration": 120.5,
            "item_id": album,
            "item_title": "Radio-Aktivität",
            "item_kind": "album"
        })
    );

    let (status, detail) = call(&app, Method::GET, &format!("/playlists/{id}"), None, &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["name"], "P");
    assert_eq!(detail["entries"], json!([item_entry, track_entry]));
}

#[sqlx::test]
async fn reordering_over_the_api(db: PgPool) {
    let (app, token, user) = signed_in(db).await;
    let (playlist, ids) = playlist_with(&app, user, 3).await;
    let uri = |entry: i32| format!("/playlists/{playlist}/entries/{entry}");

    let (status, _) = call(
        &app,
        Method::PUT,
        &uri(ids[0]),
        Some(json!({"position": 3})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        positions(&app, playlist).await,
        vec![(ids[1], 1), (ids[2], 2), (ids[0], 3)]
    );

    for body in [
        json!({"position": 0}),
        json!({"position": 4}),
        json!({"position": 1.5}),
        json!({"position": "2"}),
        json!({}),
    ] {
        let (status, out) = call(&app, Method::PUT, &uri(ids[0]), Some(body.clone()), &token).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(out["detail"], "position außerhalb des gültigen Bereichs");
    }

    // An entry of another playlist of the same user is not this playlist's.
    let (other, other_ids) = playlist_with(&app, user, 1).await;
    for (method, body) in [
        (Method::PUT, Some(json!({"position": 1}))),
        (Method::DELETE, None),
    ] {
        for entry_uri in [
            uri(other_ids[0]),
            format!("/playlists/{playlist}/entries/abc"),
        ] {
            let (status, out) = call(&app, method.clone(), &entry_uri, body.clone(), &token).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {entry_uri}");
            assert_eq!(out["detail"], "Unbekannter Eintrag");
        }
    }
    assert_eq!(positions(&app, other).await, vec![(other_ids[0], 1)]);

    let (status, _) = call(&app, Method::DELETE, &uri(ids[1]), None, &token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        positions(&app, playlist).await,
        vec![(ids[2], 1), (ids[0], 2)]
    );
}
