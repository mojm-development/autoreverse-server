//! Users and passwords (legacy tests/unit/passwords.test.ts).

use autoreverse_server::auth::passwords::{authenticate, create_user, set_password};
use sqlx::PgPool;

async fn is_admin(db: &PgPool, id: i32) -> bool {
    sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn promotes_the_first_user_to_admin_regardless_of_the_flag(db: PgPool) {
    let id = create_user(&db, "oliver", "hunter2hunter2", false)
        .await
        .unwrap();
    assert!(is_admin(&db, id).await);
}

#[sqlx::test]
async fn does_not_promote_a_later_user(db: PgPool) {
    create_user(&db, "oliver", "hunter2hunter2", false)
        .await
        .unwrap();
    let id = create_user(&db, "mara", "hunter2hunter2", false)
        .await
        .unwrap();
    assert!(!is_admin(&db, id).await);
}

#[sqlx::test]
async fn names_are_unique(db: PgPool) {
    create_user(&db, "oliver", "hunter2hunter2", false)
        .await
        .unwrap();
    assert!(create_user(&db, "oliver", "other", false).await.is_err());
}

#[sqlx::test]
async fn authenticate_returns_none_for_an_unknown_user(db: PgPool) {
    assert_eq!(authenticate(&db, "nobody", "whatever").await.unwrap(), None);
}

#[sqlx::test]
async fn authenticate_returns_the_id_on_the_correct_password(db: PgPool) {
    let id = create_user(&db, "oliver", "hunter2hunter2", false)
        .await
        .unwrap();
    assert_eq!(
        authenticate(&db, "oliver", "hunter2hunter2").await.unwrap(),
        Some(id)
    );
    assert_eq!(authenticate(&db, "oliver", "wrong").await.unwrap(), None);
}

#[sqlx::test]
async fn set_password_replaces_the_old_one(db: PgPool) {
    let id = create_user(&db, "oliver", "hunter2hunter2", false)
        .await
        .unwrap();
    set_password(&db, id, "new-password").await.unwrap();
    assert_eq!(
        authenticate(&db, "oliver", "hunter2hunter2").await.unwrap(),
        None
    );
    assert_eq!(
        authenticate(&db, "oliver", "new-password").await.unwrap(),
        Some(id)
    );
}
