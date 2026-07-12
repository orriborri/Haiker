//! Database connection and migration management.
//!
//! Provides a connection pool backed by SQLx and runs migrations at startup.

use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::info;

use crate::config::DatabaseConfig;

/// Create a PostgreSQL connection pool from the provided configuration.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(&config.url)
        .await?;

    info!(
        max_connections = config.max_connections,
        "Database connection pool established"
    );

    Ok(pool)
}

/// Run all pending database migrations.
///
/// Migrations are embedded at compile time from the `migrations/` directory
/// at the workspace root.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    info!("Running database migrations");

    sqlx::migrate!("../../migrations").run(pool).await?;

    info!("Database migrations completed");
    Ok(())
}
