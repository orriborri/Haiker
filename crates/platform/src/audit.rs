//! Audit log for tracking user actions on resources.
//!
//! Provides append-only storage for audit events with transactional support
//! so audit records can be committed atomically with domain state changes.

use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Audit log backed by PostgreSQL.
#[derive(Clone)]
pub struct AuditLog {
    pool: PgPool,
}

impl AuditLog {
    /// Create a new audit log instance.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Append an audit record.
    ///
    /// Inserts a new audit entry and returns its generated ID.
    /// The metadata must be privacy-safe (no coordinates, file content, or PII
    /// beyond user identifiers).
    pub async fn append(
        &self,
        actor_id: Uuid,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        metadata: Option<Value>,
    ) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query_as::<_, (Uuid,)>(
            r#"
            INSERT INTO platform.audit_log (actor_id, action, resource_type, resource_id, metadata)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(actor_id)
        .bind(action)
        .bind(resource_type)
        .bind(resource_id)
        .bind(&metadata)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Append an audit record within an existing transaction.
    ///
    /// Use this to ensure the audit record is committed atomically with
    /// domain state changes. The metadata must be privacy-safe (no coordinates,
    /// file content, or PII beyond user identifiers).
    pub async fn append_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        actor_id: Uuid,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        metadata: Option<Value>,
    ) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query_as::<_, (Uuid,)>(
            r#"
            INSERT INTO platform.audit_log (actor_id, action, resource_type, resource_id, metadata)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(actor_id)
        .bind(action)
        .bind(resource_type)
        .bind(resource_id)
        .bind(&metadata)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row.0)
    }
}
