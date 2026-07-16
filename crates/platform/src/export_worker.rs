//! Worker job handler for GPX export generation.
//!
//! Implements JobHandler for the 'generate_export' job type, connecting the
//! platform infrastructure to the domain GPX generator. Handles idempotent
//! retries, state transitions, and failure recovery.

use async_trait::async_trait;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::exports::state_machine::ExportStatus;
use haiker_app::exports::{
    generate_gpx, ExportFormat, ExportJob, ExportJobId, GenerateExportJob, GpxGeneratorInput,
    GpxPoint, GENERATE_EXPORT_JOB_TYPE,
};

use crate::job_queue::{Job, JobHandler};
use crate::metrics;
use crate::object_storage::ObjectStorageClient;

/// Job handler for generating GPX export files.
///
/// Fetches route version geometry from the database, generates GPX using the
/// domain generator on a blocking task, computes a SHA-256 checksum, uploads
/// to object storage with an idempotent key, and updates the export job state.
pub struct GenerateExportJobHandler {
    pool: PgPool,
    object_storage: ObjectStorageClient,
}

impl GenerateExportJobHandler {
    /// Create a new GenerateExportJobHandler.
    pub fn new(pool: PgPool, object_storage: ObjectStorageClient) -> Self {
        Self {
            pool,
            object_storage,
        }
    }
}

/// Construct the idempotent object storage key for an export.
fn storage_key(owner_id: Uuid, export_job_id: Uuid) -> String {
    format!("exports/{}/{}.gpx", owner_id, export_job_id)
}

/// Compute the SHA-256 hex digest of a byte slice.
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Geometry point as stored in the database JSON column.
#[derive(Debug, serde::Deserialize)]
struct GeometryPoint {
    latitude: f64,
    longitude: f64,
    elevation: Option<f64>,
}

/// Parse geometry_json from the database into segments of GpxPoints.
///
/// The geometry_json may be stored in two formats:
/// - Segmented: `[[{latitude, longitude, elevation?}, ...], ...]`
/// - Flat (legacy/initial imports): `[{latitude, longitude}, ...]`
///
/// This function handles both formats gracefully.
fn parse_geometry_json(raw: &serde_json::Value) -> Result<Vec<Vec<GpxPoint>>, String> {
    // Try segmented format first: Vec<Vec<GeometryPoint>>
    if let Ok(segments) = serde_json::from_value::<Vec<Vec<GeometryPoint>>>(raw.clone()) {
        let result = segments
            .into_iter()
            .map(|seg| {
                seg.into_iter()
                    .map(|p| GpxPoint {
                        latitude: p.latitude,
                        longitude: p.longitude,
                        elevation: p.elevation,
                    })
                    .collect()
            })
            .collect();
        return Ok(result);
    }

    // Try flat format: Vec<GeometryPoint> (wrap in a single segment)
    if let Ok(points) = serde_json::from_value::<Vec<GeometryPoint>>(raw.clone()) {
        let segment: Vec<GpxPoint> = points
            .into_iter()
            .map(|p| GpxPoint {
                latitude: p.latitude,
                longitude: p.longitude,
                elevation: p.elevation,
            })
            .collect();
        return Ok(vec![segment]);
    }

    Err("geometry_json is neither a segmented array nor a flat point array".to_string())
}

