//! What playlists learned in Rust: description, folder and cover, sharing with the other accounts
//! of the server (who may read, who may change entries, what stays the owner's), `?scope=`,
//! export as M3U and import from it. The native apps are built against exactly this.

use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

use autoreverse_server::library::paths::set_library_paths;
use autoreverse_server::library::playlists::{
    EntryTarget, PlaylistChanges, append_entry, create_playlist, store_cover, update_playlist,
    visible_playlist,
};

use crate::artist_covers::multipart;
use crate::common::{Seed, TempLibrary, TestApp, body_json, body_text, header_str, seed};

const BOUNDARY: &str = "XyZBoundary";

/// A real picture, `edge` pixels square.
fn png(edge: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(edge, edge, image::Rgb([200, 40, 90]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
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

/// A request with any body and content type.
async fn send_raw(
    app: &TestApp,
    method: Method,
    uri: &str,
    content_type: &str,
    body: Vec<u8>,
    token: &str,
) -> Response<Body> {
    app.send(
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
}

struct Two {
    app: TestApp,
    library: TempLibrary,
    owner: i32,
    owner_token: String,
    other: i32,
    other_token: String,
}

/// Oskar, who owns the playlists, and Mara, another account of the same server.
async fn two_accounts(db: PgPool) -> Two {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let owner = app.user("oskar", true).await;
    let other = app.user("mara", false).await;
    Two {
        owner_token: app.token(owner).await,
        other_token: app.token(other).await,
        app,
        library,
        owner,
        other,
    }
}

async fn share(app: &TestApp, playlist: i32, shared: bool, collaborative: bool) {
    update_playlist(
        app.db(),
        playlist,
        &PlaylistChanges {
            shared: Some(shared),
            collaborative: Some(collaborative),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn track(db: &PgPool, item: i32, position: i32, path: &str, title: &str, secs: f64) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO tracks (item_id, position, path, duration, title)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(item)
    .bind(position)
    .bind(path)
    .bind(secs)
    .bind(title)
    .fetch_one(db)
    .await
    .unwrap()
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

// --- the playlist object, the list and its scope -------------------------------------------------

#[sqlx::test]
async fn a_playlist_carries_the_new_fields_everywhere_it_is_returned(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let (status, created) = call(
        app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "Zug"})),
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = created["id"].as_i64().unwrap();
    let fresh = json!({
        "id": id, "name": "Zug", "created_at": created["created_at"], "entry_count": 0,
        "description": null, "folder": null, "has_cover": false, "owner": "oskar",
        "mine": true, "shared": false, "collaborative": false, "can_edit": true, "duration": 0,
    });
    assert_eq!(created, fresh);

    let record = album(app.db(), "Air", "Moon Safari").await;
    let single = track(app.db(), record, 1, "/m/1.mp3", "Eins", 100.5).await;
    track(app.db(), record, 2, "/m/2.mp3", "Zwei", 50.0).await;
    append_entry(app.db(), id as i32, EntryTarget::Track(single))
        .await
        .unwrap();
    append_entry(app.db(), id as i32, EntryTarget::Item(record))
        .await
        .unwrap();

    let (status, patched) = call(
        app,
        Method::PATCH,
        &format!("/playlists/{id}"),
        Some(json!({"description": "Für lange Fahrten", "folder": " Reisen ", "shared": true})),
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut expected = fresh.clone();
    expected["entry_count"] = json!(2);
    expected["duration"] = json!(251);
    expected["description"] = json!("Für lange Fahrten");
    expected["folder"] = json!("Reisen");
    expected["shared"] = json!(true);
    assert_eq!(patched, expected, "the name stays when it is not sent");

    let (_, list) = call(app, Method::GET, "/playlists", None, &two.owner_token).await;
    assert_eq!(list, json!({"playlists": [expected]}));

    let (_, detail) = call(
        app,
        Method::GET,
        &format!("/playlists/{id}"),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(detail["entries"].as_array().unwrap().len(), 2);
    let mut without_entries = detail.clone();
    without_entries.as_object_mut().unwrap().remove("entries");
    assert_eq!(without_entries, expected);

    // The same playlist as the other account sees it.
    let (_, theirs) = call(
        app,
        Method::GET,
        &format!("/playlists/{id}"),
        None,
        &two.other_token,
    )
    .await;
    assert_eq!(theirs["owner"], "oskar");
    assert_eq!(theirs["mine"], false);
    assert_eq!(theirs["can_edit"], false);
    assert_eq!(theirs["shared"], true);

    // `collaborative` is reported as chosen, but gives nothing while the playlist is private.
    share(app, id as i32, false, true).await;
    let (_, list) = call(app, Method::GET, "/playlists", None, &two.owner_token).await;
    assert_eq!(list["playlists"][0]["collaborative"], true);
    assert_eq!(list["playlists"][0]["shared"], false);
    share(app, id as i32, true, true).await;
    let (_, theirs) = call(
        app,
        Method::GET,
        &format!("/playlists/{id}"),
        None,
        &two.other_token,
    )
    .await;
    assert_eq!(theirs["can_edit"], true);
    assert_eq!(theirs["mine"], false);
}

#[sqlx::test]
async fn an_episode_not_downloaded_counts_what_its_feed_claims(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let playlist = create_playlist(app.db(), two.owner, "Podcasts")
        .await
        .unwrap();
    let show = seed(app.db(), Seed::new("podcast", "Show")).await;
    let episode = seed(
        app.db(),
        Seed {
            parent_id: Some(show),
            ..Seed::new("episode", "Folge 1")
        },
    )
    .await;
    sqlx::query("UPDATE items SET feed_duration = 1800 WHERE id = $1")
        .bind(episode)
        .execute(app.db())
        .await
        .unwrap();
    append_entry(app.db(), playlist.id, EntryTarget::Item(episode))
        .await
        .unwrap();
    let (_, list) = call(app, Method::GET, "/playlists", None, &two.owner_token).await;
    assert_eq!(list["playlists"][0]["duration"], 1800);
}

#[sqlx::test]
async fn the_list_is_the_users_own_unless_a_scope_asks_for_more(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    for (user, name, shared) in [
        (two.owner, "beta", false),
        (two.owner, "Alpha", true),
        (two.other, "Zebra", false),
        (two.other, "Mittag", true),
        (two.other, "abend", true),
    ] {
        let playlist = create_playlist(app.db(), user, name).await.unwrap();
        share(app, playlist.id, shared, false).await;
    }
    let names = |body: &Value| -> Vec<String> {
        body["playlists"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_owned())
            .collect()
    };
    // An old client: no parameter, and nothing of anybody else, shared or not.
    let (status, plain) = call(app, Method::GET, "/playlists", None, &two.owner_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&plain), ["Alpha", "beta"]);
    assert!(
        plain["playlists"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["mine"] == true)
    );
    let (_, mine) = call(
        app,
        Method::GET,
        "/playlists?scope=mine",
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(mine, plain);

    let (_, shared) = call(
        app,
        Method::GET,
        "/playlists?scope=shared",
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(names(&shared), ["abend", "Mittag"]);
    assert_eq!(shared["playlists"][0]["owner"], "mara");
    assert_eq!(shared["playlists"][0]["mine"], false);

    let (_, all) = call(
        app,
        Method::GET,
        "/playlists?scope=all",
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(names(&all), ["Alpha", "beta", "abend", "Mittag"]);

    // Mara's side: her private one is hers alone, Oskar's shared one shows under `shared`.
    let (_, all) = call(
        app,
        Method::GET,
        "/playlists?scope=all",
        None,
        &two.other_token,
    )
    .await;
    assert_eq!(names(&all), ["abend", "Mittag", "Zebra", "Alpha"]);

    let (status, body) = call(
        app,
        Method::GET,
        "/playlists?scope=alle",
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"], "scope muss mine, shared oder all sein");
}

// --- who may do what -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Need {
    Read,
    Edit,
    Own,
}

/// A playlist of Oskar's with two entries and a cover; returns it, its entries and an album to
/// add.
async fn fresh(two: &Two, shared: bool, collaborative: bool) -> (i32, Vec<i32>, i32) {
    let app = &two.app;
    let playlist = create_playlist(app.db(), two.owner, "Liste").await.unwrap();
    share(app, playlist.id, shared, collaborative).await;
    let record = seed(app.db(), Seed::new("album", "A")).await;
    let mut entries = Vec::new();
    for _ in 0..2 {
        let entry = append_entry(app.db(), playlist.id, EntryTarget::Item(record))
            .await
            .unwrap();
        entries.push(entry.id);
    }
    store_cover(
        app.db(),
        &playlist,
        "image/png",
        8,
        &png(4),
        &app.state.config.playlists_dir,
    )
    .await
    .unwrap();
    (playlist.id, entries, record)
}

/// Every route of a playlist as one request, with what it needs and what it answers when
/// allowed.
async fn attempt(two: &Two, route: usize, token: &str, at: (i32, &[i32], i32)) -> StatusCode {
    let app = &two.app;
    let (id, entries, record) = at;
    let json_call = |method: Method, uri: String, body: Option<Value>| async move {
        app.call(method, &uri, body, Some(token)).await.status()
    };
    match route {
        0 => json_call(Method::GET, format!("/playlists/{id}"), None).await,
        1 => json_call(Method::GET, format!("/playlists/{id}/export"), None).await,
        2 => json_call(Method::GET, format!("/playlists/{id}/cover"), None).await,
        3 => {
            json_call(
                Method::POST,
                format!("/playlists/{id}/entries"),
                Some(json!({"item_id": record})),
            )
            .await
        }
        4 => {
            json_call(
                Method::PUT,
                format!("/playlists/{id}/entries/{}", entries[0]),
                Some(json!({"position": 2})),
            )
            .await
        }
        5 => {
            json_call(
                Method::DELETE,
                format!("/playlists/{id}/entries/{}", entries[0]),
                None,
            )
            .await
        }
        6 => {
            json_call(
                Method::PATCH,
                format!("/playlists/{id}"),
                Some(json!({"name": "Neu"})),
            )
            .await
        }
        7 => {
            json_call(
                Method::PATCH,
                format!("/playlists/{id}"),
                Some(json!({"description": "x", "folder": "y", "shared": true,
                            "collaborative": true})),
            )
            .await
        }
        8 => send_raw(
            app,
            Method::PUT,
            &format!("/playlists/{id}/cover"),
            "image/png",
            png(4),
            token,
        )
        .await
        .status(),
        9 => json_call(Method::DELETE, format!("/playlists/{id}/cover"), None).await,
        10 => json_call(Method::DELETE, format!("/playlists/{id}"), None).await,
        _ => unreachable!(),
    }
}

const ROUTES: [(&str, Need, StatusCode); 11] = [
    ("GET detail", Need::Read, StatusCode::OK),
    ("GET export", Need::Read, StatusCode::OK),
    ("GET cover", Need::Read, StatusCode::OK),
    ("POST entry", Need::Edit, StatusCode::OK),
    ("PUT entry", Need::Edit, StatusCode::NO_CONTENT),
    ("DELETE entry", Need::Edit, StatusCode::NO_CONTENT),
    ("PATCH name", Need::Own, StatusCode::OK),
    ("PATCH details", Need::Own, StatusCode::OK),
    ("PUT cover", Need::Own, StatusCode::OK),
    ("DELETE cover", Need::Own, StatusCode::NO_CONTENT),
    ("DELETE playlist", Need::Own, StatusCode::NO_CONTENT),
];

#[sqlx::test]
async fn every_route_for_the_owner_and_for_another_account_private_shared_and_collaborative(
    db: PgPool,
) {
    let two = two_accounts(db).await;
    // (shared, collaborative) → what the other account may do at most.
    let states = [
        ((false, false), None),
        // Collaborative alone opens nothing: it only counts on a shared playlist.
        ((false, true), None),
        ((true, false), Some(Need::Read)),
        ((true, true), Some(Need::Edit)),
    ];
    for ((shared, collaborative), allowed) in states {
        for (route, (label, need, success)) in ROUTES.iter().enumerate() {
            let context = format!("{label}, shared={shared}, collaborative={collaborative}");

            let (id, entries, record) = fresh(&two, shared, collaborative).await;
            let status = attempt(&two, route, &two.owner_token, (id, &entries, record)).await;
            assert_eq!(status, *success, "owner: {context}");

            let (id, entries, record) = fresh(&two, shared, collaborative).await;
            let status = attempt(&two, route, &two.other_token, (id, &entries, record)).await;
            let expected = match (allowed, need) {
                // Not even visible: unknown, whatever was tried.
                (None, _) => StatusCode::NOT_FOUND,
                (Some(_), Need::Read) => *success,
                (Some(Need::Edit), Need::Edit) => *success,
                (Some(_), _) => StatusCode::FORBIDDEN,
            };
            assert_eq!(status, expected, "other account: {context}");
            // A refused request changed nothing.
            if expected != *success {
                let playlist = visible_playlist(two.app.db(), two.owner, id)
                    .await
                    .unwrap()
                    .expect("still there");
                assert_eq!(playlist.name, "Liste", "{context}");
                assert_eq!(playlist.description, None, "{context}");
                assert!(playlist.cover_path.is_some(), "{context}");
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM playlist_entries WHERE playlist_id = $1",
                )
                .bind(id)
                .fetch_one(two.app.db())
                .await
                .unwrap();
                assert_eq!(count, 2, "{context}");
            }
        }
    }
}

#[sqlx::test]
async fn refusals_say_why_and_a_private_playlist_is_simply_unknown(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let (private, ..) = fresh(&two, false, false).await;
    let (shared, entries, record) = fresh(&two, true, false).await;

    let (status, body) = call(
        app,
        Method::PATCH,
        &format!("/playlists/{private}"),
        Some(json!({"name": "Meins"})),
        &two.other_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({"detail": "Unbekannte Playlist"}));

    let (status, body) = call(
        app,
        Method::PATCH,
        &format!("/playlists/{shared}"),
        Some(json!({"shared": false})),
        &two.other_token,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body,
        json!({"detail": "Nur der Besitzer darf diese Playlist ändern"})
    );
    let (status, body) = call(
        app,
        Method::DELETE,
        &format!("/playlists/{shared}"),
        None,
        &two.other_token,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["detail"],
        "Nur der Besitzer darf diese Playlist ändern"
    );

    for (method, uri, body) in [
        (
            Method::POST,
            format!("/playlists/{shared}/entries"),
            Some(json!({"item_id": record})),
        ),
        (
            Method::PUT,
            format!("/playlists/{shared}/entries/{}", entries[0]),
            Some(json!({"position": 2})),
        ),
        (
            Method::DELETE,
            format!("/playlists/{shared}/entries/{}", entries[0]),
            None,
        ),
    ] {
        let (status, body) = call(app, method, &uri, body, &two.other_token).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(
            body["detail"],
            "Diese Playlist darf nur ihr Besitzer bearbeiten"
        );
    }

    // An admin is an account like any other here.
    let (status, _) = call(
        app,
        Method::GET,
        &format!("/playlists/{private}"),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mara = create_playlist(app.db(), two.other, "Maras").await.unwrap();
    let (status, _) = call(
        app,
        Method::GET,
        &format!("/playlists/{}", mara.id),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "oskar is a Verwalter");
}

#[sqlx::test]
async fn a_collaborator_changes_entries_and_the_owner_sees_it(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let (id, entries, record) = fresh(&two, true, true).await;
    let (status, added) = call(
        app,
        Method::POST,
        &format!("/playlists/{id}/entries"),
        Some(json!({"item_id": record})),
        &two.other_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(added["position"], 3);
    let (status, _) = call(
        app,
        Method::DELETE,
        &format!("/playlists/{id}/entries/{}", entries[0]),
        None,
        &two.other_token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, detail) = call(
        app,
        Method::GET,
        &format!("/playlists/{id}"),
        None,
        &two.owner_token,
    )
    .await;
    let ids: Vec<i64> = detail["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [i64::from(entries[1]), added["id"].as_i64().unwrap()]);

    // The owner takes the sharing back: at once it is unknown to the other account again.
    share(app, id, false, true).await;
    let (status, _) = call(
        app,
        Method::POST,
        &format!("/playlists/{id}/entries"),
        Some(json!({"item_id": record})),
        &two.other_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- the fields ----------------------------------------------------------------------------------

#[sqlx::test]
async fn description_folder_and_the_switches_are_checked_and_can_be_cleared(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let playlist = create_playlist(app.db(), two.owner, "Zug").await.unwrap();
    let uri = format!("/playlists/{}", playlist.id);

    let (status, body) = call(
        app,
        Method::PATCH,
        &uri,
        Some(
            json!({"description": "ä".repeat(2000), "folder": "ö".repeat(100),
                    "shared": true, "collaborative": true, "name": "Bahn"}),
        ),
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the limits count characters");
    assert_eq!(body["name"], "Bahn");
    assert_eq!(body["folder"], "ö".repeat(100));
    assert_eq!(body["shared"], true);
    assert_eq!(body["collaborative"], true);

    for (bad, detail) in [
        (
            json!({"description": "x".repeat(2001)}),
            "description darf höchstens 2000 Zeichen haben",
        ),
        (
            json!({"description": 5}),
            "description darf höchstens 2000 Zeichen haben",
        ),
        (
            json!({"folder": "x".repeat(101)}),
            "folder darf höchstens 100 Zeichen haben",
        ),
        (
            json!({"folder": ["a"]}),
            "folder darf höchstens 100 Zeichen haben",
        ),
        (json!({"shared": "ja"}), "shared muss true oder false sein"),
        (
            json!({"collaborative": 1}),
            "collaborative muss true oder false sein",
        ),
        (json!({"shared": null}), "shared muss true oder false sein"),
        // A good field does not carry a bad one through.
        (
            json!({"description": "gut", "name": ""}),
            "name muss 1–100 Zeichen haben",
        ),
        // Nothing to change at all: as before, the name is missing.
        (json!({}), "name muss 1–100 Zeichen haben"),
        (json!({"unbekannt": 1}), "name muss 1–100 Zeichen haben"),
    ] {
        let (status, body) = call(
            app,
            Method::PATCH,
            &uri,
            Some(bad.clone()),
            &two.owner_token,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(body["detail"], detail, "{bad}");
    }
    let (_, unchanged) = call(app, Method::GET, &uri, None, &two.owner_token).await;
    assert_eq!(unchanged["description"], "ä".repeat(2000));
    assert_eq!(unchanged["name"], "Bahn");

    // Empty and null both clear; the rest stays.
    let (_, body) = call(
        app,
        Method::PATCH,
        &uri,
        Some(json!({"description": "", "folder": null})),
        &two.owner_token,
    )
    .await;
    assert_eq!(body["description"], Value::Null);
    assert_eq!(body["folder"], Value::Null);
    assert_eq!(body["shared"], true);
    let (_, body) = call(
        app,
        Method::PATCH,
        &uri,
        Some(json!({"description": null, "folder": "   ", "collaborative": false})),
        &two.owner_token,
    )
    .await;
    assert_eq!(body["description"], Value::Null);
    assert_eq!(body["folder"], Value::Null);
    assert_eq!(body["collaborative"], false);
    assert_eq!(body["name"], "Bahn");
}

#[sqlx::test]
async fn a_new_playlist_may_bring_its_details_along(db: PgPool) {
    let two = two_accounts(db).await;
    let (status, body) = call(
        &two.app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "Fest", "folder": "Feiern", "description": "Laut", "shared": true})),
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["folder"], "Feiern");
    assert_eq!(body["description"], "Laut");
    assert_eq!(body["shared"], true);
    let (status, body) = call(
        &two.app,
        Method::POST,
        "/playlists",
        Some(json!({"name": "Fest", "folder": "x".repeat(101)})),
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"], "folder darf höchstens 100 Zeichen haben");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM playlists")
        .fetch_one(two.app.db())
        .await
        .unwrap();
    assert_eq!(count, 1, "a refused one is not created");
}

// --- the cover -----------------------------------------------------------------------------------

#[sqlx::test]
async fn a_cover_goes_up_as_a_body_or_as_multipart_comes_down_in_sizes_and_goes_away(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let playlist = create_playlist(app.db(), two.owner, "Zug").await.unwrap();
    share(app, playlist.id, true, false).await;
    let uri = format!("/playlists/{}/cover", playlist.id);

    let (status, body) = call(app, Method::GET, &uri, None, &two.owner_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no cover of its own yet");
    assert_eq!(body, json!({"detail": "Kein Cover"}));

    // The picture itself as the body.
    let picture = png(600);
    let response = send_raw(
        app,
        Method::PUT,
        &uri,
        "image/png",
        picture.clone(),
        &two.owner_token,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["has_cover"], true);
    assert_eq!(body["id"], playlist.id);
    let stored = two
        .library
        .data()
        .join("playlists")
        .join(format!("{}.png", playlist.id));
    assert_eq!(std::fs::read(&stored).unwrap(), picture);

    // Everybody who sees the playlist gets it; the original without a size.
    let response = app.get(&uri, Some(&two.other_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert!(!header_str(&response, header::ETAG).is_empty());
    let original = response.into_body();
    let original = http_body_util::BodyExt::collect(original)
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(original.to_vec(), picture);

    // With a size: a JPEG no larger than the next thumbnail size.
    let response = app
        .get(&format!("{uri}?size=200"), Some(&two.owner_token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/jpeg");
    let small = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let small = image::load_from_memory(&small).unwrap();
    assert_eq!((small.width(), small.height()), (256, 256));
    let (status, _) = call(
        app,
        Method::GET,
        &format!("{uri}?size=-1"),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Multipart, field `image`, as an artist's picture is sent — and of another type: the old
    // file goes.
    let jpeg = {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(8, 8))
            .write_to(&mut bytes, image::ImageFormat::Jpeg)
            .unwrap();
        bytes.into_inner()
    };
    let response = send_raw(
        app,
        Method::PUT,
        &uri,
        &format!("multipart/form-data; boundary={BOUNDARY}"),
        multipart("image", "image/jpeg", &jpeg, true),
        &two.owner_token,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!stored.exists(), "the png is gone");
    let response = app.get(&uri, Some(&two.owner_token)).await;
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/jpeg");

    // Away again: 204, the file is deleted, the playlist says so.
    let (status, _) = call(app, Method::DELETE, &uri, None, &two.owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        std::fs::read_dir(two.library.data().join("playlists"))
            .unwrap()
            .next()
            .is_none()
    );
    let (status, _) = call(app, Method::GET, &uri, None, &two.owner_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, detail) = call(
        app,
        Method::GET,
        &format!("/playlists/{}", playlist.id),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(detail["has_cover"], false);
    let (status, _) = call(app, Method::DELETE, &uri, None, &two.owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "nothing to remove is fine");
}

#[sqlx::test]
async fn a_deleted_account_takes_the_covers_of_its_playlists_along(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let playlist = create_playlist(app.db(), two.other, "Lauf").await.unwrap();
    let uri = format!("/playlists/{}/cover", playlist.id);
    let response = send_raw(
        app,
        Method::PUT,
        &uri,
        "image/png",
        png(300),
        &two.other_token,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let stored = two
        .library
        .data()
        .join("playlists")
        .join(format!("{}.png", playlist.id));
    assert!(stored.is_file());

    let response = app
        .call(
            Method::DELETE,
            &format!("/users/{}", two.other),
            None,
            Some(&two.owner_token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!stored.exists(), "the cover went with the account");
}

#[sqlx::test]
async fn covers_are_checked_for_type_size_and_shape(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let playlist = create_playlist(app.db(), two.owner, "Zug").await.unwrap();
    let uri = format!("/playlists/{}/cover", playlist.id);
    let multipart_type = format!("multipart/form-data; boundary={BOUNDARY}");
    let too_large = vec![0u8; 5 * 1024 * 1024 + 1];

    let cases: Vec<(&str, &str, Vec<u8>, StatusCode, &str)> = vec![
        (
            "a text as the body",
            "text/plain",
            b"hallo".to_vec(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Nicht unterstütztes Bildformat",
        ),
        (
            "an svg",
            "image/svg+xml",
            b"<svg/>".to_vec(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Nicht unterstütztes Bildformat",
        ),
        (
            "no type and no picture",
            "application/octet-stream",
            b"hallo".to_vec(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Nicht unterstütztes Bildformat",
        ),
        (
            "an empty body",
            "image/png",
            Vec::new(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Leere Datei",
        ),
        (
            "too large a body",
            "image/png",
            too_large.clone(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "Bild ist zu groß (maximal 5 MB)",
        ),
        (
            "too large a file in a form",
            &multipart_type,
            multipart("image", "image/png", &too_large, true),
            StatusCode::PAYLOAD_TOO_LARGE,
            "Bild ist zu groß (maximal 5 MB)",
        ),
        (
            "a form with the wrong field",
            &multipart_type,
            multipart("bild", "image/png", &png(4), true),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Keine Bilddatei im Feld \"image\"",
        ),
        (
            "a form with a text in it",
            &multipart_type,
            multipart("image", "text/plain", b"hallo", true),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Nicht unterstütztes Bildformat",
        ),
    ];
    for (label, content_type, body, status, detail) in cases {
        let response = send_raw(app, Method::PUT, &uri, content_type, body, &two.owner_token).await;
        assert_eq!(response.status(), status, "{label}");
        assert_eq!(body_json(response).await["detail"], detail, "{label}");
    }
    let (_, detail) = call(
        app,
        Method::GET,
        &format!("/playlists/{}", playlist.id),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(detail["has_cover"], false);

    // Each allowed type, and a body that does not say what it is but is a picture.
    for (content_type, format, extension) in [
        ("image/png", image::ImageFormat::Png, "png"),
        ("image/jpeg", image::ImageFormat::Jpeg, "jpg"),
        ("image/gif", image::ImageFormat::Gif, "gif"),
        ("image/webp", image::ImageFormat::WebP, "webp"),
        ("application/octet-stream", image::ImageFormat::Png, "png"),
        (
            "image/jpeg; charset=binary",
            image::ImageFormat::Jpeg,
            "jpg",
        ),
    ] {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(4, 4))
            .to_rgb8()
            .write_to(&mut bytes, format)
            .unwrap();
        let response = send_raw(
            app,
            Method::PUT,
            &uri,
            content_type,
            bytes.into_inner(),
            &two.owner_token,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{content_type}");
        let files: Vec<String> = std::fs::read_dir(two.library.data().join("playlists"))
            .unwrap()
            .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            files,
            [format!("{}.{extension}", playlist.id)],
            "{content_type}"
        );
    }

    // Deleting the playlist takes its cover along.
    let (status, _) = call(
        app,
        Method::DELETE,
        &format!("/playlists/{}", playlist.id),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        std::fs::read_dir(two.library.data().join("playlists"))
            .unwrap()
            .next()
            .is_none()
    );
}

// --- export --------------------------------------------------------------------------------------

/// Roots as on a real server, and a small collection under them:
/// Björk – Debut (two tracks), Queen twice with the same song, and a book of two files.
async fn collection(app: &TestApp) -> Collection {
    let db = app.db();
    set_library_paths(db, "/srv/buch", "/srv/musik")
        .await
        .unwrap();
    let debut = album(db, "Björk", "Debut").await;
    let human = track(
        db,
        debut,
        1,
        "/srv/musik/Björk/Debut/01 Human Behaviour.flac",
        "Human Behaviour",
        252.4,
    )
    .await;
    let crying = track(
        db,
        debut,
        2,
        "/srv/musik/Björk/Debut/02 Crying.flac",
        "Crying",
        289.6,
    )
    .await;
    let live = album(db, "Queen", "Live Magic").await;
    let pressure_live = track(
        db,
        live,
        1,
        "/srv/musik/Queen/Live Magic/07 Under Pressure.mp3",
        "Under Pressure (Live)",
        221.0,
    )
    .await;
    let space = album(db, "Queen", "Hot Space").await;
    let pressure = track(
        db,
        space,
        1,
        "/srv/musik/Queen/Hot Space/11 Under Pressure.mp3",
        "Under Pressure (Remastered 2011)",
        248.0,
    )
    .await;
    let sampler = seed(db, Seed::new("album", "Sampler")).await;
    let feature = track(
        db,
        sampler,
        1,
        "/srv/musik/Sampler/03 Rós.mp3",
        "Hoppípolla",
        270.0,
    )
    .await;
    sqlx::query("UPDATE tracks SET artist = 'Sigur Rós' WHERE id = $1")
        .bind(feature)
        .execute(db)
        .await
        .unwrap();
    let momo = seed(
        db,
        Seed {
            author: Some("Michael Ende"),
            ..Seed::new("book", "Momo")
        },
    )
    .await;
    track(db, momo, 1, "/srv/buch/Ende/Momo/1.mp3", "Kapitel 1", 600.0).await;
    track(db, momo, 2, "/srv/buch/Ende/Momo/2.mp3", "Kapitel 2", 700.2).await;
    Collection {
        debut,
        human,
        crying,
        pressure_live,
        pressure,
        feature,
        momo,
    }
}

struct Collection {
    debut: i32,
    human: i32,
    crying: i32,
    pressure_live: i32,
    pressure: i32,
    feature: i32,
    momo: i32,
}

#[sqlx::test]
async fn an_export_lists_every_track_with_relative_paths_and_a_safe_file_name(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let c = collection(app).await;
    let playlist = create_playlist(app.db(), two.owner, "AC/DC: \"Best\"? Ärger")
        .await
        .unwrap();
    update_playlist(
        app.db(),
        playlist.id,
        &PlaylistChanges {
            description: Some(Some("Laut".into())),
            shared: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // A single track, a whole book, a whole album, and the single track once more.
    for target in [
        EntryTarget::Track(c.feature),
        EntryTarget::Item(c.momo),
        EntryTarget::Item(c.debut),
        EntryTarget::Track(c.feature),
    ] {
        append_entry(app.db(), playlist.id, target).await.unwrap();
    }
    let uri = format!("/playlists/{}/export", playlist.id);

    // Any account that sees the playlist may take it along.
    let response = app
        .get(&format!("{uri}?format=m3u8"), Some(&two.other_token))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_str(&response, header::CONTENT_TYPE),
        "audio/x-mpegurl; charset=utf-8"
    );
    assert_eq!(
        header_str(&response, header::CONTENT_DISPOSITION),
        "attachment; filename=\"AC_DC_ _Best__ _rger.m3u8\"; \
         filename*=UTF-8''AC_DC_%20_Best__%20%C3%84rger.m3u8"
    );
    let text = body_text(response).await;
    assert_eq!(
        text,
        "#EXTM3U\n\
         #PLAYLIST:AC/DC: \"Best\"? Ärger\n\
         #EXTINF:270,Sigur Rós - Hoppípolla\n\
         Sampler/03 Rós.mp3\n\
         #EXTINF:600,Michael Ende - Kapitel 1\n\
         Ende/Momo/1.mp3\n\
         #EXTINF:700,Michael Ende - Kapitel 2\n\
         Ende/Momo/2.mp3\n\
         #EXTINF:252,Björk - Human Behaviour\n\
         Björk/Debut/01 Human Behaviour.flac\n\
         #EXTINF:290,Björk - Crying\n\
         Björk/Debut/02 Crying.flac\n\
         #EXTINF:270,Sigur Rós - Hoppípolla\n\
         Sampler/03 Rós.mp3\n"
    );
    assert!(!text.contains("/srv/"), "no path of the server leaves it");

    // m3u8 is what comes without a format.
    let plain = body_text(app.get(&uri, Some(&two.owner_token)).await).await;
    assert_eq!(plain, text);

    let (status, body) = call(
        app,
        Method::GET,
        &format!("{uri}?format=json"),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "AC/DC: \"Best\"? Ärger");
    assert_eq!(body["description"], "Laut");
    let tracks = body["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 6);
    assert_eq!(
        tracks[0],
        json!({"title": "Hoppípolla", "artist": "Sigur Rós", "album": "Sampler",
               "duration": 270, "path": "Sampler/03 Rós.mp3"})
    );
    assert_eq!(
        tracks[2],
        json!({"title": "Kapitel 2", "artist": "Michael Ende", "album": "Momo",
               "duration": 700.2, "path": "Ende/Momo/2.mp3"})
    );

    let (status, body) = call(
        app,
        Method::GET,
        &format!("{uri}?format=pls"),
        None,
        &two.owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["detail"], "format muss m3u8 oder json sein");
}

#[sqlx::test]
async fn an_export_never_names_an_absolute_path_and_an_empty_playlist_is_a_bare_header(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    // No roots configured, and a track that lies under none: the file name is all that leaves.
    let stray = seed(app.db(), Seed::new("album", "Irgendwo")).await;
    let lost = track(app.db(), stray, 1, "/mnt/alt/x/Lied.mp3", "Lied", 61.5).await;
    let playlist = create_playlist(app.db(), two.owner, "...").await.unwrap();
    let uri = format!("/playlists/{}/export", playlist.id);
    let response = app.get(&uri, Some(&two.owner_token)).await;
    assert_eq!(
        header_str(&response, header::CONTENT_DISPOSITION),
        "attachment; filename=\"Playlist.m3u8\"; filename*=UTF-8''Playlist.m3u8"
    );
    assert_eq!(body_text(response).await, "#EXTM3U\n#PLAYLIST:...\n");

    append_entry(app.db(), playlist.id, EntryTarget::Track(lost))
        .await
        .unwrap();
    let text = body_text(app.get(&uri, Some(&two.owner_token)).await).await;
    // No artist anywhere: the title alone.
    assert_eq!(text, "#EXTM3U\n#PLAYLIST:...\n#EXTINF:62,Lied\nLied.mp3\n");
}

// --- import --------------------------------------------------------------------------------------

async fn import(two: &Two, body: Value) -> (StatusCode, Value) {
    call(
        &two.app,
        Method::POST,
        "/playlists/import",
        Some(body),
        &two.owner_token,
    )
    .await
}

async fn track_ids(app: &TestApp, playlist: i64) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT track_id FROM playlist_entries WHERE playlist_id = $1 ORDER BY position",
    )
    .bind(playlist as i32)
    .fetch_all(app.db())
    .await
    .unwrap()
}

#[sqlx::test]
async fn an_import_matches_by_path_then_by_names_and_keeps_the_order(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let c = collection(app).await;
    let content = "\u{feff}#EXTM3U\r\n\
        #PLAYLIST:Egal\r\n\
        Björk/Debut/01 Human Behaviour.flac\r\n\
        /Volumes/Musik/Björk/Debut/02 Crying.flac\r\n\
        C:\\Musik\\Queen\\Hot Space\\11 Under Pressure.mp3\r\n\
        file:///home/mara/Music/Bj%C3%B6rk/Debut/01%20Human%20Behaviour.flac\r\n\
        #EXTINF:270,Sigur Ros - HOPPIPOLLA (Live in Reykjavik)\r\n\
        /nirgends/track03.ogg\r\n\
        #EXTINF:222,Queen - Under Pressure\r\n\
        /nirgends/up.ogg\r\n\
        #EXTINF:249,Queen - Under Pressure\r\n\
        /nirgends/up2.ogg\r\n\
        #EXTINF:180,The Knife - Heartbeats\r\n\
        /nirgends/knife.mp3\r\n\
        Unbekannt/Album/99 Fehlt.mp3\r\n\
        Björk/Debut/01 Human Behaviour.flac\r\n\
        #EXTINF:600,Michael Ende - Kapitel 1\r\n";
    let (status, body) = import(
        &two,
        json!({"name": "Aus der Datei", "content": content, "folder": " Importe "}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["matched"], 9);
    assert_eq!(
        body["unmatched"],
        json!(["The Knife - Heartbeats", "Unbekannt/Album/99 Fehlt.mp3"])
    );
    let playlist = &body["playlist"];
    assert_eq!(playlist["name"], "Aus der Datei");
    assert_eq!(playlist["folder"], "Importe");
    assert_eq!(playlist["entry_count"], 9);
    assert_eq!(playlist["mine"], true);
    assert_eq!(playlist["shared"], false);
    assert_eq!(playlist["owner"], "oskar");
    assert_eq!(
        track_ids(app, playlist["id"].as_i64().unwrap()).await,
        [
            c.human,                          // relative to the music root
            c.crying,                         // under a foreign root
            c.pressure,                       // a Windows path
            c.human,                          // a file:// address, URL-encoded
            c.feature,       // artist and title, accents, case and an addition aside
            c.pressure_live, // two candidates: the length decides …
            c.pressure,      // … either way
            c.human,         // doubles stay
            first_chapter(app, c.momo).await, // a title without a path
        ]
    );
}

async fn first_chapter(app: &TestApp, book: i32) -> i32 {
    sqlx::query_scalar("SELECT id FROM tracks WHERE item_id = $1 AND position = 1")
        .bind(book)
        .fetch_one(app.db())
        .await
        .unwrap()
}

#[sqlx::test]
async fn an_export_comes_back_in_whole(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    let c = collection(app).await;
    let playlist = create_playlist(app.db(), two.owner, "Runde").await.unwrap();
    for target in [
        EntryTarget::Track(c.pressure),
        EntryTarget::Item(c.debut),
        EntryTarget::Track(c.pressure_live),
    ] {
        append_entry(app.db(), playlist.id, target).await.unwrap();
    }
    let exported = body_text(
        app.get(
            &format!("/playlists/{}/export", playlist.id),
            Some(&two.owner_token),
        )
        .await,
    )
    .await;
    let (status, body) = import(&two, json!({"name": "Zurück", "content": exported})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["unmatched"], json!([]));
    assert_eq!(
        track_ids(app, body["playlist"]["id"].as_i64().unwrap()).await,
        [c.pressure, c.human, c.crying, c.pressure_live]
    );
}

#[sqlx::test]
async fn an_import_that_finds_nothing_creates_nothing(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    collection(app).await;
    let (status, body) = import(
        &two,
        json!({"name": "Leer", "content": "#EXTM3U\n#EXTINF:1,Niemand - Nichts\nx/y.mp3\nz.mp3\n"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body,
        json!({"detail": "Kein Titel der Datei wurde in der Bibliothek gefunden",
               "unmatched": ["Niemand - Nichts", "z.mp3"]})
    );
    // A file without a single entry is the same case, with nothing to list.
    let (status, body) = import(&two, json!({"name": "Leer", "content": "#EXTM3U\n"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["unmatched"], json!([]));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM playlists")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn an_import_checks_its_body(db: PgPool) {
    let two = two_accounts(db).await;
    collection(&two.app).await;
    let line = "Björk/Debut/02 Crying.flac\n";
    for (bad, detail) in [
        (json!({"content": line}), "name muss 1–100 Zeichen haben"),
        (
            json!({"name": "", "content": line}),
            "name muss 1–100 Zeichen haben",
        ),
        (
            json!({"name": "x"}),
            "content muss der Text einer M3U-Datei sein",
        ),
        (
            json!({"name": "x", "content": ["a"]}),
            "content muss der Text einer M3U-Datei sein",
        ),
        (
            json!({"name": "x", "content": line, "folder": "f".repeat(101)}),
            "folder darf höchstens 100 Zeichen haben",
        ),
        (
            json!({"name": "x", "content": line, "folder": 3}),
            "folder darf höchstens 100 Zeichen haben",
        ),
    ] {
        let (status, body) = import(&two, bad.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(body["detail"], detail, "{bad}");
    }
    let response = two
        .app
        .post_raw("/playlists/import", "kein json", Some(&two.owner_token))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(response).await["detail"], "Ungültiger JSON-Body");

    // Exactly 2 MB of text is taken, one byte more is not.
    let filler = |bytes: usize| format!("{line}{}", "#".repeat(bytes - line.len()));
    let (status, body) = import(
        &two,
        json!({"name": "Groß", "content": filler(2 * 1024 * 1024)}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = import(
        &two,
        json!({"name": "Zu groß", "content": filler(2 * 1024 * 1024 + 1)}),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["detail"], "Die Datei ist zu groß (maximal 2 MB)");

    let response = two
        .app
        .post_json(
            "/playlists/import",
            json!({"name": "x", "content": line}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn two_thousand_lines_are_matched_with_a_handful_of_statements(db: PgPool) {
    let two = two_accounts(db).await;
    let app = &two.app;
    set_library_paths(app.db(), "", "/srv/musik").await.unwrap();
    let big = album(app.db(), "Viele", "Alles").await;
    sqlx::query(
        "INSERT INTO tracks (item_id, position, path, duration, title)
         SELECT $1, n, '/srv/musik/Viele/Alles/' || n || '.mp3', 100 + n, 'Lied ' || n
         FROM generate_series(1, 2000) AS n",
    )
    .bind(big)
    .execute(app.db())
    .await
    .unwrap();
    // Half by path, half by name, backwards.
    let content: String = (1..=2000)
        .rev()
        .map(|n| {
            if n % 2 == 0 {
                format!("D:\\Sammlung\\Viele\\Alles\\{n}.mp3\r\n")
            } else {
                format!("#EXTINF:{},Viele - Lied {n}\r\nanders/{n}.ogg\r\n", 100 + n)
            }
        })
        .collect();
    let started = std::time::Instant::now();
    let (status, body) = import(&two, json!({"name": "Viel", "content": content})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["matched"], 2000);
    assert_eq!(body["unmatched"], json!([]));
    assert_eq!(body["playlist"]["entry_count"], 2000);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    let titles: Vec<String> = sqlx::query_scalar(
        "SELECT tracks.title FROM playlist_entries
         JOIN tracks ON tracks.id = playlist_entries.track_id
         WHERE playlist_id = $1 ORDER BY playlist_entries.position LIMIT 3",
    )
    .bind(body["playlist"]["id"].as_i64().unwrap() as i32)
    .fetch_all(app.db())
    .await
    .unwrap();
    assert_eq!(titles, ["Lied 2000", "Lied 1999", "Lied 1998"]);
}

#[sqlx::test]
async fn the_new_routes_need_a_user(db: PgPool) {
    let app = TestApp::new(db);
    for (method, uri) in [
        (Method::GET, "/playlists/1/cover"),
        (Method::PUT, "/playlists/1/cover"),
        (Method::DELETE, "/playlists/1/cover"),
        (Method::GET, "/playlists/1/export"),
        (Method::POST, "/playlists/import"),
    ] {
        let response = app.call(method.clone(), uri, None, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
    }
}
