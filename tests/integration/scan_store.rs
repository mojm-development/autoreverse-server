//! `store_items` and `remove_vanished` (legacy tests/integration/scan-store.test.ts).

use std::collections::HashSet;
use std::path::Path;

use autoreverse_server::scanner::books::{ItemKind, ScanFailure, ScannedItem, ScannedTrack};
use autoreverse_server::scanner::chapters::ScannedChapter;
use autoreverse_server::scanner::store::{StoreReport, remove_vanished, store_items};
use sqlx::PgPool;

use crate::common::{Seed, TestApp, seed, seed_progress};

const ROOT: &str = "/library/books";
const COVERS: &str = "/nonexistent/covers";

fn track(path: &str, position: i32, title: &str) -> ScannedTrack {
    ScannedTrack {
        path: path.into(),
        position,
        title: Some(title.into()),
        artist: None,
        disc: None,
        duration: 10.0,
        mtime: 100.0,
        size: 200,
    }
}

fn scanned(source: &str, title: &str) -> ScannedItem {
    ScannedItem {
        source_path: source.into(),
        kind: ItemKind::Book,
        title: title.into(),
        author: Some("A".into()),
        artist: None,
        album_artist: None,
        description: None,
        series: None,
        year: None,
        genre: None,
        title_sort: None,
        artist_sort: None,
        series_index: None,
        tracks: vec![track(&format!("{source}/01.mp3"), 1, "T")],
        chapters: vec![ScannedChapter {
            title: "T".into(),
            start: 0.0,
            end: 10.0,
        }],
        unchanged: false,
    }
}

async fn store(db: &PgPool, items: &[ScannedItem]) -> (StoreReport, Vec<ScanFailure>) {
    let mut failures = Vec::new();
    let report = store_items(
        db,
        items,
        Path::new(COVERS),
        &mut failures,
        |_, _, _| {},
        || false,
    )
    .await;
    (report, failures)
}

