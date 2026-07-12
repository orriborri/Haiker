//! Transactional commit implementation for imports.
//!
//! Implements the CommitImport trait with a single database transaction
//! that atomically persists data across all three bounded contexts:
//! recorded_activity, activity_catalog, and imports.

use async_trait::async_trait;
use serde_json::json;
use sqlx::PgPool;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::imports::commit::{CommitImport, ImportCommitData};
use haiker_app::imports::ImportError;

use crate::audit::AuditLog;
use crate::outbox::Outbox;

/// PostgreSQL-backed implementation of CommitImport.
///
/// Uses a single transaction to ensure atomicity across all persistence operations.
#[derive(Clone)]
pub struct PgImportCommitter {
    pool: PgPool,
}

impl PgImportCommitter {
    /// Create a new PgImportCommitter with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CommitImport for PgImportCommitter {
    /// Commit the import data atomically in a single transaction.
    ///
    /// Within this transaction:
    /// 1. Insert source_artifact
    /// 2. Insert source_revision
    /// 3. Insert recorded_track
    /// 4. Insert activity
    /// 5. Update import status to Completed
    /// 6. Write audit event
    /// 7. Write outbox event (ImportedActivityCommitted)
    async fn commit(&self, data: &ImportCommitData) -> Result<ActivityId, ImportError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ImportError::StorageError {
                message: format!("failed to begin transaction: {e}"),
            })?;

        // 1. Insert source_artifact
        sqlx::query(
            r#"
            INSERT INTO recorded_activity.source_artifacts (
                id, import_id, owner_id, object_storage_key,
                content_type, size_bytes, checksum, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, now())
            "#,
        )
        .bind(data.source_artifact_id.0)
        .bind(data.import_id.0)
        .bind(data.owner_id.0)
        .bind(&data.object_storage_key)
        .bind(&data.content_type)
        .bind(data.file_size_bytes)
        .bind(&data.checksum)
        .execute(&mut *tx)
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to insert source_artifact: {e}"),
        })?;

        // 2. Insert source_revision
        sqlx::query(
            r#"
            INSERT INTO recorded_activity.source_revisions (
                id, activity_id, source_artifact_id, revision_number,
                provider, parser_version, imported_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, now())
            "#,
        )
        .bind(data.source_revision_id.0)
        .bind(data.activity_id.0)
        .bind(data.source_artifact_id.0)
        .bind(data.revision_number)
        .bind("gpx_import")
        .bind(&data.parser_version)
        .execute(&mut *tx)
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to insert source_revision: {e}"),
        })?;

        // 3. Insert recorded_track
        let segments_json =
            serde_json::to_value(&data.segments).map_err(|e| ImportError::StorageError {
                message: format!("failed to serialize segments: {e}"),
            })?;

        let preview_json = serde_json::to_value(&data.preview_geometry).map_err(|e| {
            ImportError::StorageError {
                message: format!("failed to serialize preview: {e}"),
            }
        })?;

        let stats_json =
            serde_json::to_value(data.statistics).map_err(|e| ImportError::StorageError {
                message: format!("failed to serialize statistics: {e}"),
            })?;

        let bbox_json =
            serde_json::to_value(data.bounding_box).map_err(|e| ImportError::StorageError {
                message: format!("failed to serialize bounding_box: {e}"),
            })?;

        sqlx::query(
            r#"
            INSERT INTO recorded_activity.recorded_tracks (
                id, activity_id, owner_id, segments, bounding_box,
                statistics, preview_geometry, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, now())
            "#,
        )
        .bind(data.recorded_track_id.0)
        .bind(data.activity_id.0)
        .bind(data.owner_id.0)
        .bind(&segments_json)
        .bind(&bbox_json)
        .bind(&stats_json)
        .bind(&preview_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to insert recorded_track: {e}"),
        })?;

        // 4. Insert activity
        sqlx::query(
            r#"
            INSERT INTO activity_catalog.activities (
                id, owner_id, title, activity_type,
                started_at, ended_at, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, now(), now())
            "#,
        )
        .bind(data.activity_id.0)
        .bind(data.owner_id.0)
        .bind(&data.activity_title)
        .bind(data.activity_type.to_string())
        .bind(data.started_at)
        .bind(data.ended_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to insert activity: {e}"),
        })?;

        // 5. Update import status to Completed with activity_id
        sqlx::query(
            r#"
            UPDATE imports.imports
            SET status = 'completed', activity_id = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(data.import_id.0)
        .bind(data.activity_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to update import status: {e}"),
        })?;

        // 6. Write audit event
        let audit_metadata = json!({
            "import_id": data.import_id.0.to_string(),
            "activity_id": data.activity_id.0.to_string(),
            "format": "gpx",
        });

        AuditLog::append_in_tx(
            &mut tx,
            data.owner_id.0,
            "import.completed",
            "activity",
            &data.activity_id.0.to_string(),
            Some(audit_metadata),
        )
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to write audit event: {e}"),
        })?;

        // 7. Write outbox event (ImportedActivityCommitted)
        let outbox_payload = json!({
            "import_id": data.import_id.0.to_string(),
            "activity_id": data.activity_id.0.to_string(),
            "owner_id": data.owner_id.0.to_string(),
            "distance_meters": data.statistics.distance_meters,
            "duration_seconds": data.statistics.duration_seconds,
            "elevation_gain_meters": data.statistics.elevation_gain_meters,
        });

        Outbox::publish(
            &mut tx,
            "import",
            &data.import_id.0.to_string(),
            "ImportedActivityCommitted",
            outbox_payload,
            Some(data.correlation_id),
        )
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to write outbox event: {e}"),
        })?;

        // Commit the transaction
        tx.commit().await.map_err(|e| ImportError::StorageError {
            message: format!("failed to commit transaction: {e}"),
        })?;

        Ok(data.activity_id)
    }
}
