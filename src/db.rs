//! Postgres pool and migrations.

use sqlx::PgPool;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::PgPoolOptions;

/// The migrations in `migrations/`, compiled into the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// What a server started against the database of the old TypeScript version says. The schema
/// was started afresh on purpose (no upgrade path); this replaces the bare "relation users
/// already exists" the first migration would otherwise die with.
pub const LEGACY_DATABASE: &str = "Die Datenbank stammt von der alten Autoreverse-Version \
(SvelteKit/Drizzle) und wird nicht übernommen. Lege eine neue, leere Datenbank an und trage sie \
in DATABASE_URL ein, setze danach unter Einstellungen → Bibliotheken die Pfade, scanne neu und \
lege die Nutzer mit `autoreverse-server set-password <name> --admin` an.";

#[derive(Debug, thiserror::Error)]
pub enum MigrateSetupError {
    #[error("{}", LEGACY_DATABASE)]
    LegacyDatabase,
    #[error("Datenbank nicht lesbar: {0}")]
    Db(#[from] sqlx::Error),
    #[error("Migration fehlgeschlagen: {0}")]
    Migrate(#[from] MigrateError),
}

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

/// A database the old version set up: its tables (`drizzle-kit push` leaves no history) or
/// Drizzle's migration journal, but no sqlx history of ours.
pub async fn is_legacy_database(db: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT (to_regclass('public.users') IS NOT NULL
                 OR to_regclass('drizzle.__drizzle_migrations') IS NOT NULL)
            AND to_regclass('public._sqlx_migrations') IS NULL",
    )
    .fetch_one(db)
    .await
}

/// Applied on every start. The schema is owned by this binary; there is no separate migrate step.
pub async fn migrate(db: &PgPool) -> Result<(), MigrateSetupError> {
    if is_legacy_database(db).await? {
        return Err(MigrateSetupError::LegacyDatabase);
    }
    MIGRATOR.run(db).await?;
    Ok(())
}
