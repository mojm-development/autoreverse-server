//! The user directory behind `/users*`, Settings → Nutzer and `set-password` (legacy
//! `directory.ts`, plus deleting users and setting another user's password, which legacy had no
//! route for).
//!
//! Every change that could leave the server without an admin locks all admin rows first
//! (`FOR UPDATE`), so two concurrent demotions or deletions of the last two admins cannot both
//! pass the check.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use super::passwords::{create_user, hash_password};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: i32,
    pub name: String,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    /// The latest `last_seen_at` over the user's tokens; `None` if never seen (or logged out
    /// everywhere).
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum DirectoryError {
    #[error("unknown user")]
    NotFound,
    #[error("the last admin")]
    LastAdmin,
    #[error("deleting oneself")]
    SelfDelete,
    #[error("name taken")]
    NameTaken,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// All users, case-insensitively by name. No password hashes.
pub async fn list_users(db: &PgPool) -> Result<Vec<UserRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.name, u.is_admin, u.created_at, max(t.last_seen_at) AS last_seen_at
         FROM users u LEFT JOIN auth_tokens t ON t.user_id = u.id
         GROUP BY u.id
         ORDER BY lower(u.name), u.id",
    )
    .fetch_all(db)
    .await
}

pub async fn find_user(db: &PgPool, user_id: i32) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.name, u.is_admin, u.created_at, max(t.last_seen_at) AS last_seen_at
         FROM users u LEFT JOIN auth_tokens t ON t.user_id = u.id
         WHERE u.id = $1
         GROUP BY u.id",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// [`create_user`] with a taken name as its own error (the unique index decides, not a prior
/// lookup, so two parallel creations cannot both pass).
pub async fn add_user(
    db: &PgPool,
    name: &str,
    password: &str,
    is_admin: bool,
) -> Result<i32, DirectoryError> {
    create_user(db, name, password, is_admin)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_unique_violation() => DirectoryError::NameTaken,
            _ => DirectoryError::Db(e),
        })
}

/// The admin ids, locked until the transaction ends.
async fn lock_admins(tx: &mut Transaction<'_, Postgres>) -> Result<Vec<i32>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users WHERE is_admin FOR UPDATE")
        .fetch_all(&mut **tx)
        .await
}

/// Sets the role and returns the resulting `is_admin`. Demoting the only admin is refused.
pub async fn set_admin(db: &PgPool, user_id: i32, is_admin: bool) -> Result<bool, DirectoryError> {
    let mut tx = db.begin().await?;
    let resulting = set_admin_in(&mut tx, user_id, is_admin).await?;
    tx.commit().await?;
    Ok(resulting)
}

async fn set_admin_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i32,
    is_admin: bool,
) -> Result<bool, DirectoryError> {
    if !is_admin {
        let admins = lock_admins(tx).await?;
        if admins.len() <= 1 && admins.contains(&user_id) {
            return Err(DirectoryError::LastAdmin);
        }
    }
    let row: Option<bool> =
        sqlx::query_scalar("UPDATE users SET is_admin = $1 WHERE id = $2 RETURNING is_admin")
            .bind(is_admin)
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;
    row.ok_or(DirectoryError::NotFound)
}

/// Deletes `user_id` on behalf of `actor_id`. Nobody deletes themselves (that would also be the
/// only way to delete the last admin through the admin-only routes); the last admin is refused
/// anyway, for callers without an actor such as tests and scripts. Tokens, progress, playlists,
/// favorites and bookmarks go with the user (`ON DELETE CASCADE`); their metadata edits stay,
/// without author.
pub async fn delete_user(
    db: &PgPool,
    actor_id: Option<i32>,
    user_id: i32,
) -> Result<String, DirectoryError> {
    if actor_id == Some(user_id) {
        return Err(DirectoryError::SelfDelete);
    }
    let mut tx = db.begin().await?;
    let admins = lock_admins(&mut tx).await?;
    if admins.len() <= 1 && admins.contains(&user_id) {
        return Err(DirectoryError::LastAdmin);
    }
    let name: Option<String> = sqlx::query_scalar("DELETE FROM users WHERE id = $1 RETURNING name")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    let name = name.ok_or(DirectoryError::NotFound)?;
    tx.commit().await?;
    Ok(name)
}

