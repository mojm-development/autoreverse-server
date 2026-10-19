//! The music library: the scanner's album pass, the queries over albums and artists, and the API
//! routes around them (legacy scanner-folders `scanMusic`, queries-core, artist-covers,
//! description-durations-played `/children`, and the music half of run.ts).

use std::path::Path;

use autoreverse_server::library::artist_covers::{chosen_cover, chosen_covers, fallback_covers};
use autoreverse_server::library::paths::set_library_paths;
use autoreverse_server::library::queries::{
    Artist, ItemsFilter, albums_of_artist, artists, count_items, count_missing, items,
    library_counts,
};
use autoreverse_server::scanner::books::{ItemKind, ScannedItem, scan_tree};
use axum::http::{StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::common::{
    Seed, TempLibrary, TestApp, body_json, header_str, make_audio, seed, seed_track,
};

const PNG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

fn scan_music(root: &Path) -> Vec<ScannedItem> {
    scan_tree(
        root,
        ItemKind::Album,
        &Default::default(),
        &mut Vec::new(),
        |_, _| {},
        || false,
    )
}

// --- scanner ---------------------------------------------------------------------------------

#[test]
fn an_album_never_has_chapters() {
    let root = tempfile::tempdir().unwrap();
    let album = root.path().join("Ansa Volt/Nordlicht");
    make_audio(
        &album.join("01.mp3"),
        &[("album", "Nordlicht"), ("artist", "Ansa Volt")],
    );
    make_audio(&album.join("02.mp3"), &[("album", "Nordlicht")]);
    let scanned = scan_music(root.path());
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].kind, ItemKind::Album);
    assert!(scanned[0].chapters.is_empty());
    assert_eq!(scanned[0].tracks.len(), 2);
}

#[test]
fn the_artist_falls_back_from_album_artist_to_track_artist_to_folder() {
    let root = tempfile::tempdir().unwrap();
    make_audio(
        &root.path().join("Ordner/Mit Albumartist/01.mp3"),
        &[("album_artist", "Album Artist"), ("artist", "Track Artist")],
    );
    make_audio(
        &root.path().join("Ordner/Nur Artist/01.mp3"),
        &[("artist", "Track Artist")],
    );
    make_audio(&root.path().join("Ordner/Ohne Tags/01.mp3"), &[]);
    let scanned = scan_music(root.path());
    let by_folder = |name: &str| {
        scanned
            .iter()
            .find(|s| s.source_path.ends_with(name))
            .unwrap()
    };
    let tagged = by_folder("Mit Albumartist");
    assert_eq!(tagged.artist.as_deref(), Some("Album Artist"));
    assert_eq!(tagged.album_artist.as_deref(), Some("Album Artist"));
    assert_eq!(
        by_folder("Nur Artist").artist.as_deref(),
        Some("Track Artist")
    );
    let untagged = by_folder("Ohne Tags");
    assert_eq!(untagged.artist.as_deref(), Some("Ordner"));
    // The folder names the album when no tag does; books' series logic stays out of music.
    assert_eq!(untagged.title, "Ohne Tags");
    assert_eq!(
        (untagged.series.as_deref(), untagged.author.as_deref()),
        (None, None)
    );
}

#[test]
fn a_two_disc_album_is_ordered_by_disc_then_track() {
    let root = tempfile::tempdir().unwrap();
    let album = root.path().join("Kraftwerk/Doppelalbum");
    // File order disagrees with the tags on purpose.
    for (file, disc, track, title) in [
        ("a.mp3", "2", "1", "Zwei-Eins"),
        ("b.mp3", "1", "2", "Eins-Zwei"),
        ("c.mp3", "1", "1", "Eins-Eins"),
        ("d.mp3", "2", "2", "Zwei-Zwei"),
    ] {
        make_audio(
            &album.join(file),
            &[
                ("album", "Doppelalbum"),
                ("disc", disc),
                ("track", track),
                ("title", title),
                ("date", "1981"),
            ],
        );
    }
    let scanned = scan_music(root.path());
    let tracks = &scanned[0].tracks;
    let order: Vec<_> = tracks
        .iter()
        .map(|t| (t.position, t.disc, t.title.as_deref().unwrap()))
        .collect();
    assert_eq!(
        order,
        [
            (1, Some(1), "Eins-Eins"),
            (2, Some(1), "Eins-Zwei"),
            (3, Some(2), "Zwei-Eins"),
            (4, Some(2), "Zwei-Zwei"),
        ]
    );
    assert_eq!(scanned[0].year, Some(1981));
}

