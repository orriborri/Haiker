//! PostgreSQL-backed implementation of ImportRepository.
//!
//! Uses raw `sqlx::query()` (not compile-time checked) to avoid needing
//! database connectivity at build time (SQLX_OFFLINE=true compatible).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::identity::UserId;
use haiker_app::imports::checksum::Checksum;
use haiker_app::imports::repository::ImportRepository;
use haiker_app::imports::state_machine::ImportStatus;
use haiker_app::imports::{Import, ImportError, ImportFormat, ImportId};
use haiker_app::recorded_activity::SourceArtifactId;

/// PostgreSQL implementation of the import repository.
#[derive(Clone)]
pub struct PgImportRepository {
    pool: PgPool,
}

impl PgImportRepository {
    /// Create a new PgImportRepository with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Type alias for the row tuple returned by import queries.
type ImportRow = (
    Uuid,
    Uuid,
    Option<Uuid>,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<Uuid>,
    DateTime<Utc>,
    DateTime<Utc>,
);

/// Map a status string from the database to the domain ImportStatus enum.
fn parse_status(s: &str) -> ImportStatus {
    match s {
        "requested" => ImportStatus::Requested,
        "uploading" => ImportStatus::Uploading,
        "uploaded" => ImportStatus::Uploaded,
        "validating" => ImportStatus::Validating,
        "queued" => ImportStatus::Queued,
        "parsing" => ImportStatus::Parsing,
        "committing" => ImportStatus::Committing,
        "completed" => ImportStatus::Completed,
        "failed" => ImportStatus::Failed,
        "cancelled" => ImportStatus::Cancelled,
        _ => ImportStatus::Failed, // Defensive fallback
    }
}

/// Map a row tuple to an Import domain object.
fn row_to_import(row: ImportRow) -> Import {
    let (
        id,
        owner_id,
        source_artifact_id,
        format,
        status,
        checksum,
        failure_reason,
        idempotency_key,
        payload_hash,
        activity_id,
        created_at,
        updated_at,
    ) = row;

    Import {
        id: ImportId::new(id),
        owner_id: UserId::new(owner_id),
        source_artifact_id: source_artifact_id.map(SourceArtifactId::new),
        format: match format.as_str() {
            "gpx" => ImportFormat::Gpx,
            _ => ImportFormat::Gpx, // Defensive fallback
        },
        status: parse_status(&status),
        checksum: checksum.and_then(|c| Checksum::new(c).ok()),
        failure_reason,
        idempotency_key,
        payload_hash,
        activity_id: activity_id.map(ActivityId::new),
        created_at,
        updated_at,
    }
}

#[async_trait]
impl ImportRepository for PgImportRepository {
    async fn save(&self, import: &Import) -> Result<(), ImportError> {
        sqlx::query(
            r#"
            INSERT INTO imports.imports (
                id, owner_id, source_artifact_id, format, status,
                checksum, failure_reason, idempotency_key, payload_hash,
                activity_id, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(import.id.0)
        .bind(import.owner_id.0)
        .bind(import.source_artifact_id.map(|a| a.0))
        .bind(import.format.to_string())
        .bind(import.status.to_string())
        .bind(import.checksum.as_ref().map(|c| c.as_str().to_string()))
        .bind(&import.failure_reason)
        .bind(&import.idempotency_key)
        .bind(&import.payload_hash)
        .bind(import.activity_id.map(|a| a.0))
        .bind(import.created_at)
        .bind(import.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| ImportError::StorageError {
            message: e.to_string(),
        })?;

        Ok(())
    }

    async fn find_by_id(&self, id: ImportId) -> Result<Option<Import>, ImportError> {
        let row = sqlx::query_as::<_, ImportRow>(
            r#"
            SELECT id, owner_id, source_artifact_id, format, status,
                   checksum, failure_reason, idempotency_key, payload_hash,
                   activity_id, created_at, updated_at
            FROM imports.imports
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ImportError::StorageError {
            message: e.to_string(),
        })?;

        Ok(row.map(row_to_import))
    }

    async fn find_by_idempotency_key(
        &self,
        owner_id: UserId,
        key: &str,
    ) -> Result<Option<Import>, ImportError> {
        let row = sqlx::query_as::<_, ImportRow>(
            r#"
            SELECT id, owner_id, source_artifact_id, format, status,
                   checksum, failure_reason, idempotency_key, payload_hash,
                   activity_id, created_at, updated_at
            FROM imports.imports
            WHERE owner_id = $1 AND idempotency_key = $2
            "#,
        )
        .bind(owner_id.0)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ImportError::StorageError {
            message: e.to_string(),
        })?;

        Ok(row.map(row_to_import))
    }

    async fn find_by_checksum(
        &self,
        owner_id: UserId,
        checksum: &Checksum,
    ) -> Result<Option<Import>, ImportError> {
        let row = sqlx::query_as::<_, ImportRow>(
            r#"
            SELECT id, owner_id, source_artifact_id, format, status,
                   checksum, failure_reason, idempotency_key, payload_hash,
                   activity_id, created_at, updated_at
            FROM imports.imports
            WHERE owner_id = $1 AND checksum = $2 AND status = 'completed'
            "#,
        )
        .bind(owner_id.0)
        .bind(checksum.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ImportError::StorageError {
            message: e.to_string(),
        })?;

        Ok(row.map(row_to_import))
    }

    async fn find_completed_by_checksum(
        &self,
        owner_id: UserId,
        checksum: &Checksum,
    ) -> Result<Option<(ImportId, Option<ActivityId>)>, ImportError> {
        let row = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
            r#"
            SELECT id, activity_id
            FROM imports.imports
            WHERE owner_id = $1 AND checksum = $2 AND status = 'completed'
            "#,
        )
        .bind(owner_id.0)
        .bind(checksum.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ImportError::StorageError {
            message: e.to_string(),
        })?;

        Ok(row.map(|(id, activity_id)| (ImportId::new(id), activity_id.map(ActivityId::new))))
    }

    async fn update(&self, import: &Import) -> Result<(), ImportError> {
        sqlx::query(
            r#"
            UPDATE imports.imports
            SET source_artifact_id = $2,
                status = $3,
                checksum = $4,
                failure_reason = $5,
                activity_id = $6,
                updated_at = $7
            WHERE id = $1
            "#,
        )
        .bind(import.id.0)
        .bind(import.source_artifact_id.map(|a| a.0))
        .bind(import.status.to_string())
        .bind(import.checksum.as_ref().map(|c| c.as_str().to_string()))
        .bind(&import.failure_reason)
        .bind(import.activity_id.map(|a| a.0))
        .bind(import.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| ImportError::StorageError {
            message: e.to_string(),
        })?;

        Ok(())
    }
}