/// Sets a new password and logs the user out everywhere except `keep` (the caller's own session,
/// when someone changes their own password). A password set for someone else ends all of that
/// user's sessions: whoever knew the old one is out.
pub async fn reset_password(
    db: &PgPool,
    user_id: i32,
    password: &str,
    keep: Option<&str>,
) -> Result<(), DirectoryError> {
    let hash = hash_password(password).await;
    let mut tx = db.begin().await?;
    set_hash_in(&mut tx, user_id, &hash, keep).await?;
    tx.commit().await?;
    Ok(())
}

async fn set_hash_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i32,
    hash: &str,
    keep: Option<&str>,
) -> Result<(), DirectoryError> {
    let updated = sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(hash)
        .bind(user_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(DirectoryError::NotFound);
    }
    sqlx::query("DELETE FROM auth_tokens WHERE user_id = $1 AND value IS DISTINCT FROM $2")
        .bind(user_id)
        .bind(keep)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// `PATCH /users/{id}` with both a role and a password: one transaction, so a refused demotion
/// (last admin) leaves the password untouched, and a missing user changes nothing at all. The
/// hash is computed before the transaction opens — argon2 takes long, the row locks should not.
pub async fn update_user(
    db: &PgPool,
    user_id: i32,
    is_admin: Option<bool>,
    password: Option<&str>,
    keep: Option<&str>,
) -> Result<(), DirectoryError> {
    let hash = match password {
        Some(password) => Some(hash_password(password).await),
        None => None,
    };
    let mut tx = db.begin().await?;
    if let Some(is_admin) = is_admin {
        set_admin_in(&mut tx, user_id, is_admin).await?;
    }
    if let Some(hash) = hash {
        set_hash_in(&mut tx, user_id, &hash, keep).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A name or password length as legacy counted it: JS `length`, UTF-16 code units.
pub fn js_length(value: &str) -> usize {
    value.encode_utf16().count()
}

pub const NAME_MAX: usize = 100;
pub const PASSWORD_MIN: usize = 8;
pub const PASSWORD_MAX: usize = 200;

pub fn valid_name(name: &str) -> bool {
    (1..=NAME_MAX).contains(&js_length(name))
}

pub fn valid_password(password: &str) -> bool {
    (PASSWORD_MIN..=PASSWORD_MAX).contains(&js_length(password))
}

/// What `autoreverse-server set-password <name>` did.
#[derive(Debug, PartialEq, Eq)]
pub enum SetPasswordOutcome {
    Updated,
    Created,
}

/// Legacy `scripts/set-password.ts`: sets the password of `name` (and with `admin` makes them
/// admin), or creates the user. Unlike the script it also ends the user's sessions — someone who
/// needs this has usually lost the old password, or fears someone else has it.
pub async fn set_password_or_create(
    db: &PgPool,
    name: &str,
    password: &str,
    admin: bool,
) -> Result<SetPasswordOutcome, DirectoryError> {
    let existing: Option<i32> = sqlx::query_scalar("SELECT id FROM users WHERE name = $1")
        .bind(name)
        .fetch_optional(db)
        .await?;
    match existing {
        Some(id) => {
            reset_password(db, id, password, None).await?;
            if admin {
                set_admin(db, id, true).await?;
            }
            Ok(SetPasswordOutcome::Updated)
        }
        None => {
            add_user(db, name, password, admin).await?;
            Ok(SetPasswordOutcome::Created)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_count_like_js() {
        assert!(valid_name("a"));
        assert!(!valid_name(""));
        assert!(valid_name(&"ä".repeat(100)));
        assert!(!valid_name(&"x".repeat(101)));
        // One emoji is two UTF-16 units: 50 of them are 100.
        assert!(valid_name(&"🎧".repeat(50)));
        assert!(!valid_name(&"🎧".repeat(51)));
        assert!(!valid_password("1234567"));
        assert!(valid_password("12345678"));
        assert!(valid_password(&"x".repeat(200)));
        assert!(!valid_password(&"x".repeat(201)));
        assert!(!valid_password(&"🎧".repeat(101)));
    }
}
