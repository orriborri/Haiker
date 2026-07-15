//! Worker job handler for GPX import processing.
//!
//! Implements JobHandler for the 'parse_gpx' job type, connecting the platform
//! infrastructure to the domain orchestrator.

use async_trait::async_trait;
use sqlx::PgPool;

use haiker_app::identity::UserId;
use haiker_app::imports::job_types::{ParseGpxJob, PARSE_GPX_JOB_TYPE};
use haiker_app::imports::orchestrator::{ImportOrchestrator, ImportProcessingResult};
use haiker_app::imports::repository::ImportRepository;
use haiker_app::imports::ImportId;

use crate::audit::AuditLog;
use crate::import_commit::PgImportCommitter;
use crate::import_persistence::PgImportRepository;
use crate::job_queue::{Job, JobHandler};
use crate::metrics;
use crate::object_storage::ObjectStorageClient;

/// Adapter implementing the domain ObjectStore trait for the platform ObjectStorageClient.
struct ObjectStoreAdapter {
    client: ObjectStorageClient,
}

#[async_trait]
impl haiker_app::imports::orchestrator::ObjectStore for ObjectStoreAdapter {
    async fn download(&self, key: &str) -> Result<Vec<u8>, haiker_app::imports::ImportError> {
        self.client.download(key).await.map_err(|e| {
            haiker_app::imports::ImportError::StorageError {
                message: e.to_string(),
            }
        })
    }
}

/// Adapter implementing the CheckDuplicate trait using the import repository.
struct DuplicateCheckerAdapter {
    repo: PgImportRepository,
}

#[async_trait]
impl haiker_app::imports::duplicate_detection::CheckDuplicate for DuplicateCheckerAdapter {
    async fn check(
        &self,
        owner_id: UserId,
        checksum: &haiker_app::imports::checksum::Checksum,
    ) -> Result<
        haiker_app::imports::duplicate_detection::DuplicateCheckResult,
        haiker_app::imports::ImportError,
    > {
        use haiker_app::imports::duplicate_detection::DuplicateCheckResult;
        use haiker_app::imports::repository::ImportRepository;

        match self
            .repo
            .find_completed_by_checksum(owner_id, checksum)
            .await?
        {
            Some((existing_import_id, existing_activity_id)) => {
                Ok(DuplicateCheckResult::ExactDuplicate {
                    existing_import_id,
                    existing_activity_id,
                })
            }
            None => Ok(DuplicateCheckResult::NotDuplicate),
        }
    }
}

/// Job handler for processing GPX import files.
///
/// Deserializes the job payload, constructs the orchestrator with real
/// infrastructure implementations, and delegates to the domain orchestrator.
pub struct ParseGpxJobHandler {
    pool: PgPool,
    object_storage: ObjectStorageClient,
    audit_log: AuditLog,
}

impl ParseGpxJobHandler {
    /// Create a new ParseGpxJobHandler.
    pub fn new(pool: PgPool, object_storage: ObjectStorageClient, audit_log: AuditLog) -> Self {
        Self {
            pool,
            object_storage,
            audit_log,
        }
    }
}

#[async_trait]
impl JobHandler for ParseGpxJobHandler {
    fn job_type(&self) -> &str {
        PARSE_GPX_JOB_TYPE
    }

    async fn handle(&self, job: &Job) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Deserialize the job payload
        let payload: ParseGpxJob = serde_json::from_value(job.payload.clone())
            .map_err(|e| format!("failed to deserialize ParseGpxJob payload: {e}"))?;

        tracing::info!(
            import_id = %payload.import_id,
            owner_id = %payload.owner_id,
            "Processing GPX import"
        );

        // Construct infrastructure adapters
        let repo = PgImportRepository::new(self.pool.clone());
        let object_store = ObjectStoreAdapter {
            client: self.object_storage.clone(),
        };
        let duplicate_checker = DuplicateCheckerAdapter {
            repo: PgImportRepository::new(self.pool.clone()),
        };
        let committer = PgImportCommitter::new(self.pool.clone());

