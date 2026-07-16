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
    /// 5. Insert initial route version
    /// 6. Update activity with current_route_version_id and recorded_summary_json
    /// 7. Update import status to Completed
    /// 8. Write audit event
    /// 9. Write outbox event (ImportedActivityCommitted)
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
        let geometry_json =
            serde_json::to_value(&data.segments).map_err(|e| ImportError::StorageError {
                message: format!("failed to serialize segments: {e}"),
            })?;

        let bbox_json =
            serde_json::to_value(data.bounding_box).map_err(|e| ImportError::StorageError {
                message: format!("failed to serialize bounding_box: {e}"),
            })?;

        sqlx::query(
            r#"
            INSERT INTO recorded_activity.recorded_tracks (
                id, source_revision_id, geometry_json, bounding_box_json,
                started_at, ended_at, distance_meters, elevation_gain_meters,
                elevation_loss_meters, point_count, segment_count,
                calculation_version, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
            "#,
        )
        .bind(data.recorded_track_id.0)
        .bind(data.source_revision_id.0)
        .bind(&geometry_json)
        .bind(&bbox_json)
        .bind(data.started_at)
        .bind(data.ended_at)
        .bind(data.statistics.distance_meters)
        .bind(data.statistics.elevation_gain_meters)
        .bind(data.statistics.elevation_loss_meters)
        .bind(data.statistics.point_count as i32)
        .bind(data.statistics.segment_count as i32)
        .bind(&data.parser_version)
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

        // 5. Insert initial route version
        let route_geometry_json = serde_json::to_value(&data.preview_geometry).map_err(|e| {
            ImportError::StorageError {
                message: format!("failed to serialize route version geometry: {e}"),
            }
        })?;

        let route_bbox_json =
            serde_json::to_value(data.bounding_box).map_err(|e| ImportError::StorageError {
                message: format!("failed to serialize route version bounding_box: {e}"),
            })?;

        let statistics_json =
            serde_json::to_value(data.statistics).map_err(|e| ImportError::StorageError {
                message: format!("failed to serialize statistics: {e}"),
            })?;

        let corrected_statistics_json =
            serde_json::to_value(&data.corrected_statistics).map_err(|e| {
                ImportError::StorageError {
                    message: format!("failed to serialize corrected statistics: {e}"),
                }
            })?;

        sqlx::query(
            r#"
            INSERT INTO route_versioning.route_versions (
                id, activity_id, parent_version_id, version_number,
                geometry_json, bounding_box_json, corrected_statistics_json,
                calculation_version, edit_summary, created_by, created_at
            )
            VALUES ($1, $2, NULL, 1, $3, $4, $5, $6, 'Initial import', $7, now())
            "#,
        )
        .bind(data.route_version_id.0)
        .bind(data.activity_id.0)
        .bind(&route_geometry_json)
        .bind(&route_bbox_json)
        .bind(&corrected_statistics_json)
        .bind(&data.parser_version)
        .bind(data.owner_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to insert route_version: {e}"),
        })?;

        // 6. Update activity with current_route_version_id and recorded_summary_json
        sqlx::query(
            r#"
            UPDATE activity_catalog.activities
            SET current_route_version_id = $2, recorded_summary_json = $3, corrected_summary_json = $4, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(data.activity_id.0)
        .bind(data.route_version_id.0)
        .bind(&statistics_json)
        .bind(&corrected_statistics_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| ImportError::StorageError {
            message: format!("failed to update activity with route version: {e}"),
        })?;

        // 7. Update import status to Completed with activity_id
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

        // 8. Write audit event
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

        // 9. Write outbox event (ImportedActivityCommitted)
        let outbox_payload = json!({
            "import_id": data.import_id.0.to_string(),
            "activity_id": data.activity_id.0.to_string(),
            "owner_id": data.owner_id.0.to_string(),
            "route_version_id": data.route_version_id.0.to_string(),
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
