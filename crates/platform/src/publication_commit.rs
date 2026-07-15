//! Transactional commit implementation for route version publication.
//!
//! Implements the CommitPublication trait with a single database transaction
//! that atomically persists the new route version, updates the activity pointer,
//! marks the draft as published, writes audit and outbox events.

use async_trait::async_trait;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::route_versioning::commit::{
    CommitPublication, PublicationCommitData, PublicationResult,
};
use haiker_app::route_versioning::{RouteVersionId, RouteVersioningError};

use crate::audit::AuditLog;
use crate::outbox::Outbox;

/// PostgreSQL-backed implementation of CommitPublication.
///
/// Uses a single transaction to ensure atomicity across all persistence operations.
#[derive(Clone)]
pub struct PgPublicationCommitter {
    pool: PgPool,
}

impl PgPublicationCommitter {
    /// Create a new PgPublicationCommitter with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CommitPublication for PgPublicationCommitter {
    /// Commit the publication data atomically in a single transaction.
    ///
    /// Within this transaction:
    /// 1. Check idempotency via outbox correlation_id
    /// 2. Load and lock the draft (SELECT FOR UPDATE)
    /// 3. Validate owner, state, and expected revision
    /// 4. Load draft geometry and compute bounding box/statistics
    /// 5. Find latest version number for the activity
    /// 6. INSERT into route_versioning.route_versions
    /// 7. UPDATE activity_catalog.activities SET current_route_version_id
    /// 8. UPDATE route_editing.route_drafts SET state = 'published'
    /// 9. Write audit event
    /// 10. Write outbox event (RouteVersionPublished)
    async fn commit(
        &self,
        data: &PublicationCommitData,
    ) -> Result<PublicationResult, RouteVersioningError> {
        let mut tx =
            self.pool
                .begin()
                .await
                .map_err(|e| RouteVersioningError::PersistenceError {
                    message: format!("failed to begin transaction: {e}"),
                })?;

        // 1. Check idempotency: look for an existing outbox event with this correlation_id
        let correlation_id = Uuid::parse_str(&data.idempotency_key).unwrap_or_else(|_| {
            // If the key is not a valid UUID, derive one deterministically
            let bytes = data.idempotency_key.as_bytes();
            let mut hash = [0u8; 16];
            for (i, b) in bytes.iter().enumerate() {
                hash[i % 16] ^= *b;
            }
            Uuid::from_bytes(hash)
        });

        let existing_event = sqlx::query_as::<_, (String,)>(
            r#"
            SELECT aggregate_id
            FROM platform.outbox
            WHERE aggregate_type = 'route_version' AND correlation_id = $1
            "#,
        )
        .bind(correlation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to check idempotency: {e}"),
        })?;

        if let Some((aggregate_id,)) = existing_event {
            // Idempotent replay: look up the existing route version
            let version_id = Uuid::parse_str(&aggregate_id).map_err(|e| {
                RouteVersioningError::PersistenceError {
                    message: format!("invalid aggregate_id in outbox: {e}"),
                }
            })?;

            let row = sqlx::query_as::<_, (i32,)>(
                r#"
                SELECT version_number
                FROM route_versioning.route_versions
                WHERE id = $1
                "#,
            )
            .bind(version_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RouteVersioningError::PersistenceError {
                message: format!("failed to fetch existing version: {e}"),
            })?;

            if let Some((version_number,)) = row {
                return Ok(PublicationResult {
                    route_version_id: RouteVersionId::new(version_id),
                    version_number,
                    draft_id: data.draft_id,
                });
            }
        }

        // 2. Load and lock the draft
        let draft_row = sqlx::query_as::<_, (Uuid, String, i64, serde_json::Value)>(
            r#"
            SELECT owner_id, state, revision, geometry_json
            FROM route_editing.route_drafts
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(data.draft_id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to load draft: {e}"),
        })?
        .ok_or(RouteVersioningError::DraftNotFound)?;

        let (owner_id, state, revision, geometry_json) = draft_row;

        // 3. Validate owner, state, and expected revision
        if owner_id != data.actor_id.0 {
            return Err(RouteVersioningError::NotAuthorized);
        }

        if state != "active" {
            return Err(RouteVersioningError::DraftNotActive);
        }

        if revision as u64 != data.expected_revision {
            return Err(RouteVersioningError::RevisionConflict {
                expected: data.expected_revision,
                actual: revision as u64,
            });
        }

        // 4. Compute geometry, bounding box, and statistics from draft geometry
        // The geometry_json is stored as a JSON array of segments
        let segments: Vec<Vec<GeometryPoint>> = serde_json::from_value(geometry_json.clone())
            .map_err(|e| RouteVersioningError::ValidationFailed {
                errors: vec![format!("failed to parse draft geometry: {e}")],
            })?;

        let flat_coords: Vec<(f64, f64)> = segments
            .iter()
            .flat_map(|seg| seg.iter())
            .map(|p| (p.latitude, p.longitude))
            .collect();

