//! The `library_config` row (legacy tests/integration/library-paths.test.ts).

use autoreverse_server::library::paths::{LibraryPaths, get_library_paths, set_library_paths};
use sqlx::PgPool;

#[sqlx::test]
async fn get_returns_nothing_on_an_empty_table(db: PgPool) {
    assert_eq!(
        get_library_paths(&db).await.unwrap(),
        LibraryPaths::default()
    );
}

#[sqlx::test]
async fn set_then_get_round_trips_and_normalizes(db: PgPool) {
    set_library_paths(&db, "/library/books/", " /library/music ")
        .await
        .unwrap();
    assert_eq!(
        get_library_paths(&db).await.unwrap(),
        LibraryPaths {
            books_dir: Some("/library/books".into()),
            music_dir: Some("/library/music".into()),
        }
    );
}

#[sqlx::test]
async fn setting_twice_updates_the_one_row(db: PgPool) {
    set_library_paths(&db, "/library/books", "/library/music")
        .await
        .unwrap();
    set_library_paths(&db, "/library/books2", "/library/music2")
        .await
        .unwrap();
    let paths = get_library_paths(&db).await.unwrap();
    assert_eq!(paths.books_dir.as_deref(), Some("/library/books2"));
    assert_eq!(paths.music_dir.as_deref(), Some("/library/music2"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM library_config")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