// --- scanning both roots ---------------------------------------------------------------------

async fn admin(app: &TestApp) -> String {
    let id = app.user("admin", true).await;
    app.token(id).await
}

async fn scan(app: &TestApp, token: &str) -> autoreverse_server::scanner::state::ScanState {
    let response = app.post_raw("/scan", "", Some(token)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    app.wait_for_scan().await
}

async fn titles_of(app: &TestApp, kind: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT title FROM items WHERE kind = $1 ORDER BY title")
        .bind(kind)
        .fetch_all(app.db())
        .await
        .unwrap()
}

async fn library(db: PgPool) -> (TempLibrary, TestApp, String) {
    let library = TempLibrary::new();
    let app = TestApp::with_data_dir(db, &library.data());
    let token = admin(&app).await;
    set_library_paths(
        app.db(),
        &TempLibrary::path_str(&library.books()),
        &TempLibrary::path_str(&library.music()),
    )
    .await
    .unwrap();
    (library, app, token)
}

#[sqlx::test]
async fn each_pass_only_removes_what_vanished_under_its_own_root(db: PgPool) {
    let (library, app, token) = library(db).await;
    make_audio(&library.books().join("Autor/Buch/01.mp3"), &[]);
    make_audio(&library.books().join("Autor/Zweites/01.mp3"), &[]);
    make_audio(&library.music().join("Interpret/Album/01.mp3"), &[]);
    make_audio(&library.music().join("Interpret/Zweites/01.mp3"), &[]);
    assert_eq!(scan(&app, &token).await.last_report.unwrap().new, 4);

    // An album gone: the music pass removes it, the books keep theirs — including the book
    // folder with the same name.
    std::fs::remove_dir_all(library.music().join("Interpret/Zweites")).unwrap();
    let report = scan(&app, &token).await.last_report.unwrap();
    assert_eq!(report.removed, 1);
    assert_eq!(titles_of(&app, "album").await, ["Album"]);
    assert_eq!(titles_of(&app, "book").await, ["Buch", "Zweites"]);

    // A book gone: the books pass removes it, the album stays.
    std::fs::remove_dir_all(library.books().join("Autor/Zweites")).unwrap();
    let report = scan(&app, &token).await.last_report.unwrap();
    assert_eq!(report.removed, 1);
    assert_eq!(titles_of(&app, "book").await, ["Buch"]);
    assert_eq!(titles_of(&app, "album").await, ["Album"]);
}

#[sqlx::test]
async fn an_unreadable_music_root_keeps_the_albums_and_still_scans_the_books(db: PgPool) {
    let (library, app, token) = library(db).await;
    make_audio(&library.music().join("Interpret/Album/01.mp3"), &[]);
    scan(&app, &token).await;
    assert_eq!(titles_of(&app, "album").await, ["Album"]);

    std::fs::remove_dir_all(library.music()).unwrap();
    make_audio(&library.books().join("Autor/Buch/01.mp3"), &[]);
    let status = scan(&app, &token).await;
    assert_eq!(
        status.last_error,
        Some(format!(
            "{}: Verzeichnis existiert nicht",
            library.music().display()
        ))
    );
    assert_eq!(titles_of(&app, "album").await, ["Album"]);
    assert_eq!(titles_of(&app, "book").await, ["Buch"]);
}

#[sqlx::test]
async fn an_album_lands_with_tracks_discs_year_and_folder_cover(db: PgPool) {
    let (library, app, token) = library(db).await;
    let album = library.music().join("Kraftwerk/Computerwelt");
    for (file, disc, track) in [("b.mp3", "2", "1"), ("a.mp3", "1", "1")] {
        make_audio(
            &album.join(file),
            &[
                ("album", "Computerwelt"),
                ("album_artist", "Kraftwerk"),
                ("disc", disc),
                ("track", track),
                ("date", "1981"),
            ],
        );
    }
    std::fs::write(album.join("cover.jpg"), [0xff, 0xd8, 0xff, 0xe0]).unwrap();
    let status = scan(&app, &token).await;
    assert_eq!(status.last_error, None);

    let list = body_json(app.get("/items?kind=album", Some(&token)).await).await;
    let row = &list["items"][0];
    assert_eq!(row["title"], "Computerwelt");
    assert_eq!(row["artist"], "Kraftwerk");
    assert_eq!(row["year"], 1981);
    assert_eq!(row["has_cover"], true);
    let id = row["id"].as_i64().unwrap();
    let cover = app.get(&format!("/items/{id}/cover"), Some(&token)).await;
    assert_eq!(cover.status(), StatusCode::OK);
    assert_eq!(header_str(&cover, header::CONTENT_TYPE), "image/jpeg");

    let discs: Vec<(i32, Option<i32>, String)> =
        sqlx::query_as("SELECT position, disc, path FROM tracks ORDER BY position")
            .fetch_all(app.db())
            .await
            .unwrap();
    assert_eq!(discs.len(), 2);
    assert_eq!((discs[0].0, discs[0].1), (1, Some(1)));
    assert!(discs[0].2.ends_with("a.mp3"));
    assert_eq!((discs[1].0, discs[1].1), (2, Some(2)));

    // The track route places a track in its album and disc.
    let track_id: i32 = sqlx::query_scalar("SELECT id FROM tracks WHERE position = 2")
        .fetch_one(app.db())
        .await
        .unwrap();
    let track = body_json(app.get(&format!("/tracks/{track_id}"), Some(&token)).await).await;
    assert_eq!(track["item_id"], id);
    assert_eq!(track["disc"], 2);
    assert_eq!(track["position"], 2);
}

// --- queries ---------------------------------------------------------------------------------

fn album<'a>(title: &'a str, artist: &'a str) -> Seed<'a> {
    Seed {
        artist: Some(artist),
        ..Seed::new("album", title)
    }
}

#[sqlx::test]
async fn vanished_albums_are_hidden_from_every_album_listing(db: PgPool) {
    seed(
        &db,
        Seed {
            missing: true,
            ..album("Weg", "Ansa Volt")
        },
    )
    .await;
    seed(&db, album("Da", "Ansa Volt")).await;
    let present = ItemsFilter {
        kind: Some("album".into()),
        missing: Some(false),
        limit: 10,
        ..Default::default()
    };
    assert_eq!(items(&db, &present).await.unwrap().len(), 1);
    assert_eq!(count_items(&db, "album", None, None).await.unwrap(), 1);
    assert_eq!(library_counts(&db).await.unwrap().album_count, 1);
    assert_eq!(
        artists(&db).await.unwrap(),
        [Artist {
            name: "Ansa Volt".into(),
            album_count: 1
        }]
    );
    let titles: Vec<String> = albums_of_artist(&db, "Ansa Volt")
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.title)
        .collect();
    assert_eq!(titles, ["Da"]);
    assert_eq!(count_missing(&db, Some("album")).await.unwrap(), 1);
}