/// Load an export job from the database.
async fn load_export_job(
    pool: &PgPool,
    export_job_id: Uuid,
) -> Result<ExportJob, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_as::<
        _,
        (
            Uuid,                          // id
            Uuid,                          // activity_id
            Uuid,                          // route_version_id
            Uuid,                          // requested_by
            String,                        // format
            String,                        // status
            String,                        // idempotency_key
            Option<String>,                // payload_hash
            Option<String>,                // object_storage_key
            Option<String>,                // checksum
            Option<String>,                // failure_reason
            Option<chrono::DateTime<Utc>>, // expires_at
            chrono::DateTime<Utc>,         // created_at
            chrono::DateTime<Utc>,         // updated_at
        ),
    >(
        r#"
        SELECT id, activity_id, route_version_id, requested_by, format, status,
               idempotency_key, payload_hash, object_storage_key, checksum,
               failure_reason, expires_at, created_at, updated_at
        FROM exports.export_jobs
        WHERE id = $1
        "#,
    )
    .bind(export_job_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("failed to query export job: {e}"))?
    .ok_or_else(|| format!("export job {} not found", export_job_id))?;

    let status = match row.5.as_str() {
        "queued" => ExportStatus::Queued,
        "generating" => ExportStatus::Generating,
        "ready" => ExportStatus::Ready,
        "failed" => ExportStatus::Failed,
        "expired" => ExportStatus::Expired,
        other => return Err(format!("unknown export job status: {other}").into()),
    };

    let format = match row.4.as_str() {
        "gpx" => ExportFormat::Gpx,
        other => return Err(format!("unknown export format: {other}").into()),
    };

    Ok(ExportJob {
        id: ExportJobId::new(row.0),
        activity_id: haiker_app::activity_catalog::ActivityId::new(row.1),
        route_version_id: haiker_app::route_versioning::RouteVersionId::new(row.2),
        requested_by: haiker_app::identity::UserId::new(row.3),
        format,
        status,
        idempotency_key: row.6,
        payload_hash: row.7,
        object_storage_key: row.8,
        checksum: row.9,
        failure_reason: row.10,
        expires_at: row.11,
        created_at: row.12,
        updated_at: row.13,
    })
}

/// Persist an export job update to the database.
async fn persist_export_job(
    pool: &PgPool,
    job: &ExportJob,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let status_str = job.status.to_string();
    let format_str = job.format.to_string();

    sqlx::query(
        r#"
        UPDATE exports.export_jobs
        SET status = $2, format = $3, object_storage_key = $4,
            checksum = $5, failure_reason = $6, expires_at = $7, updated_at = $8
        WHERE id = $1
        "#,
    )
    .bind(job.id.0)
    .bind(&status_str)
    .bind(&format_str)
    .bind(&job.object_storage_key)
    .bind(&job.checksum)
    .bind(&job.failure_reason)
    .bind(job.expires_at)
    .bind(job.updated_at)
    .execute(pool)
    .await
    .map_err(|e| format!("failed to update export job: {e}"))?;

    Ok(())
}

/// Load route version geometry_json from the database.
async fn load_geometry(
    pool: &PgPool,
    route_version_id: Uuid,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_as::<_, (serde_json::Value,)>(
        "SELECT geometry_json FROM route_versioning.route_versions WHERE id = $1",
    )
    .bind(route_version_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("failed to query route version geometry: {e}"))?
    .ok_or_else(|| format!("route version {} not found", route_version_id))?;

    Ok(row.0)
}

/// Optionally load the activity title from the database.
async fn load_activity_name(
    pool: &PgPool,
    activity_id: Uuid,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT title FROM activity_catalog.activities WHERE id = $1",
    )
    .bind(activity_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("failed to query activity title: {e}"))?;

    Ok(row.and_then(|r| r.0))
}

#[async_trait]
impl JobHandler for GenerateExportJobHandler {
    fn job_type(&self) -> &str {
        GENERATE_EXPORT_JOB_TYPE
    }

    async fn handle(&self, job: &Job) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start = std::time::Instant::now();

        let result = self.process(job).await;

        let duration_ms = start.elapsed().as_millis() as u64;
        metrics::record_job_processed(GENERATE_EXPORT_JOB_TYPE, duration_ms, result.is_ok());

        result
    }
}

impl GenerateExportJobHandler {
    /// Internal processing logic, separated to enable clean error handling
    /// and metrics recording.
    async fn process(&self, job: &Job) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // (a) Deserialize the job payload
        let payload: GenerateExportJob = serde_json::from_value(job.payload.clone())
            .map_err(|e| format!("failed to deserialize GenerateExportJob payload: {e}"))?;

        tracing::info!(
            export_job_id = %payload.export_job_id,
            activity_id = %payload.activity_id,
            route_version_id = %payload.route_version_id,
            owner_id = %payload.owner_id,
            format = %payload.format,
            "Processing export generation"
        );

