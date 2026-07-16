//! PostgreSQL-backed user repository.
//!
//! Implements the `UserRepository` trait from the domain layer using SQLx
//! to query and persist user records in the `identity.users` table.

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::identity::{AuthenticationError, UserId, UserRepository};

/// PostgreSQL implementation of the `UserRepository` trait.
#[derive(Clone)]
pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn find_or_create_by_external_id(
        &self,
        external_id: &str,
        email: Option<&str>,
        name: Option<&str>,
    ) -> Result<UserId, AuthenticationError> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO identity.users (id, external_id, email, display_name, created_at, updated_at)
            VALUES (gen_random_uuid(), $1, $2, $3, now(), now())
            ON CONFLICT (external_id) DO UPDATE
                SET email = COALESCE(EXCLUDED.email, identity.users.email),
                    display_name = COALESCE(EXCLUDED.display_name, identity.users.display_name),
                    updated_at = now()
            RETURNING id
            "#,
        )
        .bind(external_id)
        .bind(email)
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AuthenticationError::PersistenceError(format!("user upsert failed: {e}")))?;

        Ok(UserId::new(row.0))
    }
}