#[sqlx::test]
async fn an_artist_whose_every_album_is_gone_drops_out(db: PgPool) {
    seed(
        &db,
        Seed {
            missing: true,
            ..album("Weg", "Ansa Volt")
        },
    )
    .await;
    assert!(artists(&db).await.unwrap().is_empty());
}

#[sqlx::test]
async fn artists_are_sorted_case_insensitively_and_count_their_albums(db: PgPool) {
    seed(&db, album("13", "die Ärzte")).await;
    seed(&db, album("Jazz ist anders", "die Ärzte")).await;
    seed(&db, album("Crush", "Bon Jovi")).await;
    seed(&db, Seed::new("album", "Ohne Interpret")).await;
    seed(
        &db,
        Seed {
            author: Some("Autorin"),
            ..Seed::new("book", "Kein Album")
        },
    )
    .await;
    let names: Vec<(String, i32)> = artists(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|a| (a.name, a.album_count))
        .collect();
    assert_eq!(names, [("Bon Jovi".into(), 1), ("die Ärzte".into(), 2)]);
}

async fn set_year(db: &PgPool, id: i32, year: i32) {
    sqlx::query("UPDATE items SET year = $2 WHERE id = $1")
        .bind(id)
        .bind(year)
        .execute(db)
        .await
        .unwrap();
}