        let key = storage_key(payload.owner_id, payload.export_job_id);

        // (i) Idempotency check: if the object already exists, mark complete and return
        let already_exists = self
            .object_storage
            .exists(&key)
            .await
            .map_err(|e| format!("failed to check object existence: {e}"))?;

        if already_exists {
            tracing::info!(
                export_job_id = %payload.export_job_id,
                "Export file already exists in storage, marking complete"
            );
            // The file was already generated (retry scenario). Ensure the export
            // job record is in the Ready state.
            let mut export_job = load_export_job(&self.pool, payload.export_job_id).await?;
            if export_job.status != ExportStatus::Ready {
                // We do not have the checksum from storage, so download and compute it
                let existing_bytes =
                    self.object_storage.download(&key).await.map_err(|e| {
                        format!("failed to download existing export for checksum: {e}")
                    })?;
                let checksum = sha256_hex(&existing_bytes);
                let expires_at = Utc::now() + Duration::hours(24);

                // Transition through states as needed
                if export_job.status == ExportStatus::Queued {
                    export_job
                        .start_generating()
                        .map_err(|e| format!("failed to transition to generating: {e}"))?;
                }
                if export_job.status == ExportStatus::Generating {
                    export_job
                        .complete(key.clone(), checksum, expires_at)
                        .map_err(|e| format!("failed to complete export job: {e}"))?;
                }
                persist_export_job(&self.pool, &export_job).await?;
            }
            return Ok(());
        }

        // (e) Load export job and transition to Generating
        let mut export_job = load_export_job(&self.pool, payload.export_job_id).await?;

        // Handle re-delivery gracefully
        match export_job.status {
            ExportStatus::Ready => {
                tracing::info!(
                    export_job_id = %payload.export_job_id,
                    "Export job already in Ready state, skipping"
                );
                return Ok(());
            }
            ExportStatus::Failed | ExportStatus::Expired => {
                tracing::warn!(
                    export_job_id = %payload.export_job_id,
                    status = %export_job.status,
                    "Export job in terminal state, cannot process"
                );
                return Err(format!(
                    "export job {} is in terminal state '{}'",
                    payload.export_job_id, export_job.status
                )
                .into());
            }
            ExportStatus::Queued => {
                export_job
                    .start_generating()
                    .map_err(|e| format!("failed to transition to generating: {e}"))?;
                persist_export_job(&self.pool, &export_job).await?;
            }
            ExportStatus::Generating => {
                // Already generating (re-delivery), continue processing
                tracing::info!(
                    export_job_id = %payload.export_job_id,
                    "Export job already in Generating state, continuing"
                );
            }
        }

        // (b) Load route version geometry
        let geometry_json = match load_geometry(&self.pool, payload.route_version_id).await {
            Ok(g) => g,
            Err(e) => {
                let reason = format!("failed to load geometry: {e}");
                self.mark_failed(payload.export_job_id, &reason).await;
                return Err(reason.into());
            }
        };

        // (c) Parse geometry into GpxPoints
        let segments = match parse_geometry_json(&geometry_json) {
            Ok(s) => s,
            Err(e) => {
                let reason = format!("failed to parse geometry_json: {e}");
                self.mark_failed(payload.export_job_id, &reason).await;
                return Err(reason.into());
            }
        };

        // (d) Optionally load activity name
        let activity_name = match load_activity_name(&self.pool, payload.activity_id).await {
            Ok(name) => name,
            Err(e) => {
                tracing::warn!(
                    export_job_id = %payload.export_job_id,
                    activity_id = %payload.activity_id,
                    error = %e,
                    "Failed to load activity name, proceeding without it"
                );
                None
            }
        };

        // (d.1) Input size guard: prevent OOM from pathologically large geometry
        let total_points: usize = segments.iter().map(|seg| seg.len()).sum();
        const MAX_POINTS: usize = 500_000;
        if total_points > MAX_POINTS {
            let reason = format!(
                "geometry exceeds maximum point count: {} points (limit: {})",
                total_points, MAX_POINTS
            );
            self.mark_failed(payload.export_job_id, &reason).await;
            return Err(reason.into());
        }

