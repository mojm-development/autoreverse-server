//! Albums split into disc folders (`CD1`, `CD2` …): read as one album, the per-disc albums of
//! earlier scans absorbed, the discs shown on the album page; and deleting hidden copies.

use std::path::Path;

use autoreverse_server::library::duplicates::{
    DeleteRefused, delete_all_hidden, delete_hidden_copy, keep,
};
use autoreverse_server::scanner::books::{ItemKind, ScannedItem, ScannedTrack, scan_tree};
use autoreverse_server::scanner::store::store_items;
use sqlx::PgPool;

use crate::common::{Seed, TestApp, body_text, make_audio, seed, seed_track};

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

#[test]
fn disc_folders_become_one_album_ordered_by_folder_number() {
    let root = tempfile::tempdir().unwrap();
    let album = root.path().join("AC_DC/AC_DC - Bonfire");
    // Ripped one disc at a time: each says disc 1 and carries the disc in its album tag.
    for (disc, tracks) in [("CD2", ["01.mp3", "02.mp3"]), ("CD1", ["01.mp3", "02.mp3"])] {
        for (n, file) in tracks.iter().enumerate() {
            let number = (n + 1).to_string();
            make_audio(
                &album.join(disc).join(file),
                &[
                    ("album", &format!("Bonfire ({disc})")),
                    ("artist", "AC/DC"),
                    ("title", &format!("{disc}-{number}")),
                    ("track", &number),
                    ("disc", "1"),
                ],
            );
        }
    }
    let scanned = scan_music(root.path());
    assert_eq!(scanned.len(), 1);
    let item = &scanned[0];
    assert_eq!(item.source_path, album.to_str().unwrap());
    assert_eq!(item.title, "Bonfire");
    assert!(!item.unchanged);
    let order: Vec<(Option<&str>, Option<i32>, i32)> = item
        .tracks
        .iter()
        .map(|t| (t.title.as_deref(), t.disc, t.position))
        .collect();
    assert_eq!(
        order,
        [
            (Some("CD1-1"), Some(1), 1),
            (Some("CD1-2"), Some(1), 2),
            (Some("CD2-1"), Some(2), 3),
            (Some("CD2-2"), Some(2), 4),
        ]
    );
}

fn track(path: &str, position: i32, disc: i32) -> ScannedTrack {
    ScannedTrack {
        path: path.into(),
        position,
        title: Some(format!("Titel {position}")),
        disc: Some(disc),
        duration: 100.0,
        mtime: 1.0,
        size: 2,
    }
}

fn album(source: &str, tracks: Vec<ScannedTrack>) -> ScannedItem {
    ScannedItem {
        source_path: source.into(),
        kind: ItemKind::Album,
        title: "Bonfire".into(),
        author: None,
        artist: Some("AC/DC".into()),
        album_artist: None,
        description: None,
        series: None,
        year: None,
        series_index: None,
        tracks,
        chapters: Vec::new(),
        unchanged: false,
    }
}

async fn store(db: &PgPool, items: &[ScannedItem]) {
    let mut failures = Vec::new();
    store_items(
        db,
        items,
        Path::new("/nonexistent/covers"),
        &mut failures,
        |_, _, _| {},
        || false,
    )
    .await;
    assert!(failures.is_empty(), "{failures:?}");
}

