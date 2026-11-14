//! Opaque bearer tokens, shared by the API and the web cookie (legacy `tokens.ts`).
//!
//! The database keeps only a SHA-256 of each token (`auth_tokens.value`), so a leaked backup or
//! a read-only SQL hole does not hand out working sessions. The tokens are 256 random bits, so a
//! plain, unsalted hash is enough — there is nothing to guess. A token nobody used for
//! [`IDLE_DAYS`] stops working.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// Days without a request after which a token is no longer accepted. Long enough that a phone
/// left in a drawer over the summer stays logged in, short enough that a forgotten device does
/// not stay a way in forever.
pub const IDLE_DAYS: i32 = 180;

/// 32 random bytes, base64url without padding (43 characters).
pub fn new_token_value() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source failed");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// What `auth_tokens.value` stores for a token: its SHA-256, lower-case hex.
pub fn token_digest(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub async fn issue_token(db: &PgPool, user_id: i32) -> Result<String, sqlx::Error> {
    let value = new_token_value();
    sqlx::query("INSERT INTO auth_tokens (value, user_id) VALUES ($1, $2)")
        .bind(token_digest(&value))
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(value)
}

/// The token's user, refreshing `last_seen_at` at most once an hour so a busy client does not
/// turn every request into a write. A token idle for longer than [`IDLE_DAYS`] is deleted and
/// counts as unknown.
pub async fn user_for_token(db: &PgPool, token: &str) -> Result<Option<i32>, sqlx::Error> {
    let digest = token_digest(token);
    let row: Option<(i32, bool, bool)> = sqlx::query_as(
        "SELECT user_id,
                (last_seen_at IS NULL OR last_seen_at < now() - interval '1 hour'),
                coalesce(last_seen_at, created_at) < now() - make_interval(days => $2)
         FROM auth_tokens WHERE value = $1",
    )
    .bind(&digest)
    .bind(IDLE_DAYS)
    .fetch_optional(db)
    .await?;
    let Some((user_id, stale, expired)) = row else {
        return Ok(None);
    };
    if expired {
        sqlx::query("DELETE FROM auth_tokens WHERE value = $1")
            .bind(&digest)
            .execute(db)
            .await?;
        return Ok(None);
    }
    if stale {
        sqlx::query("UPDATE auth_tokens SET last_seen_at = now() WHERE value = $1")
            .bind(&digest)
            .execute(db)
            .await?;
    }
    Ok(Some(user_id))
}

pub async fn revoke_token(db: &PgPool, token: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM auth_tokens WHERE value = $1")
        .bind(token_digest(token))
        .execute(db)
        .await?;
    Ok(())
}

/// Logs a user out everywhere except the session in `keep` (after a password change).
pub async fn revoke_other_tokens(db: &PgPool, user_id: i32, keep: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM auth_tokens WHERE user_id = $1 AND value <> $2")
        .bind(user_id)
        .bind(token_digest(keep))
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

    #[test]
    fn the_digest_is_sha256_hex() {
        assert_eq!(
            token_digest("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