        if flat_coords.len() < 2 {
            return Err(RouteVersioningError::ValidationFailed {
                errors: vec!["geometry must have at least 2 points".to_string()],
            });
        }

        // Compute bounding box
        let min_lat = flat_coords
            .iter()
            .map(|(lat, _)| *lat)
            .fold(f64::MAX, f64::min);
        let max_lat = flat_coords
            .iter()
            .map(|(lat, _)| *lat)
            .fold(f64::MIN, f64::max);
        let min_lon = flat_coords
            .iter()
            .map(|(_, lon)| *lon)
            .fold(f64::MAX, f64::min);
        let max_lon = flat_coords
            .iter()
            .map(|(_, lon)| *lon)
            .fold(f64::MIN, f64::max);

        let bounding_box_json = json!({
            "south_west": {"latitude": min_lat, "longitude": min_lon},
            "north_east": {"latitude": max_lat, "longitude": max_lon}
        });

        // Compute total distance using haversine
        let total_distance = compute_total_distance(&flat_coords);
        let corrected_statistics_json = json!({
            "distance_meters": total_distance,
            "point_count": flat_coords.len()
        });

        // 5. Find latest version number for the activity
        let latest_row = sqlx::query_as::<_, (Uuid, i32)>(
            r#"
            SELECT id, version_number
            FROM route_versioning.route_versions
            WHERE activity_id = $1
            ORDER BY version_number DESC
            LIMIT 1
            "#,
        )
        .bind(data.activity_id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to find latest version: {e}"),
        })?
        .ok_or(RouteVersioningError::ActivityNotFound)?;

        let (parent_version_id, latest_version_number) = latest_row;
        let new_version_number = latest_version_number + 1;
        let new_version_id = Uuid::new_v4();

        // 6. INSERT into route_versioning.route_versions
        sqlx::query(
            r#"
            INSERT INTO route_versioning.route_versions (
                id, activity_id, parent_version_id, version_number,
                geometry_json, bounding_box_json, corrected_statistics_json,
                calculation_version, edit_summary, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now())
            "#,
        )
        .bind(new_version_id)
        .bind(data.activity_id.0)
        .bind(parent_version_id)
        .bind(new_version_number)
        .bind(&geometry_json)
        .bind(&bounding_box_json)
        .bind(&corrected_statistics_json)
        .bind("v1.0")
        .bind(&data.edit_summary)
        .bind(data.actor_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to insert route version: {e}"),
        })?;

        // 7. UPDATE activity_catalog.activities SET current_route_version_id
        sqlx::query(
            r#"
            UPDATE activity_catalog.activities
            SET current_route_version_id = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(data.activity_id.0)
        .bind(new_version_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to update activity current version: {e}"),
        })?;

        // 8. UPDATE route_editing.route_drafts SET state = 'published'
        sqlx::query(
            r#"
            UPDATE route_editing.route_drafts
            SET state = 'published', updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(data.draft_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to update draft state: {e}"),
        })?;

        // 9. Write audit event
        let audit_metadata = json!({
            "draft_id": data.draft_id.0.to_string(),
            "activity_id": data.activity_id.0.to_string(),
            "version_number": new_version_number,
        });

        AuditLog::append_in_tx(
            &mut tx,
            data.actor_id.0,
            "route_version.published",
            "route_version",
            &new_version_id.to_string(),
            Some(audit_metadata),
        )
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to write audit event: {e}"),
        })?;

        // 10. Write outbox event (RouteVersionPublished)
        let outbox_payload = json!({
            "route_version_id": new_version_id.to_string(),
            "activity_id": data.activity_id.0.to_string(),
            "draft_id": data.draft_id.0.to_string(),
            "version_number": new_version_number,
            "actor_id": data.actor_id.0.to_string(),
        });

        Outbox::publish(
            &mut tx,
            "route_version",
            &new_version_id.to_string(),
            "RouteVersionPublished",
            outbox_payload,
            Some(correlation_id),
        )
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to write outbox event: {e}"),
        })?;

        // Commit the transaction
        tx.commit()
            .await
            .map_err(|e| RouteVersioningError::PersistenceError {
                message: format!("failed to commit transaction: {e}"),
            })?;

        Ok(PublicationResult {
            route_version_id: RouteVersionId::new(new_version_id),
            version_number: new_version_number,
            draft_id: data.draft_id,
        })
    }
}

/// A geometry point for deserialization from draft geometry JSON.
#[derive(Debug, serde::Deserialize)]
struct GeometryPoint {
    latitude: f64,
    longitude: f64,
}

/// Compute total distance in meters using haversine formula.
fn compute_total_distance(coords: &[(f64, f64)]) -> f64 {
    if coords.len() < 2 {
        return 0.0;
    }

    let mut total = 0.0;
    for window in coords.windows(2) {
        total += haversine_distance(window[0].0, window[0].1, window[1].0, window[1].1);
    }
    total
}

/// Haversine distance between two coordinates in meters.
fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0; // Earth radius in meters
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let h =
        (dlat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}
