//! Opaque bearer tokens, shared by the API and the web cookie (legacy `tokens.ts`).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sqlx::PgPool;

/// 32 random bytes, base64url without padding (43 characters).
pub fn new_token_value() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source failed");
    URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn issue_token(db: &PgPool, user_id: i32) -> Result<String, sqlx::Error> {
    let value = new_token_value();
    sqlx::query("INSERT INTO auth_tokens (value, user_id) VALUES ($1, $2)")
        .bind(&value)
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(value)
}

/// The token's user, refreshing `last_seen_at` at most once an hour so a busy client does not
/// turn every request into a write.
pub async fn user_for_token(db: &PgPool, token: &str) -> Result<Option<i32>, sqlx::Error> {
    let row: Option<(i32, bool)> = sqlx::query_as(
        "SELECT user_id, (last_seen_at IS NULL OR last_seen_at < now() - interval '1 hour')
         FROM auth_tokens WHERE value = $1",
    )
    .bind(token)
    .fetch_optional(db)
    .await?;
    let Some((user_id, stale)) = row else {
        return Ok(None);
    };
    if stale {
        sqlx::query("UPDATE auth_tokens SET last_seen_at = now() WHERE value = $1")
            .bind(token)
            .execute(db)
            .await?;
    }
    Ok(Some(user_id))
}

pub async fn revoke_token(db: &PgPool, token: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM auth_tokens WHERE value = $1")
        .bind(token)
        .execute(db)
        .await?;
    Ok(())
}

/// Logs a user out everywhere except the session in `keep` (after a password change).
pub async fn revoke_other_tokens(db: &PgPool, user_id: i32, keep: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM auth_tokens WHERE user_id = $1 AND value <> $2")
        .bind(user_id)
        .bind(keep)
        .execute(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_values_are_long_url_safe_and_unique() {
        let a = new_token_value();
        let b = new_token_value();
        assert_eq!(a.len(), 43);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_ne!(a, b);
    }
}