        // (f) Generate GPX on a blocking task (CPU-bound XML serialization)
        let input = GpxGeneratorInput {
            activity_name,
            segments,
        };

        // Apply timeout from the job's configured timeout_seconds (default 300s if 0)
        let timeout_secs = if job.timeout_seconds > 0 {
            job.timeout_seconds as u64
        } else {
            300
        };
        let timeout_duration = std::time::Duration::from_secs(timeout_secs);

        let gpx_bytes = match tokio::time::timeout(
            timeout_duration,
            tokio::task::spawn_blocking(move || generate_gpx(&input)),
        )
        .await
        {
            Ok(Ok(Ok(bytes))) => bytes,
            Ok(Ok(Err(gen_err))) => {
                let reason = format!("GPX generation failed: {gen_err}");
                self.mark_failed(payload.export_job_id, &reason).await;
                return Err(reason.into());
            }
            Ok(Err(join_err)) => {
                let reason = format!("GPX generation task panicked: {join_err}");
                self.mark_failed(payload.export_job_id, &reason).await;
                return Err(reason.into());
            }
            Err(_elapsed) => {
                let reason = format!("GPX generation timed out after {}s", timeout_secs);
                self.mark_failed(payload.export_job_id, &reason).await;
                return Err(reason.into());
            }
        };

        // (g) Compute SHA-256 checksum
        let checksum = sha256_hex(&gpx_bytes);

        // (j) Upload to object storage
        if let Err(e) = self
            .object_storage
            .upload(&key, gpx_bytes, Some("application/gpx+xml"))
            .await
        {
            let reason = format!("failed to upload export to storage: {e}");
            self.mark_failed(payload.export_job_id, &reason).await;
            return Err(reason.into());
        }

