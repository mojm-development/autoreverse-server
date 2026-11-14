//! First admin from the environment (legacy `bootstrap.ts`).

use sqlx::PgPool;

use super::passwords::create_user;

#[derive(Debug, PartialEq, Eq)]
pub enum Bootstrap {
    /// Users exist already; the environment is ignored.
    NotNeeded,
    Created(i32),
    /// Empty table and no credentials in the environment — nobody can log in.
    MissingCredentials,
}

/// Creates the first admin from `AUTOREVERSE_ADMIN_USER` / `AUTOREVERSE_ADMIN_PASSWORD`, but only
/// while the `users` table is empty: changing the variables later must not create accounts.
pub async fn ensure_first_admin(
    db: &PgPool,
    name: Option<&str>,
    password: Option<&str>,
) -> Result<Bootstrap, sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(db)
        .await?;
    if count > 0 {
        return Ok(Bootstrap::NotNeeded);
    }
    let (Some(name), Some(password)) = (
        name.filter(|n| !n.is_empty()),
        password.filter(|p| !p.is_empty()),
    ) else {
        tracing::error!(
            "Kein Nutzer vorhanden und AUTOREVERSE_ADMIN_USER/AUTOREVERSE_ADMIN_PASSWORD nicht gesetzt — \
             Server startet trotzdem, aber niemand kann sich anmelden."
        );
        return Ok(Bootstrap::MissingCredentials);
    };
    let id = create_user(db, name, password, true).await?;
    tracing::info!("Ersten Verwalter '{name}' aus der Umgebung angelegt.");
    Ok(Bootstrap::Created(id))
}
