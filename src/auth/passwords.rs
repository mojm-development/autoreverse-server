//! argon2id password hashes and user creation (legacy `passwords.ts`).

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, PasswordHash, Version};
use sqlx::PgPool;

/// Same cost as legacy (`@node-rs/argon2`: m=64 MiB, t=3, p=4, 32-byte output).
fn hasher() -> Argon2<'static> {
    let params = Params::new(65_536, 3, 4, Some(32)).expect("constant argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hashes `plain` into a PHC string. Runs on the blocking pool: one hash costs ~64 MiB and tens of
/// milliseconds of CPU, which must not stall the async workers.
pub async fn hash_password(plain: &str) -> String {
    let plain = plain.to_owned();
    tokio::task::spawn_blocking(move || {
        hasher()
            .hash_password(plain.as_bytes())
            // Only fails on invalid parameters or an RNG failure — nothing a caller can handle.
            .expect("argon2 hashing failed")
            .to_string()
    })
    .await
    .expect("hashing task panicked")
}

/// False for a wrong password and for a hash that does not parse.
pub async fn verify_password(hashed: &str, plain: &str) -> bool {
    let (hashed, plain) = (hashed.to_owned(), plain.to_owned());
    tokio::task::spawn_blocking(move || {
        let Ok(parsed) = PasswordHash::new(&hashed) else {
            return false;
        };
        hasher().verify_password(plain.as_bytes(), &parsed).is_ok()
    })
    .await
    .unwrap_or(false)
}

/// Creates a user. The very first user always becomes admin, whatever `is_admin` says — otherwise
/// a fresh server could end up with nobody able to manage it.
pub async fn create_user(
    db: &PgPool,
    name: &str,
    password: &str,
    is_admin: bool,
) -> Result<i32, sqlx::Error> {
    // Hash before opening the transaction, so it is not held open for the duration of a hash.
    let hash = hash_password(password).await;
    let mut tx = db.begin().await?;
    // Serialises concurrent first-user creation: two parallel requests against an empty table
    // would otherwise both see count = 0 and both become admin.
    sqlx::query("LOCK TABLE users IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut *tx)
        .await?;
    let id: i32 = sqlx::query_scalar(
        "INSERT INTO users (name, password_hash, is_admin) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(name)
    .bind(hash)
    .bind(is_admin || count == 0)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn set_password(db: &PgPool, user_id: i32, password: &str) -> Result<(), sqlx::Error> {
    let hash = hash_password(password).await;
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(hash)
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(())
}

/// The user id for a correct name/password pair. An unknown name still costs one hash, so the
/// response time does not reveal which names exist.
pub async fn authenticate(
    db: &PgPool,
    name: &str,
    password: &str,
) -> Result<Option<i32>, sqlx::Error> {
    let row: Option<(i32, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE name = $1")
            .bind(name)
            .fetch_optional(db)
            .await?;
    let Some((id, hash)) = row else {
        hash_password(password).await;
        return Ok(None);
    };
    Ok(verify_password(&hash, password).await.then_some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hashes_and_verifies() {
        let hash = hash_password("correct horse battery staple").await;
        assert!(hash.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"));
        assert!(verify_password(&hash, "correct horse battery staple").await);
        assert!(!verify_password(&hash, "wrong").await);
    }

    #[tokio::test]
    async fn a_garbage_hash_never_verifies() {
        assert!(!verify_password("not a hash", "whatever").await);
    }
}