#[sqlx::test]
async fn an_artists_albums_come_newest_first(db: PgPool) {
    let old = seed(&db, album("Alt", "X")).await;
    let new = seed(&db, album("Neu", "X")).await;
    seed(&db, album("Ohne Jahr", "X")).await;
    seed(&db, album("Fremd", "Y")).await;
    set_year(&db, old, 1990).await;
    set_year(&db, new, 2020).await;
    let titles: Vec<String> = albums_of_artist(&db, "X")
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.title)
        .collect();
    assert_eq!(titles, ["Neu", "Alt", "Ohne Jahr"]);
}

async fn album_with_cover(db: &PgPool, artist: &str, title: &str, cover: Option<&str>) -> i32 {
    seed(
        db,
        Seed {
            cover_path: cover,
            ..album(title, artist)
        },
    )
    .await
}

#[sqlx::test]
async fn the_fallback_picks_the_same_album_every_time(db: PgPool) {
    for title in ["13", "Jazz ist anders", "Die Bestie in Menschengestalt"] {
        album_with_cover(&db, "Die Ärzte", title, Some("/covers/x.jpg")).await;
    }
    let first = fallback_covers(&db).await.unwrap();
    let second = fallback_covers(&db).await.unwrap();
    assert!(first.contains_key("Die Ärzte"));
    assert_eq!(first.get("Die Ärzte"), second.get("Die Ärzte"));
}

#[sqlx::test]
async fn the_fallback_offers_one_album_per_artist_with_artwork(db: PgPool) {
    album_with_cover(&db, "Die Ärzte", "13", Some("/c/1.jpg")).await;
    album_with_cover(&db, "Die Ärzte", "Jazz ist anders", Some("/c/2.jpg")).await;
    album_with_cover(&db, "Bon Jovi", "Crush", Some("/c/3.jpg")).await;
    album_with_cover(&db, "Ansa Volt", "Nordlicht", None).await;
    let map = fallback_covers(&db).await.unwrap();
    assert_eq!(map.len(), 2);
    assert!(map.contains_key("Bon Jovi"));
    assert!(!map.contains_key("Ansa Volt"));
}

async fn choose_album(db: &PgPool, artist: &str, item_id: i32) {
    sqlx::query("INSERT INTO artist_covers (artist, item_id) VALUES ($1, $2)")
        .bind(artist)
        .bind(item_id)
        .execute(db)
        .await
        .unwrap();
}

async fn choose_image(db: &PgPool, artist: &str, path: &str) {
    sqlx::query("INSERT INTO artist_covers (artist, image_path) VALUES ($1, $2)")
        .bind(artist)
        .bind(path)
        .execute(db)
        .await
        .unwrap();
}

#[sqlx::test]
async fn reads_back_choices_one_and_all(db: PgPool) {
    let a = album_with_cover(&db, "A", "a1", None).await;
    choose_album(&db, "A", a).await;
    choose_image(&db, "B", "/data/artists/b.png").await;
    let cover = chosen_cover(&db, "A").await.unwrap().unwrap();
    assert_eq!((cover.item_id, cover.image_path), (Some(a), None));
    assert_eq!(chosen_cover(&db, "C").await.unwrap(), None);
    let all = chosen_covers(&db).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all["B"].image_path.as_deref(), Some("/data/artists/b.png"));
}

#[sqlx::test]
async fn a_choice_goes_with_the_album_it_points_at(db: PgPool) {
    let a = album_with_cover(&db, "A", "a1", None).await;
    choose_album(&db, "A", a).await;
    sqlx::query("DELETE FROM items").execute(&db).await.unwrap();
    assert_eq!(chosen_cover(&db, "A").await.unwrap(), None);
}

