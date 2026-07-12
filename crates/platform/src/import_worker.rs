//! Worker job handler for GPX import processing.
//!
//! Implements JobHandler for the 'parse_gpx' job type, connecting the platform
//! infrastructure to the domain orchestrator.

use async_trait::async_trait;
use sqlx::PgPool;

use haiker_app::identity::UserId;
use haiker_app::imports::job_types::{ParseGpxJob, PARSE_GPX_JOB_TYPE};
use haiker_app::imports::orchestrator::ImportOrchestrator;
use haiker_app::imports::ImportId;

use crate::import_commit::PgImportCommitter;
use crate::import_persistence::PgImportRepository;
use crate::job_queue::{Job, JobHandler};
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

        match self.repo.find_by_checksum(owner_id, checksum).await? {
            Some(existing) => Ok(DuplicateCheckResult::ExactDuplicate {
                existing_import_id: existing.id,
                existing_activity_id: None, // Activity ID lookup would require additional query
            }),
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
}

impl ParseGpxJobHandler {
    /// Create a new ParseGpxJobHandler.
    pub fn new(pool: PgPool, object_storage: ObjectStorageClient) -> Self {
        Self {
            pool,
            object_storage,
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
                tracing::info!(
                    import_id = %payload.import_id,
                    ?result,
                    "GPX import processing completed successfully"
                );
                Ok(())
            }
            Err(e) => {
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
