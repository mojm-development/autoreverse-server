//! Postgres pool and migrations.

use sqlx::PgPool;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;

/// The migrations in `migrations/`, compiled into the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

/// Applied on every start. The schema is owned by this binary; there is no separate migrate step.
pub async fn migrate(db: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(db).await
}