// --- API -------------------------------------------------------------------------------------

async fn signed_in(db: PgPool) -> (TestApp, String) {
    let app = TestApp::new(db);
    let user = app.user("oliver", false).await;
    let token = app.token(user).await;
    (app, token)
}

#[sqlx::test]
async fn the_music_routes_need_a_session(db: PgPool) {
    let app = TestApp::new(db);
    for uri in [
        "/artists",
        "/artists/X/cover",
        "/artists/X/image",
        "/tracks/1",
        "/items/1/children",
    ] {
        let response = app.get(uri, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(body_json(response).await, json!({"detail": "Kein Token"}));
    }
}

#[sqlx::test]
async fn lists_artists_with_their_album_counts(db: PgPool) {
    let (app, token) = signed_in(db).await;
    seed(app.db(), album("Autobahn", "Kraftwerk")).await;
    seed(app.db(), album("Radioaktivität", "Kraftwerk")).await;
    seed(app.db(), album("Nordlicht", "Ansa Volt")).await;
    let response = app.get("/artists", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({"artists": [
            {"name": "Ansa Volt", "album_count": 1},
            {"name": "Kraftwerk", "album_count": 2}
        ]})
    );
}

#[sqlx::test]
async fn items_filter_by_exact_artist(db: PgPool) {
    let (app, token) = signed_in(db).await;
    seed(app.db(), album("Autobahn", "Kraftwerk")).await;
    seed(app.db(), album("Kraftwerk Tribute", "Andere")).await;
    let body = body_json(
        app.get("/items?kind=album&artist=Kraftwerk", Some(&token))
            .await,
    )
    .await;
    let titles: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Autobahn"]);
}

#[sqlx::test]
async fn a_track_has_its_shape_and_an_unknown_one_is_404(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let id = seed(app.db(), album("Autobahn", "Kraftwerk")).await;
    let track = seed_track(app.db(), id, 1, "/m/1.mp3", 1350.5).await;
    let body = body_json(app.get(&format!("/tracks/{track}"), Some(&token)).await).await;
    assert_eq!(
        body,
        json!({"id": track, "position": 1, "title": null, "duration": 1350.5, "artist": null,
               "item_id": id, "disc": null})
    );
    for uri in ["/tracks/999999", "/tracks/abc"] {
        let response = app.get(uri, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(
            body_json(response).await,
            json!({"detail": "Unbekannter Track"})
        );
    }
}

#[sqlx::test]
async fn children_report_each_episodes_duration(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let podcast = seed(app.db(), Seed::new("podcast", "Maschinenraum")).await;
    let episode = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Folge 118")
        },
    )
    .await;
    seed_track(app.db(), episode, 1, "/a.mp3", 1200.0).await;
    seed_track(app.db(), episode, 2, "/b.mp3", 300.0).await;
    let body = body_json(
        app.get(&format!("/items/{podcast}/children"), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["title"], "Folge 118");
    assert_eq!(body["durations"][episode.to_string()], 1500.0);
}

#[sqlx::test]
async fn children_fall_back_to_the_feeds_duration_until_measured(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let podcast = seed(app.db(), Seed::new("podcast", "Maschinenraum")).await;
    let fetched = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Geladen")
        },
    )
    .await;
    let pending = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Nur im Feed")
        },
    )
    .await;
    let unknown = seed(
        app.db(),
        Seed {
            parent_id: Some(podcast),
            ..Seed::new("episode", "Ohne Angabe")
        },
    )
    .await;
    sqlx::query("UPDATE items SET feed_duration = 999 WHERE id = ANY($1)")
        .bind(vec![fetched, pending])
        .execute(app.db())
        .await
        .unwrap();
    seed_track(app.db(), fetched, 1, "/f.mp3", 1234.0).await;
    let body = body_json(
        app.get(&format!("/items/{podcast}/children"), Some(&token))
            .await,
    )
    .await;
    let durations = &body["durations"];
    assert_eq!(durations[fetched.to_string()], 1234.0);
    assert_eq!(durations[pending.to_string()], 999.0);
    assert_eq!(durations.get(unknown.to_string()), None);
}