        // (j.1) Post-upload verification: re-download and verify checksum
        let verified_bytes = match self.object_storage.download(&key).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let reason = format!("failed to download export for verification: {e}");
                self.mark_failed(payload.export_job_id, &reason).await;
                return Err(reason.into());
            }
        };
        let verified_checksum = sha256_hex(&verified_bytes);

        if checksum != verified_checksum {
            let reason = format!(
                "post-upload verification failed: checksum mismatch (expected {}, got {})",
                checksum, verified_checksum
            );
            self.mark_failed(payload.export_job_id, &reason).await;
            return Err(reason.into());
        }

        // (k) Compute expiration (24 hours from now)
        let expires_at = Utc::now() + Duration::hours(24);

        // (l) Complete the export job with verified checksum
        // Re-load the export job in case it was updated concurrently
        let mut export_job = load_export_job(&self.pool, payload.export_job_id).await?;
        if export_job.status == ExportStatus::Generating {
            export_job
                .complete_with_verified_checksum(key, checksum, verified_checksum, expires_at)
                .map_err(|e| format!("failed to complete export job: {e}"))?;
            persist_export_job(&self.pool, &export_job).await?;
        }

        tracing::info!(
            export_job_id = %payload.export_job_id,
            "Export generation completed successfully"
        );

        Ok(())
    }

    /// Attempt to mark an export job as failed. Logs a warning if this
    /// secondary persistence also fails (best-effort).
    async fn mark_failed(&self, export_job_id: Uuid, reason: &str) {
        match load_export_job(&self.pool, export_job_id).await {
            Ok(mut export_job) => {
                if !export_job.status.is_terminal() {
                    if let Err(e) = export_job.fail(reason.to_string()) {
                        tracing::warn!(
                            export_job_id = %export_job_id,
                            error = %e,
                            "Failed to transition export job to Failed state"
                        );
                        return;
                    }
                    if let Err(e) = persist_export_job(&self.pool, &export_job).await {
                        tracing::warn!(
                            export_job_id = %export_job_id,
                            error = %e,
                            "Failed to persist export job failure"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    export_job_id = %export_job_id,
                    error = %e,
                    "Failed to load export job for failure marking"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_type_returns_correct_value() {
        // We cannot construct a real handler without a PgPool and ObjectStorageClient,
        // but we can verify the constant is correct.
        assert_eq!(GENERATE_EXPORT_JOB_TYPE, "generate_export");
    }

    #[test]
    fn storage_key_format_is_correct() {
        let owner_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("valid uuid");
        let export_job_id =
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("valid uuid");

        let key = storage_key(owner_id, export_job_id);
        assert_eq!(
            key,
            "exports/11111111-1111-1111-1111-111111111111/22222222-2222-2222-2222-222222222222.gpx"
        );
    }

    #[test]
    fn sha256_hex_computes_correct_digest() {
        let data = b"hello world";
        let digest = sha256_hex(data);
        // Known SHA-256 of "hello world"
        assert_eq!(
            digest,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn payload_deserialization_succeeds() {
        let json = serde_json::json!({
            "export_job_id": "11111111-1111-1111-1111-111111111111",
            "activity_id": "22222222-2222-2222-2222-222222222222",
            "route_version_id": "33333333-3333-3333-3333-333333333333",
            "owner_id": "44444444-4444-4444-4444-444444444444",
            "format": "gpx",
            "correlation_id": "55555555-5555-5555-5555-555555555555"
        });

        let payload: GenerateExportJob =
            serde_json::from_value(json).expect("deserialization should succeed");

        assert_eq!(
            payload.export_job_id,
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("valid")
        );
        assert_eq!(payload.format, "gpx");
    }

    #[test]
    fn payload_deserialization_fails_for_invalid_json() {
        let json = serde_json::json!({
            "export_job_id": "not-a-uuid",
            "activity_id": "22222222-2222-2222-2222-222222222222",
            "route_version_id": "33333333-3333-3333-3333-333333333333",
            "owner_id": "44444444-4444-4444-4444-444444444444",
            "format": "gpx",
            "correlation_id": "55555555-5555-5555-5555-555555555555"
        });

        let result = serde_json::from_value::<GenerateExportJob>(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_geometry_json_segmented_format() {
        let json = serde_json::json!([
            [
                {"latitude": 47.0, "longitude": 11.0, "elevation": 500.0},
                {"latitude": 47.1, "longitude": 11.1, "elevation": null}
            ],
            [
                {"latitude": 48.0, "longitude": 12.0}
            ]
        ]);

        let segments = parse_geometry_json(&json).expect("parsing should succeed");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].len(), 2);
        assert_eq!(segments[1].len(), 1);

        assert_eq!(segments[0][0].latitude, 47.0);
        assert_eq!(segments[0][0].longitude, 11.0);
        assert_eq!(segments[0][0].elevation, Some(500.0));

        assert_eq!(segments[0][1].elevation, None);

        assert_eq!(segments[1][0].latitude, 48.0);
        assert_eq!(segments[1][0].elevation, None);
    }

    #[test]
    fn parse_geometry_json_flat_format() {
        let json = serde_json::json!([
            {"latitude": 47.0, "longitude": 11.0},
            {"latitude": 47.1, "longitude": 11.1, "elevation": 600.0}
        ]);

        let segments = parse_geometry_json(&json).expect("parsing should succeed");
        assert_eq!(segments.len(), 1, "flat format wraps in a single segment");
        assert_eq!(segments[0].len(), 2);

        assert_eq!(segments[0][0].latitude, 47.0);
        assert_eq!(segments[0][0].elevation, None);
        assert_eq!(segments[0][1].elevation, Some(600.0));
    }

    #[test]
    fn parse_geometry_json_invalid_format() {
        let json = serde_json::json!("not an array");
        let result = parse_geometry_json(&json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_geometry_json_empty_segments() {
        let json = serde_json::json!([[], []]);

        let segments = parse_geometry_json(&json).expect("parsing should succeed");
        assert_eq!(segments.len(), 2);
        assert!(segments[0].is_empty());
        assert!(segments[1].is_empty());
    }
}
