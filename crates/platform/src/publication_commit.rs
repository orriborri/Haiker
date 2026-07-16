//! Transactional commit implementation for route version publication.
//!
//! Implements the CommitPublication trait with a single database transaction
//! that atomically persists the new route version, updates the activity pointer,
//! marks the draft as published, writes audit and outbox events.
//!
//! # Relationship to publish_command.rs (domain orchestrator)
//!
//! The `execute_publish` function in `publish_command.rs` serves as the domain
//! specification and test harness for the publication workflow. It defines the
//! business rules and invariants using pure domain types and repository traits.
//!
//! This committer (`PgPublicationCommitter`) is the production implementation
//! that must maintain behavioral equivalence with `execute_publish`. It
//! reimplements the logic in SQL within a single transaction for atomicity
//! across schemas (route_versioning, route_editing, activity_catalog, audit,
//! outbox). This is the standard pattern in the codebase -- see
//! `import_commit.rs` which similarly reimplements the domain import logic
//! in SQL for transactional guarantees.
//!
//! When modifying publication rules, update both paths and ensure the domain
//! tests in `publish_command.rs` still pass as a specification check.

use async_trait::async_trait;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
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

/// Compute a deterministic fingerprint of the publication request payload.
///
/// Used to detect idempotency conflicts: same key but different parameters
/// should return 409 IDEMPOTENCY_CONFLICT. The fingerprint captures the
/// fields that define "same request": draft_id, expected_revision, and edit_summary.
fn compute_payload_fingerprint(data: &PublicationCommitData) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.draft_id.0.hash(&mut hasher);
    data.expected_revision.hash(&mut hasher);
    data.edit_summary.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
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

        // 1. Check idempotency: look for an existing outbox event with this correlation_id.
        // The idempotency key is validated as a UUID at the API layer
        // (extract_uuid_idempotency_key), so parse_str should always succeed here.
        // The unwrap_or fallback is retained as defense-in-depth only.
        let correlation_id = Uuid::parse_str(&data.idempotency_key).unwrap_or_else(|_| {
            // Defense-in-depth: API layer rejects non-UUID keys, but if one somehow
            // reaches here, derive deterministically. This path should be unreachable.
            let bytes = data.idempotency_key.as_bytes();
            let mut hash = [0u8; 16];
            for (i, b) in bytes.iter().enumerate() {
                hash[i % 16] ^= *b;
            }
            Uuid::from_bytes(hash)
        });

        // Compute the payload fingerprint for idempotency conflict detection
        let fingerprint = compute_payload_fingerprint(data);

        let existing_event = sqlx::query_as::<_, (String, serde_json::Value)>(
            r#"
            SELECT aggregate_id, payload
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

        if let Some((aggregate_id, payload)) = existing_event {
            // Check if the payload fingerprint matches. If different, this is
            // a key reuse with different parameters -> 409 IDEMPOTENCY_CONFLICT.
            if let Some(stored_fingerprint) = payload.get("_fingerprint").and_then(|v| v.as_str()) {
                if stored_fingerprint != fingerprint {
                    return Err(RouteVersioningError::IdempotencyConflict);
                }
            }

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

        // 2. Load and lock the draft, resolving activity_id from the locked row.
        // This eliminates the TOCTOU window: activity_id comes from the same row
        // that is locked for the duration of the transaction.
        let draft_row = sqlx::query_as::<_, (Uuid, Uuid, String, i64, serde_json::Value)>(
            r#"
            SELECT owner_id, activity_id, state, revision, geometry_json
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

        let (owner_id, activity_id_raw, state, revision, geometry_json) = draft_row;
        let activity_id = ActivityId(activity_id_raw);

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

        // 4. Compute geometry, bounding box, and statistics from draft geometry.
        // The geometry_json is stored as a JSON array of segments.
        // GeometryPoint includes an optional elevation field so that elevation
        // data from the draft is preserved in the stored version geometry_json.
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
        .bind(activity_id.0)
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
        .bind(activity_id.0)
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
        .bind(activity_id.0)
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
            "activity_id": activity_id.0.to_string(),
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

        // 10. Write outbox event (RouteVersionPublished).
        // The payload includes a `_fingerprint` field used for idempotency conflict
        // detection. On replay, if the stored fingerprint differs from the incoming
        // request's fingerprint, we return IdempotencyConflict (409).
        let outbox_payload = json!({
            "route_version_id": new_version_id.to_string(),
            "activity_id": activity_id.0.to_string(),
            "draft_id": data.draft_id.0.to_string(),
            "version_number": new_version_number,
            "actor_id": data.actor_id.0.to_string(),
            "_fingerprint": fingerprint,
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
///
/// Includes an optional `elevation` field to preserve elevation data from the
/// draft's RoutePoint in the stored geometry_json of the version. The elevation
/// is carried through via the raw `geometry_json` column (which stores the
/// original segments including all fields), so no data is lost even though
/// the flat_coords tuple used for distance/bbox computation only uses lat/lon.
#[derive(Debug, serde::Deserialize)]
struct GeometryPoint {
    latitude: f64,
    longitude: f64,
    /// Elevation in meters, preserved in the stored geometry_json when present.
    #[allow(dead_code)]
    elevation: Option<f64>,
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