#[sqlx::test]
async fn children_of_an_unknown_item_are_404_and_an_album_has_none(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let response = app.get("/items/999999/children", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(response).await,
        json!({"detail": "Unbekanntes Item"})
    );
    let id = seed(app.db(), album("Autobahn", "Kraftwerk")).await;
    let body = body_json(
        app.get(&format!("/items/{id}/children"), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body, json!({"items": [], "durations": {}}));
}

#[sqlx::test]
async fn the_cover_route_names_the_chosen_album(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let id = seed(app.db(), album("Autobahn", "Kraftwerk")).await;
    choose_album(app.db(), "Kraftwerk", id).await;
    let response = app.get("/artists/Kraftwerk/cover", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({"artist": "Kraftwerk", "item_id": id, "has_image": false})
    );
}

#[sqlx::test]
async fn the_cover_route_reports_an_upload_without_an_album_id(db: PgPool) {
    let (app, token) = signed_in(db).await;
    choose_image(app.db(), "Die Ärzte", "/data/artists/x.png").await;
    let body = body_json(
        app.get("/artists/Die%20%C3%84rzte/cover", Some(&token))
            .await,
    )
    .await;
    assert_eq!(
        body,
        json!({"artist": "Die Ärzte", "item_id": null, "has_image": true})
    );
}

#[sqlx::test]
async fn the_cover_route_says_nothing_is_chosen_when_nothing_is(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let body = body_json(app.get("/artists/Kraftwerk/cover", Some(&token)).await).await;
    assert_eq!(
        body,
        json!({"artist": "Kraftwerk", "item_id": null, "has_image": false})
    );
}

/// The app with library roots under `library` and a PNG cover inside the music root.
async fn image_app(db: PgPool, library: &TempLibrary) -> (TestApp, String, String) {
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
    let cover = library.music().join("Kraftwerk/Autobahn/cover.png");
    std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
    std::fs::write(&cover, PNG).unwrap();
    (app, token, TempLibrary::path_str(&cover))
}

#[sqlx::test]
async fn the_image_route_serves_the_chosen_album_cover(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token, cover) = image_app(db, &library).await;
    album_with_cover(app.db(), "Kraftwerk", "Ohne", None).await;
    let id = album_with_cover(app.db(), "Kraftwerk", "Autobahn", Some(&cover)).await;
    choose_album(app.db(), "Kraftwerk", id).await;
    let response = app.get("/artists/Kraftwerk/image", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");
    assert_eq!(
        header_str(&response, header::CACHE_CONTROL),
        "private, max-age=300"
    );
}

#[sqlx::test]
async fn the_image_route_falls_back_to_the_derived_album(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token, cover) = image_app(db, &library).await;
    album_with_cover(app.db(), "Kraftwerk", "Autobahn", Some(&cover)).await;
    let response = app.get("/artists/Kraftwerk/image", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test]
async fn the_image_route_serves_an_upload_from_the_artists_directory_only(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token, cover) = image_app(db, &library).await;
    let artists_dir = library.data().join("artists");
    std::fs::create_dir_all(&artists_dir).unwrap();
    let upload = artists_dir.join("kraftwerk.png");
    std::fs::write(&upload, PNG).unwrap();
    choose_image(app.db(), "Kraftwerk", upload.to_str().unwrap()).await;
    let response = app.get("/artists/Kraftwerk/image", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, header::CONTENT_TYPE), "image/png");

    // A stored path outside the artists directory is not served, even if the file exists.
    choose_image(app.db(), "Ansa Volt", &cover).await;
    let response = app.get("/artists/Ansa%20Volt/image", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(response).await, json!({"detail": "Kein Bild"}));
}

