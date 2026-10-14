//! First admin from the environment (legacy auth/bootstrap.ts).

use autoreverse_server::auth::bootstrap::{Bootstrap, ensure_first_admin};
use autoreverse_server::auth::passwords::{authenticate, create_user};
use sqlx::PgPool;

async fn user_count(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn creates_the_admin_on_an_empty_table(db: PgPool) {
    let result = ensure_first_admin(&db, Some("admin"), Some("change-me"))
        .await
        .unwrap();
    let Bootstrap::Created(id) = result else {
        panic!("{result:?}")
    };
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(is_admin);
    assert_eq!(
        authenticate(&db, "admin", "change-me").await.unwrap(),
        Some(id)
    );
}

#[sqlx::test]
async fn does_nothing_once_a_user_exists(db: PgPool) {
    create_user(&db, "oliver", "hunter2hunter2", false)
        .await
        .unwrap();
    let result = ensure_first_admin(&db, Some("admin"), Some("change-me"))
        .await
        .unwrap();
    assert_eq!(result, Bootstrap::NotNeeded);
    assert_eq!(user_count(&db).await, 1);
}

#[sqlx::test]
async fn starts_without_a_user_when_credentials_are_missing(db: PgPool) {
    for (name, password) in [
        (None, Some("x")),
        (Some("admin"), None),
        (Some(""), Some("x")),
    ] {
        let result = ensure_first_admin(&db, name, password).await.unwrap();
        assert_eq!(result, Bootstrap::MissingCredentials);
    }
    assert_eq!(user_count(&db).await, 0);
}