async fn id_of(db: &PgPool, source: &str) -> Option<i32> {
    sqlx::query_scalar("SELECT id FROM items WHERE source_path = $1")
        .bind(source)
        .fetch_optional(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn the_merged_album_absorbs_the_per_disc_albums_with_progress_and_favorites(db: PgPool) {
    let app = TestApp::new(db.clone());
    let anna = app.user("anna", true).await;
    // What earlier scans made: one album per disc.
    store(
        &db,
        &[
            album(
                "/m/Bonfire/CD1",
                vec![
                    track("/m/Bonfire/CD1/1.mp3", 1, 1),
                    track("/m/Bonfire/CD1/2.mp3", 2, 1),
                ],
            ),
            album("/m/Bonfire/CD2", vec![track("/m/Bonfire/CD2/1.mp3", 1, 2)]),
        ],
    )
    .await;
    let cd2 = id_of(&db, "/m/Bonfire/CD2").await.unwrap();
    let cd1 = id_of(&db, "/m/Bonfire/CD1").await.unwrap();
    let favorite_track: i32 =
        sqlx::query_scalar("SELECT id FROM tracks WHERE path = '/m/Bonfire/CD2/1.mp3'")
            .fetch_one(&db)
            .await
            .unwrap();
    for statement in [
        "INSERT INTO progress (user_id, item_id, position) VALUES ($1, $2, 30)",
        "INSERT INTO favorites (user_id, item_id) VALUES ($1, $2)",
    ] {
        sqlx::query(statement)
            .bind(anna)
            .bind(cd2)
            .execute(&db)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO favorites (user_id, item_id) VALUES ($1, $2)")
        .bind(anna)
        .bind(cd1)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO favorites (user_id, track_id) VALUES ($1, $2)")
        .bind(anna)
        .bind(favorite_track)
        .execute(&db)
        .await
        .unwrap();

    // This scan reads the folder as one album.
    store(
        &db,
        &[album(
            "/m/Bonfire",
            vec![
                track("/m/Bonfire/CD1/1.mp3", 1, 1),
                track("/m/Bonfire/CD1/2.mp3", 2, 1),
                track("/m/Bonfire/CD2/1.mp3", 3, 2),
            ],
        )],
    )
    .await;

    let merged = id_of(&db, "/m/Bonfire").await.unwrap();
    assert_eq!(id_of(&db, "/m/Bonfire/CD1").await, None);
    assert_eq!(id_of(&db, "/m/Bonfire/CD2").await, None);
    // 30 s into disc 2 is 200 s (disc 1) + 30 s into the album.
    let position: f64 =
        sqlx::query_scalar("SELECT position FROM progress WHERE user_id = $1 AND item_id = $2")
            .bind(anna)
            .bind(merged)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(position, 230.0);
    let favorites: Vec<(Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT item_id, track_id FROM favorites WHERE user_id = $1
         ORDER BY item_id NULLS LAST",
    )
    .bind(anna)
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        favorites,
        [(Some(merged), None), (None, Some(favorite_track))]
    );
}

#[sqlx::test]
async fn the_album_page_heads_each_disc_and_numbers_within_it(db: PgPool) {
    let app = TestApp::new(db.clone());
    let user = app.user("anna", true).await;
    let cookie = format!("autoreverse_session={}", app.token(user).await);
    let id = seed(
        &db,
        Seed {
            artist: Some("AC/DC"),
            ..Seed::new("album", "Bonfire")
        },
    )
    .await;
    for (position, disc) in [(1, 1), (2, 1), (3, 2)] {
        let track = seed_track(&db, id, position, &format!("/m/{position}.mp3"), 60.0).await;
        sqlx::query("UPDATE tracks SET disc = $2 WHERE id = $1")
            .bind(track)
            .bind(disc)
            .execute(&db)
            .await
            .unwrap();
    }
    let html = body_text(
        app.get_with_cookie(&format!("/library/albums/{id}"), &cookie)
            .await,
    )
    .await;
    assert!(html.contains(r#"<span role="cell" class="eyebrow">CD 1</span>"#));
    assert!(html.contains(r#"<span role="cell" class="eyebrow">CD 2</span>"#));
    let numbers: Vec<&str> = html
        .match_indices(r#"<span class="num">"#)
        .map(|(i, m)| &html[i + m.len()..i + m.len() + 1])
        .collect();
    assert_eq!(numbers, ["1", "2", "1"]);

    // A single disc gets no heading.
    sqlx::query("UPDATE tracks SET disc = 1 WHERE item_id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let html = body_text(
        app.get_with_cookie(&format!("/library/albums/{id}"), &cookie)
            .await,
    )
    .await;
    assert!(!html.contains("CD 1"));
}

/// Two copies of one album under a real music root, the second hidden. Returns (kept, copy,
/// copy folder).
async fn hidden_copy(db: &PgPool, music: &Path) -> (i32, i32, std::path::PathBuf) {
    autoreverse_server::library::paths::set_library_paths(db, "/b", music.to_str().unwrap())
        .await
        .unwrap();
    let mut ids = Vec::new();
    for folder in ["Bonfire", "Bonfire (Kopie)"] {
        let dir = music.join("AC_DC").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("01.mp3"), b"audio").unwrap();
        std::fs::write(dir.join("cover.jpg"), b"jpg").unwrap();
        let id = seed(
            db,
            Seed {
                artist: Some("AC/DC"),
                source_path: dir.to_str(),
                ..Seed::new("album", "Bonfire")
            },
        )
        .await;
        seed_track(db, id, 1, dir.join("01.mp3").to_str().unwrap(), 60.0).await;
        ids.push(id);
    }
    keep(db, ids[0]).await.unwrap();
    (ids[0], ids[1], music.join("AC_DC/Bonfire (Kopie)"))
}

#[sqlx::test]
async fn deleting_a_hidden_copy_removes_its_files_folder_and_item(db: PgPool) {
    let music = tempfile::tempdir().unwrap();
    let (kept, copy, folder) = hidden_copy(&db, music.path()).await;

    assert_eq!(
        delete_hidden_copy(&db, kept, Some(music.path()))
            .await
            .unwrap(),
        Err(DeleteRefused::NotHidden)
    );
    assert_eq!(
        delete_hidden_copy(&db, copy, Some(Path::new("/elsewhere")))
            .await
            .unwrap(),
        Err(DeleteRefused::OutsideLibrary)
    );
    assert!(folder.exists());

    assert_eq!(
        delete_hidden_copy(&db, copy, Some(music.path()))
            .await
            .unwrap(),
        Ok(())
    );
    assert!(!folder.exists());
    assert!(music.path().join("AC_DC/Bonfire/01.mp3").exists());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(left, 1);
}

#[sqlx::test]
async fn a_copy_folder_with_subfolders_keeps_them(db: PgPool) {
    let music = tempfile::tempdir().unwrap();
    let (_, _, folder) = hidden_copy(&db, music.path()).await;
    std::fs::create_dir_all(folder.join("Bonus")).unwrap();
    std::fs::write(folder.join("Bonus/01.mp3"), b"audio").unwrap();

    let (deleted, refused) = delete_all_hidden(&db, Some(music.path())).await.unwrap();
    assert_eq!((deleted, refused), (1, None));
    assert!(!folder.join("01.mp3").exists());
    assert!(folder.join("Bonus/01.mp3").exists());
}

#[sqlx::test]
async fn the_page_deletes_on_request_and_reports_it(db: PgPool) {
    let music = tempfile::tempdir().unwrap();
    let (_, copy, folder) = hidden_copy(&db, music.path()).await;
    let app = TestApp::new(db.clone());
    let admin = app.user("admin", true).await;
    let cookie = format!("autoreverse_session={}", app.token(admin).await);

    let html = body_text(app.get_with_cookie("/settings/duplicates", &cookie).await).await;
    assert!(html.contains("Alle ausgeblendeten löschen"));
    assert!(html.contains("data-confirm="));

    let response = app
        .post_form(
            "/settings/duplicates",
            &format!("action=delete&id={copy}"),
            Some(&cookie),
        )
        .await;
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    assert_eq!(location, "/settings/duplicates?geloescht=1");
    assert!(!folder.exists());
    let html = body_text(app.get_with_cookie(&location, &cookie).await).await;
    assert!(html.contains("1 Kopie gelöscht."));

    let html = body_text(
        app.get_with_cookie(
            "/settings/duplicates?geloescht=0&fehler=schreibgeschuetzt",
            &cookie,
        )
        .await,
    )
    .await;
    assert!(html.contains("Ist der Musik-Ordner schreibgeschützt eingebunden?"));
}