#[sqlx::test]
async fn the_image_route_says_404_without_any_picture(db: PgPool) {
    let library = TempLibrary::new();
    let (app, token, _) = image_app(db, &library).await;
    album_with_cover(app.db(), "Kraftwerk", "Autobahn", None).await;
    // A cover path outside every root does not count either.
    album_with_cover(app.db(), "Ansa Volt", "Nordlicht", Some("/etc/hosts")).await;
    for uri in [
        "/artists/Kraftwerk/image",
        "/artists/Ansa%20Volt/image",
        "/artists/Niemand/image",
    ] {
        let response = app.get(uri, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        let body: Value = body_json(response).await;
        assert_eq!(body, json!({"detail": "Kein Bild"}), "{uri}");
    }
}

// --- genres, track artists, search by artist ---------------------------------------------------

async fn set_genre(db: &PgPool, id: i32, genre: &str) {
    sqlx::query("UPDATE items SET genre = $2 WHERE id = $1")
        .bind(id)
        .bind(genre)
        .execute(db)
        .await
        .unwrap();
}

#[sqlx::test]
async fn albums_filter_by_genre_and_genres_are_listed_case_insensitively(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let rock = seed(app.db(), album("Back in Black", "AC/DC")).await;
    let rock2 = seed(app.db(), album("Highway", "AC/DC")).await;
    let rock3 = seed(app.db(), album("Let There Be Rock", "AC/DC")).await;
    let jazz = seed(app.db(), album("Kind of Blue", "Miles Davis")).await;
    // Listed in the spelling most albums use.
    set_genre(app.db(), rock, "Rock").await;
    set_genre(app.db(), rock2, "rock").await;
    set_genre(app.db(), rock3, "Rock").await;
    set_genre(app.db(), jazz, "Jazz").await;

    let body = body_json(app.get("/genres", Some(&token)).await).await;
    assert_eq!(
        body,
        json!({"genres": [
            {"name": "Jazz", "album_count": 1},
            {"name": "Rock", "album_count": 3}
        ]})
    );
    let body = body_json(app.get("/items?kind=album&genre=ROCK", Some(&token)).await).await;
    let titles: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Back in Black", "Highway", "Let There Be Rock"]);
    let body = body_json(app.get(&format!("/items/{jazz}"), Some(&token)).await).await;
    assert_eq!(body["genre"], "Jazz");
}

#[sqlx::test]
async fn tracks_carry_their_artist_and_disc_and_search_finds_them_by_artist(db: PgPool) {
    let (app, token) = signed_in(db).await;
    let sampler = seed(app.db(), album("Bravo Hits", "Verschiedene Interpreten")).await;
    let track = seed_track(app.db(), sampler, 1, "/m/1.mp3", 200.0).await;
    sqlx::query("UPDATE tracks SET artist = 'Nena', title = 'Leuchtturm', disc = 2 WHERE id = $1")
        .bind(track)
        .execute(app.db())
        .await
        .unwrap();

    let body = body_json(app.get(&format!("/items/{sampler}"), Some(&token)).await).await;
    assert_eq!(body["tracks"][0]["artist"], "Nena");
    assert_eq!(body["tracks"][0]["disc"], 2);

    let body = body_json(app.get("/search?q=nena", Some(&token)).await).await;
    assert_eq!(body["tracks"][0]["title"], "Leuchtturm");
    assert_eq!(body["tracks"][0]["artist"], "Nena");
    let body = body_json(app.get("/search?q=interpreten", Some(&token)).await).await;
    assert_eq!(
        body["artists"],
        json!([{"name": "Verschiedene Interpreten", "role": "artist", "work_count": 1}])
    );
}

#[sqlx::test]
async fn a_verwalter_sets_the_genre_and_it_is_locked(db: PgPool) {
    let app = TestApp::new(db);
    let admin = app.user("admin", true).await;
    let token = app.token(admin).await;
    let id = seed(app.db(), album("Kind of Blue", "Miles Davis")).await;
    let response = app
        .call(
            axum::http::Method::PATCH,
            &format!("/items/{id}"),
            Some(json!({ "set": { "genre": "Modal Jazz" } })),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let (genre, locked): (Option<String>, Vec<String>) =
        sqlx::query_as("SELECT genre, locked_fields FROM items WHERE id = $1")
            .bind(id)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(genre.as_deref(), Some("Modal Jazz"));
    assert!(locked.contains(&"genre".to_owned()));
}

#[sqlx::test]
async fn artist_spellings_are_unified_to_the_most_common_or_the_locked_one(db: PgPool) {
    use autoreverse_server::library::artist_covers::unify_artist_spellings;
    let a = seed(&db, album("Back in Black", "AC/DC")).await;
    let b = seed(&db, album("Highway", "AC/DC")).await;
    let c = seed(&db, album("Powerage", "Ac/Dc")).await;
    sqlx::query("INSERT INTO artist_covers (artist, item_id) VALUES ('Ac/Dc', $1)")
        .bind(c)
        .execute(&db)
        .await
        .unwrap();
    let k1 = seed(&db, album("Autobahn", "kraftwerk")).await;
    let k2 = seed(&db, album("Radio", "kraftwerk")).await;
    let k3 = seed(&db, album("Mix", "Kraftwerk")).await;
    sqlx::query("UPDATE items SET locked_fields = ARRAY['artist'] WHERE id = $1")
        .bind(k3)
        .execute(&db)
        .await
        .unwrap();

    unify_artist_spellings(&db).await.unwrap();

    let artist = |id: i32| {
        let db = db.clone();
        async move {
            sqlx::query_scalar::<_, String>("SELECT artist FROM items WHERE id = $1")
                .bind(id)
                .fetch_one(&db)
                .await
                .unwrap()
        }
    };
    for id in [a, b, c] {
        assert_eq!(artist(id).await, "AC/DC");
    }
    // A Verwalter's spelling wins over the majority.
    for id in [k1, k2, k3] {
        assert_eq!(artist(id).await, "Kraftwerk");
    }
    let covers: Vec<String> = sqlx::query_scalar("SELECT artist FROM artist_covers")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(covers, ["AC/DC"]);
    assert_eq!(
        artists(&db).await.unwrap(),
        [
            Artist {
                name: "AC/DC".into(),
                album_count: 3
            },
            Artist {
                name: "Kraftwerk".into(),
                album_count: 3
            }
        ]
    );
}

#[sqlx::test]
async fn artists_sort_by_their_sort_name_or_without_a_leading_article(db: PgPool) {
    seed(&db, album("Gold", "Abba")).await;
    seed(&db, album("Parachutes", "Coldplay")).await;
    seed(&db, album("Is This It", "The Strokes")).await;
    let beatles = seed(&db, album("Abbey Road", "The Beatles")).await;
    sqlx::query("UPDATE items SET artist_sort = 'Beatles, The' WHERE id = $1")
        .bind(beatles)
        .execute(&db)
        .await
        .unwrap();
    let names: Vec<String> = artists(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(names, ["Abba", "The Beatles", "Coldplay", "The Strokes"]);
}

#[sqlx::test]
async fn a_stored_album_sorts_without_its_article_unless_the_tags_say_otherwise(db: PgPool) {
    use autoreverse_server::scanner::store::store_items;
    let mut wall = scan_music_item("The Wall");
    let mut road = scan_music_item("Abbey Road");
    road.source_path = "/m/Beatles/Abbey Road".into();
    road.title_sort = Some("Road, Abbey".into());
    wall.source_path = "/m/Pink Floyd/The Wall".into();
    let mut failures = Vec::new();
    store_items(
        &db,
        &[wall, road],
        Path::new("/nonexistent"),
        &mut failures,
        |_, _, _| {},
        || false,
    )
    .await;
    let keys: Vec<(String, String)> =
        sqlx::query_as("SELECT title, sort_title FROM items ORDER BY title")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        keys,
        [
            ("Abbey Road".into(), "road, abbey".into()),
            ("The Wall".into(), "wall".into())
        ]
    );
}

fn scan_music_item(title: &str) -> ScannedItem {
    ScannedItem {
        source_path: String::new(),
        kind: ItemKind::Album,
        title: title.into(),
        author: None,
        artist: Some("X".into()),
        album_artist: None,
        description: None,
        series: None,
        year: None,
        genre: None,
        title_sort: None,
        artist_sort: None,
        series_index: None,
        tracks: Vec::new(),
        chapters: Vec::new(),
        unchanged: false,
    }
}