async fn titles(db: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT title FROM items ORDER BY id")
        .fetch_all(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn skips_a_folder_whose_insert_fails_and_stores_the_rest(db: PgPool) {
    // Two tracks claiming the same position collide on UNIQUE(item_id, position). The scanner no
    // longer produces this, but storing must survive whatever it is handed.
    let mut broken = scanned("/library/books/A/Kaputt", "Kaputt");
    broken.tracks = vec![
        track("/library/books/A/Kaputt/01.mp3", 1, "eins"),
        track("/library/books/A/Kaputt/02.mp3", 1, "zwei"),
    ];
    let healthy = scanned("/library/books/A/Heil", "Heil");
    let (report, failures) = store(&db, &[broken, healthy]).await;
    assert_eq!(report.skipped, 1);
    assert_eq!(report.new, 1);
    let paths: Vec<_> = failures.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["/library/books/A/Kaputt"]);
    assert_eq!(titles(&db).await, ["Heil"]);
}

#[sqlx::test]
async fn spares_a_failed_folder_from_removal(db: PgPool) {
    store(&db, &[scanned("/library/books/A/B", "B")]).await;
    let removed = remove_vanished(&db, ROOT, &HashSet::new(), &["/library/books/A/B".into()])
        .await
        .unwrap();
    assert_eq!(removed, 0);
    assert_eq!(titles(&db).await.len(), 1);
}

#[sqlx::test]
async fn inserts_a_new_item_with_its_tracks_and_chapters(db: PgPool) {
    let (report, _) = store(&db, &[scanned("/library/books/A/B", "B")]).await;
    assert_eq!(report.new, 1);
    let (kind, sort_title, missing): (String, String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT kind, sort_title, missing_since FROM items")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!((kind.as_str(), sort_title.as_str()), ("book", "b"));
    assert!(missing.is_none());
    let counts: (i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM tracks), (SELECT count(*) FROM chapters)")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(counts, (1, 1));
}

#[sqlx::test]
async fn rescanning_the_same_folder_updates_instead_of_duplicating(db: PgPool) {
    store(&db, &[scanned("/library/books/A/B", "B")]).await;
    let (report, _) = store(&db, &[scanned("/library/books/A/B", "B renamed")]).await;
    assert_eq!(report.updated, 1);
    assert_eq!(titles(&db).await, ["B renamed"]);
}

#[sqlx::test]
async fn clears_missing_since_on_an_item_found_again(db: PgPool) {
    seed(
        &db,
        Seed {
            source_path: Some("/library/books/A/B"),
            missing: true,
            ..Seed::new("book", "B")
        },
    )
    .await;
    store(&db, &[scanned("/library/books/A/B", "B")]).await;
    let missing: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT missing_since FROM items")
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(missing.is_none());
}

#[sqlx::test]
async fn removes_tracks_no_longer_in_the_folder(db: PgPool) {
    let mut two = scanned("/library/books/A/B", "B");
    two.tracks = vec![
        track("/library/books/A/B/01.mp3", 1, "T1"),
        track("/library/books/A/B/02.mp3", 2, "T2"),
    ];
    store(&db, &[two]).await;
    store(&db, &[scanned("/library/books/A/B", "B")]).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tracks")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn swaps_two_surviving_tracks_without_a_unique_violation(db: PgPool) {
    let mut first = scanned("/library/books/A/B", "B");
    first.tracks = vec![
        track("/library/books/A/B/01.mp3", 1, "T1"),
        track("/library/books/A/B/02.mp3", 2, "T2"),
    ];
    store(&db, &[first]).await;
    let mut swapped = scanned("/library/books/A/B", "B");
    swapped.tracks = vec![
        track("/library/books/A/B/02.mp3", 1, "T2"),
        track("/library/books/A/B/01.mp3", 2, "T1"),
    ];
    let (report, failures) = store(&db, &[swapped]).await;
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(report.updated, 1);
    let rows: Vec<(String, i32)> =
        sqlx::query_as("SELECT path, position FROM tracks ORDER BY path")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [
            ("/library/books/A/B/01.mp3".to_owned(), 2),
            ("/library/books/A/B/02.mp3".to_owned(), 1)
        ]
    );
}

async fn series_of(db: &PgPool) -> (Option<String>, Option<f64>) {
    sqlx::query_as("SELECT series, series_index FROM items")
        .fetch_one(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn stores_the_series_and_volume_and_corrects_it_on_a_rescan(db: PgPool) {
    let folder = "/library/books/Eschbach/Perry Rhodan/03 - Der Fluch";
    let mut item = scanned(folder, "Der Fluch");
    item.series = Some("Perry Rhodan".into());
    item.series_index = Some(3.0);
    store(&db, &[item.clone()]).await;
    assert_eq!(
        series_of(&db).await,
        (Some("Perry Rhodan".into()), Some(3.0))
    );

    // Half volumes are why the column is not an integer.
    item.series_index = Some(3.5);
    store(&db, &[item]).await;
    assert_eq!(series_of(&db).await.1, Some(3.5));
}

#[sqlx::test]
async fn backfills_a_missing_series_on_an_unchanged_folder_but_never_overwrites_one(db: PgPool) {
    let folder = "/library/books/Eschbach/Perry Rhodan/03 - Der Fluch";
    store(&db, &[scanned(folder, "Der Fluch")]).await;

    let mut tree_only = scanned(folder, "Der Fluch");
    tree_only.unchanged = true;
    tree_only.tracks.clear();
    tree_only.chapters.clear();
    tree_only.series = Some("Perry Rhodan".into());
    tree_only.series_index = Some(3.0);
    let (report, _) = store(&db, &[tree_only.clone()]).await;
    assert_eq!(report.unchanged, 1);
    assert_eq!(
        series_of(&db).await,
        (Some("Perry Rhodan".into()), Some(3.0))
    );

    tree_only.series = Some("Etwas anderes".into());
    tree_only.series_index = Some(99.0);
    store(&db, &[tree_only]).await;
    assert_eq!(
        series_of(&db).await,
        (Some("Perry Rhodan".into()), Some(3.0))
    );
    // The unchanged pass leaves tracks alone.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tracks")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn a_hand_edited_field_survives_the_rescan(db: PgPool) {
    store(&db, &[scanned("/library/books/A/B", "B")]).await;
    sqlx::query(
        "UPDATE items SET title = 'Von Hand', author = 'Jemand', locked_fields = '{title,author}'",
    )
    .execute(&db)
    .await
    .unwrap();
    store(&db, &[scanned("/library/books/A/B", "Aus der Datei")]).await;
    let row: (String, String, Option<String>) =
        sqlx::query_as("SELECT title, sort_title, author FROM items")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(row, ("Von Hand".into(), "b".into(), Some("Jemand".into())));
}

#[sqlx::test]
async fn takes_the_cover_from_the_folder(db: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("A/B");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Cover.jpg"), [0xff, 0xd8, 0xff]).unwrap();
    let source = folder.to_str().unwrap();
    store(&db, &[scanned(source, "B")]).await;
    let cover: Option<String> = sqlx::query_scalar("SELECT cover_path FROM items")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        cover.as_deref(),
        Some(folder.join("Cover.jpg").to_str().unwrap())
    );
}

#[sqlx::test]
async fn removes_items_under_the_root_only(db: PgPool) {
    seed(
        &db,
        Seed {
            source_path: Some("/library/books/A"),
            ..Seed::new("book", "A")
        },
    )
    .await;
    seed(
        &db,
        Seed {
            source_path: Some("/library/books2/B"),
            ..Seed::new("book", "B")
        },
    )
    .await;
    let removed = remove_vanished(&db, ROOT, &HashSet::new(), &[])
        .await
        .unwrap();
    assert_eq!(removed, 1);
    assert_eq!(titles(&db).await, ["B"]);
}

#[sqlx::test]
async fn keeps_an_item_found_this_run(db: PgPool) {
    seed(
        &db,
        Seed {
            source_path: Some("/library/books/A"),
            ..Seed::new("book", "A")
        },
    )
    .await;
    let found: HashSet<String> = ["/library/books/A".to_owned()].into();
    assert_eq!(remove_vanished(&db, ROOT, &found, &[]).await.unwrap(), 0);
    assert_eq!(titles(&db).await.len(), 1);
}

#[sqlx::test]
async fn removes_the_root_folder_item_itself_when_it_is_gone(db: PgPool) {
    seed(
        &db,
        Seed {
            source_path: Some(ROOT),
            ..Seed::new("book", "Root")
        },
    )
    .await;
    assert_eq!(
        remove_vanished(&db, ROOT, &HashSet::new(), &[])
            .await
            .unwrap(),
        1
    );
}

#[sqlx::test]
async fn clears_out_rows_an_older_version_only_marked_missing(db: PgPool) {
    seed(
        &db,
        Seed {
            source_path: Some("/library/books/A"),
            missing: true,
            ..Seed::new("book", "A")
        },
    )
    .await;
    assert_eq!(
        remove_vanished(&db, ROOT, &HashSet::new(), &[])
            .await
            .unwrap(),
        1
    );
}

#[sqlx::test]
async fn takes_the_listening_history_with_it(db: PgPool) {
    let app = TestApp::new(db.clone());
    let user = app.user("oliver", false).await;
    let item = seed(
        &db,
        Seed {
            source_path: Some("/library/books/A"),
            ..Seed::new("book", "A")
        },
    )
    .await;
    seed_progress(&db, user, item, 42.0, false).await;
    sqlx::query(
        "INSERT INTO bookmarks (user_id, item_id, position, title) VALUES ($1, $2, 42, 'Stelle')",
    )
    .bind(user)
    .bind(item)
    .execute(&db)
    .await
    .unwrap();
    remove_vanished(&db, ROOT, &HashSet::new(), &[])
        .await
        .unwrap();
    let left: (i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM progress), (SELECT count(*) FROM bookmarks)")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(left, (0, 0));
}
