//! Token store (legacy tests/unit/tokens.test.ts).

use autoreverse_server::auth::tokens::{
    issue_token, revoke_other_tokens, revoke_token, user_for_token,
};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::common::TestApp;

#[sqlx::test]
async fn issues_a_resolvable_token(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = issue_token(app.db(), id).await.unwrap();
    assert!(token.len() > 30);
    assert_eq!(user_for_token(app.db(), &token).await.unwrap(), Some(id));
}

#[sqlx::test]
async fn an_unknown_token_resolves_to_none(db: PgPool) {
    assert_eq!(user_for_token(&db, "not-a-real-token").await.unwrap(), None);
}

#[sqlx::test]
async fn revoke_token_deletes_it(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = issue_token(app.db(), id).await.unwrap();
    revoke_token(app.db(), &token).await.unwrap();
    assert_eq!(user_for_token(app.db(), &token).await.unwrap(), None);
}

#[sqlx::test]
async fn revoke_other_tokens_keeps_the_current_one(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let other_user = app.user("mara", false).await;
    let keep = issue_token(app.db(), id).await.unwrap();
    let other = issue_token(app.db(), id).await.unwrap();
    let foreign = issue_token(app.db(), other_user).await.unwrap();
    revoke_other_tokens(app.db(), id, &keep).await.unwrap();
    assert_eq!(user_for_token(app.db(), &keep).await.unwrap(), Some(id));
    assert_eq!(user_for_token(app.db(), &other).await.unwrap(), None);
    assert_eq!(
        user_for_token(app.db(), &foreign).await.unwrap(),
        Some(other_user)
    );
}

async fn last_seen(db: &PgPool, token: &str) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT last_seen_at FROM auth_tokens WHERE value = $1")
        .bind(token)
        .fetch_one(db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn refreshes_last_seen_only_when_null_or_stale(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = issue_token(app.db(), id).await.unwrap();
    assert_eq!(last_seen(app.db(), &token).await, None);

    user_for_token(app.db(), &token).await.unwrap();
    assert!(last_seen(app.db(), &token).await.is_some());

    // Fresh: left alone.
    let fixed = "2099-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
    sqlx::query("UPDATE auth_tokens SET last_seen_at = $1 WHERE value = $2")
        .bind(fixed)
        .bind(&token)
        .execute(app.db())
        .await
        .unwrap();
    user_for_token(app.db(), &token).await.unwrap();
    assert_eq!(last_seen(app.db(), &token).await, Some(fixed));

    // Older than an hour: refreshed.
    sqlx::query(
        "UPDATE auth_tokens SET last_seen_at = now() - interval '2 hours' WHERE value = $1",
    )
    .bind(&token)
    .execute(app.db())
    .await
    .unwrap();
    user_for_token(app.db(), &token).await.unwrap();
    let seen = last_seen(app.db(), &token).await.unwrap();
    assert!(Utc::now() - seen < chrono::Duration::minutes(1));
}

#[sqlx::test]
async fn deleting_a_user_deletes_their_tokens(db: PgPool) {
    let app = TestApp::new(db);
    let id = app.user("oliver", false).await;
    let token = issue_token(app.db(), id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(app.db())
        .await
        .unwrap();
    assert_eq!(user_for_token(app.db(), &token).await.unwrap(), None);
}
