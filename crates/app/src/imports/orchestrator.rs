//! Import orchestrator: coordinates the full async import pipeline.
//!
//! The orchestrator chains: download file -> verify checksum -> parse GPX ->
//! check duplicates -> normalize track -> commit activity.
//!
//! All external dependencies are abstracted behind traits, keeping this
//! module free of infrastructure concerns and fully testable with mocks.

use async_trait::async_trait;
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::activity_catalog::{ActivityId, ActivityType};
use crate::identity::UserId;
use crate::recorded_activity::normalization::normalize_gpx;
use crate::recorded_activity::{RecordedTrackId, SourceArtifactId, SourceRevisionId};
use crate::route_versioning::{CorrectedStatistics, RouteVersion};

use super::checksum::Checksum;
use super::commit::{CommitImport, ImportCommitData};
use super::duplicate_detection::{CheckDuplicate, DuplicateCheckResult};
use super::failure_code::FailureCode;
use super::gpx_parser::parse_gpx;
use super::repository::ImportRepository;
use super::state_machine::ImportStatus;
use super::{ImportError, ImportId};

/// Parser version string for audit and versioning.
const PARSER_VERSION: &str = "1.0.0";

/// Trait for downloading files from object storage.
///
/// This is the domain-layer abstraction that the orchestrator depends on.
/// The platform layer provides the real implementation.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Download a file by its storage key, returning the raw bytes.
    async fn download(&self, key: &str) -> Result<Vec<u8>, ImportError>;
}

/// Result of processing an import.
#[derive(Debug, Clone)]
pub enum ImportProcessingResult {
    /// Import completed successfully with a new activity.
    Completed {
        activity_id: ActivityId,
        file_size_bytes: u64,
        point_count: u64,
    },
    /// Import completed as a duplicate of an existing activity.
    Duplicate {
        existing_import_id: ImportId,
        existing_activity_id: Option<ActivityId>,
    },
}

/// The import orchestrator coordinates the full import processing pipeline.
///
/// It uses trait-based dependencies to remain infrastructure-agnostic and testable.
pub struct ImportOrchestrator<'a> {
    pub repo: &'a dyn ImportRepository,
    pub object_store: &'a dyn ObjectStore,
    pub duplicate_checker: &'a dyn CheckDuplicate,
    pub committer: &'a dyn CommitImport,
}

