//! Start-up checks: migrations against an empty database, and a clear refusal for the database
//! of the old TypeScript version instead of a cryptic "relation already exists".

use autoreverse_server::db::{LEGACY_DATABASE, MigrateSetupError, migrate};
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn an_empty_database_is_migrated_and_a_second_start_is_a_no_op(db: PgPool) {
    migrate(&db).await.unwrap();
    migrate(&db).await.unwrap();
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

#[sqlx::test(migrations = false)]
async fn a_drizzle_push_database_is_refused_with_a_german_message(db: PgPool) {
    // What `drizzle-kit push` left: the tables, no migration history.
    sqlx::query("CREATE TABLE users (id serial PRIMARY KEY, name text NOT NULL)")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name) VALUES ('oliver')")
        .execute(&db)
        .await
        .unwrap();
    let err = migrate(&db).await.unwrap_err();
    assert!(matches!(err, MigrateSetupError::LegacyDatabase));
    assert_eq!(err.to_string(), LEGACY_DATABASE);
    assert!(err.to_string().contains("neue, leere Datenbank"));
    // Nothing was touched.
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM users")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(names, vec!["oliver"]);
}

#[sqlx::test(migrations = false)]
async fn a_drizzle_migrated_database_is_refused(db: PgPool) {
    sqlx::query("CREATE SCHEMA drizzle")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE drizzle.__drizzle_migrations (id serial PRIMARY KEY, hash text)")
        .execute(&db)
        .await
        .unwrap();
    assert!(matches!(
        migrate(&db).await,
        Err(MigrateSetupError::LegacyDatabase)
    ));
}