        // Build the orchestrator with real implementations
        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        // Run the import processing pipeline
        let import_id = ImportId::new(payload.import_id);
        let owner_id = UserId::new(payload.owner_id);

        // Transition from Uploaded -> Validating -> Queued before the orchestrator
        // picks up the import. The orchestrator expects the import to be in Queued state.
        // Handle re-delivery gracefully: if the import is already in Validating, Queued,
        // or Parsing state from a previous partial run, skip the transitions that are
        // already completed rather than erroring.
        {
            let mut import = repo
                .find_by_id(import_id)
                .await
                .map_err(|e| format!("failed to load import: {e}"))?
                .ok_or_else(|| format!("import {} not found", import_id))?;

            match import.status {
                haiker_app::imports::state_machine::ImportStatus::Uploaded => {
                    import
                        .start_validation()
                        .map_err(|e| format!("failed to transition to validating: {e}"))?;
                    import
                        .queue_for_parsing()
                        .map_err(|e| format!("failed to transition to queued: {e}"))?;
                    repo.update(&import)
                        .await
                        .map_err(|e| format!("failed to persist queued status: {e}"))?;
                }
                haiker_app::imports::state_machine::ImportStatus::Validating => {
                    import
                        .queue_for_parsing()
                        .map_err(|e| format!("failed to transition to queued: {e}"))?;
                    repo.update(&import)
                        .await
                        .map_err(|e| format!("failed to persist queued status: {e}"))?;
                }
                haiker_app::imports::state_machine::ImportStatus::Queued
                | haiker_app::imports::state_machine::ImportStatus::Parsing => {
                    // Already past the pre-orchestrator transitions; nothing to do.
                    // The orchestrator will handle these states appropriately.
                }
                other => {
                    return Err(format!(
                        "import {} is in unexpected state '{}' for processing",
                        import_id, other
                    )
                    .into());
                }
            }
        }

        match orchestrator
            .process_import(
                import_id,
                owner_id,
                &payload.object_storage_key,
                payload.correlation_id,
            )
            .await
        {
            Ok(result) => {
                // Emit success attempt metric
                metrics::record_import_attempt(PARSE_GPX_JOB_TYPE, job.retry_count + 1, true);

                // Emit file metrics for completed imports
                if let ImportProcessingResult::Completed {
                    file_size_bytes,
                    point_count,
                    ..
                } = &result
                {
                    metrics::record_import_file_metrics(*file_size_bytes, *point_count);
                }

                // Write audit event for duplicate detections.
                // This is an infrastructure concern handled here in the worker
                // because the domain orchestrator must remain free of infra deps.
                if let ImportProcessingResult::Duplicate {
                    existing_import_id,
                    existing_activity_id,
                } = &result
                {
                    let metadata = serde_json::json!({
                        "existing_import_id": existing_import_id.0.to_string(),
                        "existing_activity_id": existing_activity_id.map(|a| a.0.to_string()),
                        "owner_id": owner_id.0.to_string(),
                    });

                    if let Err(e) = self
                        .audit_log
                        .append(
                            owner_id.0,
                            "import.duplicate_detected",
                            "import",
                            &import_id.0.to_string(),
                            Some(metadata),
                        )
                        .await
                    {
                        tracing::error!(
                            import_id = %payload.import_id,
                            error = %e,
                            "Failed to write audit event for duplicate detection"
                        );
                    }
                }

                tracing::info!(
                    import_id = %payload.import_id,
                    ?result,
                    "GPX import processing completed successfully"
                );
                Ok(())
            }
            Err(e) => {
                // Emit failure attempt metric
                metrics::record_import_attempt(PARSE_GPX_JOB_TYPE, job.retry_count + 1, false);

                // Emit failure code metric if available
                // Try to reload the import to get the failure code set by the orchestrator
                if let Ok(Some(failed_import)) = repo.find_by_id(import_id).await {
                    if let Some(failure_code) = &failed_import.failure_code {
                        metrics::record_import_failure(failure_code.as_str());
                    }
                }

                tracing::warn!(
                    import_id = %payload.import_id,
                    error = %e,
                    "GPX import processing failed"
                );
                Err(Box::new(e))
            }
        }
    }
}