impl<'a> ImportOrchestrator<'a> {
    /// Process an import through the full pipeline.
    ///
    /// Steps:
    /// 1. Load the import, verify status is Queued, transition to Parsing
    /// 2. Download the GPX file from object storage
    /// 3. Compute SHA-256 checksum and validate against stored checksum
    /// 4. Parse the GPX file
    /// 5. Check for duplicate via checksum match
    /// 6. If duplicate, transition to Completed with duplicate reference
    /// 7. Normalize the parsed track data
    /// 8. Transition to Committing
    /// 9. Call the transactional commit
    pub async fn process_import(
        &self,
        import_id: ImportId,
        owner_id: UserId,
        object_storage_key: &str,
        correlation_id: Uuid,
    ) -> Result<ImportProcessingResult, ImportError> {
        // 1. Load import and verify status
        let mut import = self
            .repo
            .find_by_id(import_id)
            .await?
            .ok_or(ImportError::NotFound)?;

        if import.status != ImportStatus::Queued {
            return Err(ImportError::InvalidTransition {
                from: import.status.to_string(),
                to: "parsing".to_string(),
            });
        }

        // Capture the time spent in Queued state before transitioning
        let queued_duration_ms = {
            let now = Utc::now();
            let duration = now.signed_duration_since(import.updated_at);
            duration.num_milliseconds().max(0) as u64
        };

        // Transition to Parsing
        import.start_parsing()?;
        self.repo.update(&import).await?;

        // Emit state duration metric for time spent in Queued state
        tracing::info!(
            target: "metrics",
            metric = "import_state_transition",
            import_status = "queued",
            duration_in_state_ms = queued_duration_ms,
            "import state transition"
        );

        // 2. Download the GPX file
        let file_bytes = match self.object_store.download(object_storage_key).await {
            Ok(bytes) => bytes,
            Err(e) => {
                import.fail_with_code(
                    format!("failed to download file: {e}"),
                    FailureCode::StorageUnavailable,
                )?;
                self.repo.update(&import).await?;
                return Err(e);
            }
        };

        let file_size_bytes = file_bytes.len() as u64;

        // 3. Compute and verify checksum
        let computed_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&file_bytes);
            format!("{:x}", hasher.finalize())
        };

        let stored_checksum =
            import
                .checksum
                .as_ref()
                .ok_or_else(|| ImportError::ValidationFailed {
                    message: "import has no stored checksum".to_string(),
                })?;

        if computed_hash != stored_checksum.as_str() {
            let err = ImportError::ChecksumMismatch {
                expected: stored_checksum.as_str().to_string(),
                actual: computed_hash.clone(),
            };
            import.fail_with_code(
                format!(
                    "checksum mismatch: expected {}, got {}",
                    stored_checksum.as_str(),
                    computed_hash
                ),
                FailureCode::ChecksumMismatch,
            )?;
            self.repo.update(&import).await?;
            return Err(err);
        }

        // 4. Parse the GPX file
        let parse_result = match parse_gpx(&file_bytes) {
            Ok(result) => result,
            Err(parse_err) => {
                let reason = format!("parsing failed: {} ({})", parse_err.message, parse_err.code);
                import.fail_with_code(reason.clone(), FailureCode::ParseError)?;
                self.repo.update(&import).await?;
                return Err(ImportError::ParsingFailed { reason });
            }
        };

        // 5. Check for duplicates
        let checksum = Checksum::new(&computed_hash)?;
        let dup_result = self.duplicate_checker.check(owner_id, &checksum).await?;

        if let DuplicateCheckResult::ExactDuplicate {
            existing_import_id,
            existing_activity_id,
        } = dup_result
        {
            // 6. Duplicate found - complete with reference to the existing activity.
            //
            // Concurrency note: a race exists where two workers process the same file
            // simultaneously, both call find_completed_by_checksum before either commits,
            // and both get None (no duplicate). In that scenario, one will succeed and the
            // other will hit the partial unique index (idx_imports_owner_checksum_completed)
            // on the UPDATE to 'completed' status, surfacing as a StorageError that fails
            // the second import. This is fail-closed by design: no duplicate activity is
            // created. A full integration test for this path requires a live database.
            import.activity_id = existing_activity_id;
            import.start_committing()?;
            import.complete()?;
            self.repo.update(&import).await?;
            return Ok(ImportProcessingResult::Duplicate {
                existing_import_id,
                existing_activity_id,
            });
        }

        // 7. Normalize the parsed track data
        let normalized = match normalize_gpx(&parse_result) {
            Ok(n) => n,
            Err(e) => {
                let reason = format!("normalization failed: {e}");
                import.fail_with_code(reason.clone(), FailureCode::ParseError)?;
                self.repo.update(&import).await?;
                return Err(ImportError::ParsingFailed { reason });
            }
        };

        let point_count = normalized.recorded_track.statistics.point_count as u64;

        // Capture time spent in Parsing state before transitioning
        let parsing_duration_ms = {
            let now = Utc::now();
            let duration = now.signed_duration_since(import.updated_at);
            duration.num_milliseconds().max(0) as u64
        };

        // 8. Transition to Committing
        import.start_committing()?;
        self.repo.update(&import).await?;

        // Emit state duration metric for time spent in Parsing state
        tracing::info!(
            target: "metrics",
            metric = "import_state_transition",
            import_status = "parsing",
            duration_in_state_ms = parsing_duration_ms,
            "import state transition"
        );

        // 9. Build commit data and commit
        let activity_id = ActivityId::generate();
        let source_artifact_id = import
            .source_artifact_id
            .unwrap_or_else(SourceArtifactId::generate);
        let source_revision_id = SourceRevisionId::generate();
        let recorded_track_id = RecordedTrackId::generate();

        let activity_title = normalized
            .suggested_title
            .unwrap_or_else(|| "Imported Activity".to_string());

        // Construct the initial route version via the domain factory to enforce
        // the geometry invariant (minimum 2 points).
        // Compute corrected statistics from the initial geometry (which is the
        // same as the recorded geometry before any edits are applied).
        let corrected_statistics = CorrectedStatistics::calculate_from_geometry(
            &normalized.preview_geometry,
        );

        let route_version = RouteVersion::new_initial(
            activity_id,
            normalized.preview_geometry.clone(),
            normalized.recorded_track.bounding_box,
            corrected_statistics.clone(),
            PARSER_VERSION.to_string(),
            owner_id,
        )
        .map_err(|e| ImportError::ValidationFailed {
            message: format!("route version validation failed: {e}"),
        })?;

        let commit_data = ImportCommitData {
            owner_id,
            import_id,
            correlation_id,
            source_artifact_id,
            object_storage_key: object_storage_key.to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: file_bytes.len() as i64,
            checksum: computed_hash,
            source_revision_id,
            revision_number: 1,
            parser_version: PARSER_VERSION.to_string(),
            recorded_track_id,
            segments: normalized.recorded_track.segments,
            bounding_box: normalized.recorded_track.bounding_box,
            statistics: normalized.recorded_track.statistics,
            preview_geometry: normalized.preview_geometry,
            activity_id,
            activity_title,
            activity_type: ActivityType::Hike,
            started_at: normalized.started_at,
            ended_at: normalized.ended_at,
            route_version_id: route_version.id,
            corrected_statistics,
        };

        // Capture time spent in Committing state (before commit completes)
        let committing_start = Utc::now();

        match self.committer.commit(&commit_data).await {
            Ok(_) => {
                let committing_duration_ms = {
                    let now = Utc::now();
                    let duration = now.signed_duration_since(committing_start);
                    duration.num_milliseconds().max(0) as u64
                };
                // Emit state duration metric for time spent in Committing state
                tracing::info!(
                    target: "metrics",
                    metric = "import_state_transition",
                    import_status = "committing",
                    duration_in_state_ms = committing_duration_ms,
                    "import state transition"
                );
                Ok(ImportProcessingResult::Completed {
                    activity_id,
                    file_size_bytes,
                    point_count,
                })
            }
            Err(e) => {
                // Try to transition import to failed
                let _ = import
                    .fail_with_code(format!("commit failed: {e}"), FailureCode::InternalError);
                let _ = self.repo.update(&import).await;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::checksum::Checksum;
    use crate::imports::duplicate_detection::DuplicateCheckResult;
    use crate::imports::{Import, ImportFormat};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // -- Mock implementations --

    struct MockObjectStore {
        files: HashMap<String, Vec<u8>>,
    }

    #[async_trait]
    impl ObjectStore for MockObjectStore {
        async fn download(&self, key: &str) -> Result<Vec<u8>, ImportError> {
            self.files
                .get(key)
                .cloned()
                .ok_or(ImportError::StorageError {
                    message: format!("object not found: {key}"),
                })
        }
    }

    struct MockRepo {
        imports: Mutex<HashMap<ImportId, Import>>,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                imports: Mutex::new(HashMap::new()),
            }
        }

        fn insert(&self, import: Import) {
            self.imports.lock().unwrap().insert(import.id, import);
        }
    }

    #[async_trait]
    impl ImportRepository for MockRepo {
        async fn save(&self, import: &Import) -> Result<(), ImportError> {
            self.imports
                .lock()
                .unwrap()
                .insert(import.id, import.clone());
            Ok(())
        }

        async fn find_by_id(&self, id: ImportId) -> Result<Option<Import>, ImportError> {
            Ok(self.imports.lock().unwrap().get(&id).cloned())
        }

        async fn find_by_idempotency_key(
            &self,
            _owner_id: UserId,
            _key: &str,
        ) -> Result<Option<Import>, ImportError> {
            Ok(None)
        }

        async fn find_completed_by_checksum(
            &self,
            _owner_id: UserId,
            _checksum: &Checksum,
        ) -> Result<Option<(ImportId, Option<ActivityId>)>, ImportError> {
            Ok(None)
        }

        async fn update(&self, import: &Import) -> Result<(), ImportError> {
            self.imports
                .lock()
                .unwrap()
                .insert(import.id, import.clone());
            Ok(())
        }

        async fn find_abandoned(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<Vec<Import>, ImportError> {
            Ok(vec![])
        }
    }

    struct MockDuplicateChecker {
        result: DuplicateCheckResult,
    }

    #[async_trait]
    impl CheckDuplicate for MockDuplicateChecker {
        async fn check(
            &self,
            _owner_id: UserId,
            _checksum: &Checksum,
        ) -> Result<DuplicateCheckResult, ImportError> {
            Ok(self.result.clone())
        }
    }

    struct MockCommitter;

    #[async_trait]
    impl CommitImport for MockCommitter {
        async fn commit(&self, data: &ImportCommitData) -> Result<ActivityId, ImportError> {
            Ok(data.activity_id)
        }
    }

    struct FailingCommitter;

    #[async_trait]
    impl CommitImport for FailingCommitter {
        async fn commit(&self, _data: &ImportCommitData) -> Result<ActivityId, ImportError> {
            Err(ImportError::StorageError {
                message: "database connection failed".to_string(),
            })
        }
    }

    /// Helper to create a valid GPX file and its corresponding SHA-256 checksum.
    fn valid_gpx_and_checksum() -> (Vec<u8>, String) {
        let gpx = br#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test">
  <trk>
    <name>Test Track</name>
    <trkseg>
      <trkpt lat="47.0" lon="11.0">
        <ele>500.0</ele>
        <time>2024-01-15T08:00:00Z</time>
      </trkpt>
      <trkpt lat="47.001" lon="11.001">
        <ele>520.0</ele>
        <time>2024-01-15T08:05:00Z</time>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;

        let mut hasher = Sha256::new();
        hasher.update(gpx);
        let checksum = format!("{:x}", hasher.finalize());

        (gpx.to_vec(), checksum)
    }

    /// Helper to create an import in Queued state with the given checksum.
    fn queued_import(owner_id: UserId, checksum_str: &str) -> Import {
        let mut import =
            Import::new(owner_id, ImportFormat::Gpx, "key-1".to_string(), None).unwrap();
        import.start_upload().unwrap();
        let artifact_id = SourceArtifactId::generate();
        let checksum = Checksum::new(checksum_str).unwrap();
        import.complete_upload(artifact_id, checksum).unwrap();
        import.start_validation().unwrap();
        import.queue_for_parsing().unwrap();
        import
    }

    #[tokio::test]
    async fn orchestrator_happy_path_completes_successfully() {
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/file.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await
            .unwrap();

        match result {
            ImportProcessingResult::Completed {
                activity_id,
                file_size_bytes,
                point_count,
            } => {
                assert_ne!(activity_id.0, Uuid::nil());
                assert!(file_size_bytes > 0);
                assert!(point_count > 0);
            }
            _ => panic!("Expected Completed result"),
        }

        // Verify import ended in the correct final state flow
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        // The import should be in Committing state (the commit mock succeeds but
        // the orchestrator doesn't update to Completed - that's done by the committer)
        assert_eq!(final_import.status, ImportStatus::Committing);
    }

    #[tokio::test]
    async fn orchestrator_checksum_mismatch_fails_import() {
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, _correct_checksum) = valid_gpx_and_checksum();
        // Use a different checksum so it mismatches
        let wrong_checksum = "b".repeat(64);
        let import = queued_import(owner_id, &wrong_checksum);
        let import_id = import.id;
        let storage_key = "imports/test/file.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImportError::ChecksumMismatch { .. }
        ));

        // Import should be in Failed state
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(final_import.status, ImportStatus::Failed);
        assert!(final_import
            .failure_reason
            .as_ref()
            .unwrap()
            .contains("checksum mismatch"));
        assert_eq!(
            final_import.failure_code,
            Some(super::super::failure_code::FailureCode::ChecksumMismatch)
        );
    }

    #[tokio::test]
    async fn orchestrator_duplicate_returns_existing_reference() {
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/file.gpx";

        let existing_import_id = ImportId::generate();
        let existing_activity_id = ActivityId::generate();

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::ExactDuplicate {
                existing_import_id,
                existing_activity_id: Some(existing_activity_id),
            },
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await
            .unwrap();

        match result {
            ImportProcessingResult::Duplicate {
                existing_import_id: eid,
                existing_activity_id: eaid,
            } => {
                assert_eq!(eid, existing_import_id);
                assert_eq!(eaid, Some(existing_activity_id));
            }
            _ => panic!("Expected Duplicate result"),
        }

        // Import should be Completed (duplicate is still a successful completion)
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(final_import.status, ImportStatus::Completed);
        // Verify that the existing activity_id is persisted on the duplicate import
        assert_eq!(
            final_import.activity_id,
            Some(existing_activity_id),
            "Duplicate import must have activity_id set before persisting"
        );
    }

    #[tokio::test]
    async fn orchestrator_parser_failure_transitions_to_failed() {
        let owner_id = UserId::new(Uuid::new_v4());
        // Invalid GPX content
        let bad_gpx = b"this is not valid xml at all";
        let mut hasher = Sha256::new();
        hasher.update(bad_gpx);
        let checksum = format!("{:x}", hasher.finalize());

        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/bad.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), bad_gpx.to_vec());
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImportError::ParsingFailed { .. }
        ));

        // Import should be in Failed state
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(final_import.status, ImportStatus::Failed);
        assert!(final_import
            .failure_reason
            .as_ref()
            .unwrap()
            .contains("parsing failed"));
        assert_eq!(
            final_import.failure_code,
            Some(super::super::failure_code::FailureCode::ParseError)
        );
    }

    #[tokio::test]
    async fn orchestrator_download_failure_transitions_to_failed() {
        let owner_id = UserId::new(Uuid::new_v4());
        let checksum = "a".repeat(64);
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/missing.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        // Empty file store - no file at the key
        let object_store = MockObjectStore {
            files: HashMap::new(),
        };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImportError::StorageError { .. }
        ));

        // Import should be in Failed state
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(final_import.status, ImportStatus::Failed);
        assert_eq!(
            final_import.failure_code,
            Some(super::super::failure_code::FailureCode::StorageUnavailable)
        );
    }

    #[tokio::test]
    async fn orchestrator_import_not_found_returns_error() {
        let owner_id = UserId::new(Uuid::new_v4());
        let import_id = ImportId::generate();

        let repo = MockRepo::new();
        let object_store = MockObjectStore {
            files: HashMap::new(),
        };
        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, "key", Uuid::new_v4())
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ImportError::NotFound));
    }

    #[tokio::test]
    async fn orchestrator_wrong_status_returns_error() {
        let owner_id = UserId::new(Uuid::new_v4());
        // Create import in Uploading state (not Queued)
        let mut import =
            Import::new(owner_id, ImportFormat::Gpx, "key-2".to_string(), None).unwrap();
        import.start_upload().unwrap();
        let import_id = import.id;

        let repo = MockRepo::new();
        repo.insert(import);

        let object_store = MockObjectStore {
            files: HashMap::new(),
        };
        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, "key", Uuid::new_v4())
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImportError::InvalidTransition { .. }
        ));
    }

    #[tokio::test]
    async fn orchestrator_commit_failure_transitions_to_failed() {
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/file.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = FailingCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());

        // Import should be in Failed state
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(final_import.status, ImportStatus::Failed);
        assert!(final_import
            .failure_reason
            .as_ref()
            .unwrap()
            .contains("commit failed"));
    }

    #[tokio::test]
    async fn orchestrator_duplicate_emits_correct_result_with_activity_id() {
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/file.gpx";

        let existing_import_id = ImportId::generate();
        let existing_activity_id = ActivityId::generate();

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::ExactDuplicate {
                existing_import_id,
                existing_activity_id: Some(existing_activity_id),
            },
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await
            .unwrap();

        // Verify that the duplicate result includes the correct activity_id
        // from the checker (which is backed by the repository in production).
        match result {
            ImportProcessingResult::Duplicate {
                existing_import_id: eid,
                existing_activity_id: eaid,
            } => {
                assert_eq!(eid, existing_import_id);
                assert_eq!(
                    eaid,
                    Some(existing_activity_id),
                    "Duplicate result must carry the existing activity_id from the database"
                );
            }
            _ => panic!("Expected Duplicate result"),
        }
    }

    #[tokio::test]
    async fn orchestrator_duplicate_without_activity_id_returns_none() {
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/file.gpx";

        let existing_import_id = ImportId::generate();

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        // Simulate a completed import that does not yet have an activity_id
        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::ExactDuplicate {
                existing_import_id,
                existing_activity_id: None,
            },
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await
            .unwrap();

        match result {
            ImportProcessingResult::Duplicate {
                existing_import_id: eid,
                existing_activity_id: eaid,
            } => {
                assert_eq!(eid, existing_import_id);
                assert_eq!(
                    eaid, None,
                    "Duplicate result must propagate None when no activity_id exists"
                );
            }
            _ => panic!("Expected Duplicate result"),
        }
    }

    // -- Additional mock implementations for new tests --

    /// An object store that always fails with a storage error.
    /// Used to simulate storage outages.
    struct FailingObjectStore;

    #[async_trait]
    impl ObjectStore for FailingObjectStore {
        async fn download(&self, key: &str) -> Result<Vec<u8>, ImportError> {
            Err(ImportError::StorageError {
                message: format!("storage unavailable for key: {key}"),
            })
        }
    }

    /// A committer that records whether `commit` was called and captures commit data.
    /// Used to verify that the committer is NOT invoked in certain paths (e.g., duplicates)
    /// and to inspect the data passed at the commit boundary.
    struct RecordingCommitter {
        call_count: Mutex<u32>,
        last_commit_data: Mutex<Option<ImportCommitData>>,
    }

    impl RecordingCommitter {
        fn new() -> Self {
            Self {
                call_count: Mutex::new(0),
                last_commit_data: Mutex::new(None),
            }
        }

        fn was_called(&self) -> bool {
            *self.call_count.lock().unwrap() > 0
        }

        fn last_commit_data(&self) -> Option<ImportCommitData> {
            self.last_commit_data.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CommitImport for RecordingCommitter {
        async fn commit(&self, data: &ImportCommitData) -> Result<ActivityId, ImportError> {
            *self.call_count.lock().unwrap() += 1;
            *self.last_commit_data.lock().unwrap() = Some(data.clone());
            Ok(data.activity_id)
        }
    }

    #[tokio::test]
    async fn source_file_bytes_not_mutated_by_processing() {
        // Prove that the bytes flowing through the orchestrator are not
        // altered before reaching the commit boundary. The checksum recorded
        // in ImportCommitData must match the SHA-256 of the original input
        // bytes, confirming no mutation occurred between download and commit.
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();

        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/immutable.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = RecordingCommitter::new();

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_ok(), "Import should succeed");

        // Verify: the checksum recorded at the commit boundary matches the
        // original file's SHA-256. This proves the bytes were not mutated
        // between download and commit (the commit data checksum is computed
        // from the actual downloaded bytes inside process_import).
        let commit_data = committer
            .last_commit_data()
            .expect("Committer must have been called");
        assert_eq!(
            commit_data.checksum, checksum,
            "Checksum in commit data must match the original file's SHA-256, \
             proving no byte mutation occurred during orchestrator processing"
        );
    }

    #[tokio::test]
    async fn orchestrator_redelivery_from_parsing_state_returns_invalid_transition() {
        // Simulates a worker restart scenario: the import is already in
        // Parsing state (not Queued), so a second call to process_import
        // must return InvalidTransition rather than re-processing.
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();

        // Create an import and advance it to Parsing state
        let mut import = queued_import(owner_id, &checksum);
        import.start_parsing().unwrap();
        assert_eq!(import.status, ImportStatus::Parsing);
        let import_id = import.id;
        let storage_key = "imports/test/redelivery.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = MockCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ImportError::InvalidTransition { from, to } => {
                assert_eq!(from, "parsing");
                assert_eq!(to, "parsing");
            }
            other => panic!("Expected InvalidTransition, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn storage_outage_leaves_import_failed_with_storage_unavailable_code() {
        // When object storage is unreachable, the import must transition to
        // Failed with failure_code StorageUnavailable and the committer
        // must never be invoked.
        let owner_id = UserId::new(Uuid::new_v4());
        let checksum = "a".repeat(64);
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/outage.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let object_store = FailingObjectStore;

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = RecordingCommitter::new();

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());

        // Import must be in Failed state with StorageUnavailable code
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(final_import.status, ImportStatus::Failed);
        assert_eq!(
            final_import.failure_code,
            Some(FailureCode::StorageUnavailable),
            "Storage outage must produce StorageUnavailable failure code"
        );

        // Committer must not have been called
        assert!(
            !committer.was_called(),
            "Committer must not be invoked when storage is unavailable"
        );
    }

    #[tokio::test]
    async fn commit_failure_transitions_import_to_failed_not_stuck_in_committing() {
        // Extends the existing commit failure test: verifies that the import
        // transitions to Failed (not stuck in Committing) AND that the
        // failure_code is InternalError.
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/commit-fail.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = FailingCommitter;

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());

        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();

        // Must NOT be stuck in Committing
        assert_ne!(
            final_import.status,
            ImportStatus::Committing,
            "Import must not remain stuck in Committing after commit failure"
        );
        assert_eq!(final_import.status, ImportStatus::Failed);

        // Must have InternalError failure code
        assert_eq!(
            final_import.failure_code,
            Some(FailureCode::InternalError),
            "Commit failure must produce InternalError failure code"
        );
    }

    #[tokio::test]
    async fn duplicate_import_does_not_invoke_committer() {
        // When duplicate detection returns ExactDuplicate, the committer
        // must never be called. This verifies that no write path is executed
        // for duplicates.
        let owner_id = UserId::new(Uuid::new_v4());
        let (gpx_bytes, checksum) = valid_gpx_and_checksum();
        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/dup-no-commit.gpx";

        let existing_import_id = ImportId::generate();
        let existing_activity_id = ActivityId::generate();

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), gpx_bytes);
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::ExactDuplicate {
                existing_import_id,
                existing_activity_id: Some(existing_activity_id),
            },
        };
        let committer = RecordingCommitter::new();

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            ImportProcessingResult::Duplicate { .. }
        ));

        // The committer must NOT have been invoked
        assert!(
            !committer.was_called(),
            "CommitImport::commit must never be called for duplicate imports"
        );
    }

    #[tokio::test]
    async fn normalization_failure_from_single_point_segment_fails_with_parse_error() {
        // A GPX file that is valid XML and passes the GPX parser but contains
        // only 1 trackpoint per segment. The normalization step skips segments
        // with < 2 points, leaving zero valid segments, which triggers a
        // NoSegments error that the orchestrator maps to ParseError failure code.
        let owner_id = UserId::new(Uuid::new_v4());

        // Valid GPX with only 1 trackpoint in a single segment
        let single_point_gpx = br#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test">
  <trk>
    <name>Single Point Track</name>
    <trkseg>
      <trkpt lat="47.0" lon="11.0">
        <ele>500.0</ele>
        <time>2024-01-15T08:00:00Z</time>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;

        let mut hasher = Sha256::new();
        hasher.update(single_point_gpx);
        let checksum = format!("{:x}", hasher.finalize());

        let import = queued_import(owner_id, &checksum);
        let import_id = import.id;
        let storage_key = "imports/test/single-point.gpx";

        let repo = MockRepo::new();
        repo.insert(import);

        let mut files = HashMap::new();
        files.insert(storage_key.to_string(), single_point_gpx.to_vec());
        let object_store = MockObjectStore { files };

        let duplicate_checker = MockDuplicateChecker {
            result: DuplicateCheckResult::NotDuplicate,
        };
        let committer = RecordingCommitter::new();

        let orchestrator = ImportOrchestrator {
            repo: &repo,
            object_store: &object_store,
            duplicate_checker: &duplicate_checker,
            committer: &committer,
        };

        let result = orchestrator
            .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImportError::ParsingFailed { .. }
        ));

        // Import must be in Failed state with ParseError code
        let final_import = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(final_import.status, ImportStatus::Failed);
        assert_eq!(
            final_import.failure_code,
            Some(FailureCode::ParseError),
            "Normalization failure from single-point segment must produce ParseError failure code"
        );
        assert!(
            final_import
                .failure_reason
                .as_ref()
                .unwrap()
                .contains("normalization failed"),
            "Failure reason must mention normalization"
        );

        // Committer must not have been called
        assert!(
            !committer.was_called(),
            "Committer must not be invoked when normalization fails"
        );
    }
}
