//! Integration tests proving the full import orchestrator flow using GPX fixtures
//! and in-memory trait implementations.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::identity::UserId;
use haiker_app::imports::checksum::Checksum;
use haiker_app::imports::commit::{CommitImport, ImportCommitData};
use haiker_app::imports::duplicate_detection::{CheckDuplicate, DuplicateCheckResult};
use haiker_app::imports::orchestrator::{ImportOrchestrator, ImportProcessingResult, ObjectStore};
use haiker_app::imports::repository::ImportRepository;
use haiker_app::imports::state_machine::ImportStatus;
use haiker_app::imports::{Import, ImportError, ImportFormat, ImportId};
use haiker_app::recorded_activity::SourceArtifactId;

use haiker_test_support::fixtures;

// --- In-memory mock implementations ---

struct InMemoryObjectStore {
    files: HashMap<String, Vec<u8>>,
}

#[async_trait]
impl ObjectStore for InMemoryObjectStore {
    async fn download(&self, key: &str) -> Result<Vec<u8>, ImportError> {
        self.files
            .get(key)
            .cloned()
            .ok_or(ImportError::StorageError {
                message: format!("not found: {key}"),
            })
    }
}

struct InMemoryRepo {
    imports: Mutex<HashMap<ImportId, Import>>,
}

impl InMemoryRepo {
    fn new() -> Self {
        Self {
            imports: Mutex::new(HashMap::new()),
        }
    }

    fn insert(&self, import: Import) {
        self.imports.lock().unwrap().insert(import.id, import);
    }

    fn get(&self, id: ImportId) -> Option<Import> {
        self.imports.lock().unwrap().get(&id).cloned()
    }
}

#[async_trait]
impl ImportRepository for InMemoryRepo {
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

    async fn find_abandoned(&self, _timeout: chrono::Duration) -> Result<Vec<Import>, ImportError> {
        Ok(vec![])
    }
}

struct InMemoryDuplicateChecker {
    known_checksums: Mutex<HashMap<String, (ImportId, Option<ActivityId>)>>,
}

impl InMemoryDuplicateChecker {
    fn new() -> Self {
        Self {
            known_checksums: Mutex::new(HashMap::new()),
        }
    }

    fn register(&self, checksum: &str, import_id: ImportId, activity_id: Option<ActivityId>) {
        self.known_checksums
            .lock()
            .unwrap()
            .insert(checksum.to_string(), (import_id, activity_id));
    }
}

#[async_trait]
impl CheckDuplicate for InMemoryDuplicateChecker {
    async fn check(
        &self,
        _owner_id: UserId,
        checksum: &Checksum,
    ) -> Result<DuplicateCheckResult, ImportError> {
        let guard = self.known_checksums.lock().unwrap();
        match guard.get(checksum.as_str()) {
            Some((import_id, activity_id)) => Ok(DuplicateCheckResult::ExactDuplicate {
                existing_import_id: *import_id,
                existing_activity_id: *activity_id,
            }),
            None => Ok(DuplicateCheckResult::NotDuplicate),
        }
    }
}

struct RecordingCommitter {
    committed: Mutex<Vec<ImportCommitData>>,
}

impl RecordingCommitter {
    fn new() -> Self {
        Self {
            committed: Mutex::new(Vec::new()),
        }
    }

    fn committed_count(&self) -> usize {
        self.committed.lock().unwrap().len()
    }

    fn last_commit(&self) -> Option<ImportCommitData> {
        self.committed.lock().unwrap().last().cloned()
    }
}

#[async_trait]
impl CommitImport for RecordingCommitter {
    async fn commit(&self, data: &ImportCommitData) -> Result<ActivityId, ImportError> {
        self.committed.lock().unwrap().push(data.clone());
        Ok(data.activity_id)
    }
}

// --- Helper functions ---

fn compute_checksum(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn create_queued_import(owner_id: UserId, checksum: &str) -> Import {
    let mut import = Import::new(
        owner_id,
        ImportFormat::Gpx,
        Uuid::new_v4().to_string(),
        None,
    )
    .unwrap();
    import.start_upload().unwrap();
    let artifact_id = SourceArtifactId::generate();
    let cs = Checksum::new(checksum).unwrap();
    import.complete_upload(artifact_id, cs).unwrap();
    import.start_validation().unwrap();
    import.queue_for_parsing().unwrap();
    import
}

// --- Integration tests ---

/// Proves the full orchestrator flow: create import -> provide file bytes ->
/// orchestrator processes it -> activity is created via CommitImport trait.
#[tokio::test]
async fn full_orchestrator_flow_with_valid_fixture() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/valid_simple.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };

    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

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

    // Verify activity was created
    match result {
        ImportProcessingResult::Completed { activity_id, .. } => {
            assert_ne!(activity_id.0, Uuid::nil());
        }
        other => panic!("Expected Completed, got {:?}", other),
    }

    // Verify commit was called exactly once
    assert_eq!(committer.committed_count(), 1);

    // Verify commit data has sensible values
    let commit_data = committer.last_commit().unwrap();
    assert_eq!(commit_data.owner_id, owner_id);
    assert_eq!(commit_data.import_id, import_id);
    assert!(commit_data.statistics.point_count >= 2);
    assert!(commit_data.statistics.distance_meters > 0.0);
    assert!(!commit_data.segments.is_empty());
}

/// Proves GPX 1.0 files are processed successfully.
#[tokio::test]
async fn orchestrator_processes_gpx10_fixture() {
    let gpx_bytes = fixtures::valid_gpx10().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/gpx10.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

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

    assert!(matches!(result, ImportProcessingResult::Completed { .. }));
    assert_eq!(committer.committed_count(), 1);
}

/// Proves multi-segment GPX is processed and preserves segment count.
#[tokio::test]
async fn orchestrator_processes_multi_segment_fixture() {
    let gpx_bytes = fixtures::valid_multi_segment().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/multi_segment.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

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

    assert!(matches!(result, ImportProcessingResult::Completed { .. }));

    let commit_data = committer.last_commit().unwrap();
    assert_eq!(commit_data.statistics.segment_count, 3);
}

/// Proves: same file imported twice results in duplicate detection on second.
#[tokio::test]
async fn duplicate_detection_on_second_import() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/simple.gpx";

    // First import succeeds normally
    let import1 = create_queued_import(owner_id, &checksum);
    let import1_id = import1.id;

    let repo = InMemoryRepo::new();
    repo.insert(import1);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes.clone());
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

    let orchestrator = ImportOrchestrator {
        repo: &repo,
        object_store: &object_store,
        duplicate_checker: &duplicate_checker,
        committer: &committer,
    };

    let result1 = orchestrator
        .process_import(import1_id, owner_id, storage_key, Uuid::new_v4())
        .await
        .unwrap();

    let first_activity_id = match result1 {
        ImportProcessingResult::Completed { activity_id, .. } => activity_id,
        other => panic!("Expected Completed, got {:?}", other),
    };

    // Now register the checksum as a known duplicate for the second import
    duplicate_checker.register(&checksum, import1_id, Some(first_activity_id));

    // Second import of the same file triggers duplicate
    let import2 = create_queued_import(owner_id, &checksum);
    let import2_id = import2.id;
    repo.insert(import2);

    let result2 = orchestrator
        .process_import(import2_id, owner_id, storage_key, Uuid::new_v4())
        .await
        .unwrap();

    match result2 {
        ImportProcessingResult::Duplicate {
            existing_import_id,
            existing_activity_id,
        } => {
            assert_eq!(existing_import_id, import1_id);
            assert_eq!(existing_activity_id, Some(first_activity_id));
        }
        other => panic!("Expected Duplicate, got {:?}", other),
    }

    // Verify: only one commit happened (the first import), not the duplicate
    assert_eq!(committer.committed_count(), 1);

    // Verify second import ended in Completed (duplicate is a successful outcome)
    let final_import2 = repo.get(import2_id).unwrap();
    assert_eq!(final_import2.status, ImportStatus::Completed);
}

/// Proves corrupt GPX fails with correct error.
#[tokio::test]
async fn malformed_gpx_fails_with_parsing_error() {
    let gpx_bytes = fixtures::malformed_xml().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/broken.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
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

    // Verify import ended in Failed state
    let final_import = repo.get(import_id).unwrap();
    assert_eq!(final_import.status, ImportStatus::Failed);
    assert!(final_import.failure_reason.is_some());

    // Verify no commit was attempted
    assert_eq!(committer.committed_count(), 0);
}

/// Proves invalid coordinates GPX fails with parsing error.
#[tokio::test]
async fn invalid_coordinates_gpx_fails() {
    let gpx_bytes = fixtures::invalid_coordinates().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/invalid_coords.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
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
}

/// Proves XXE attack GPX is rejected.
#[tokio::test]
async fn xxe_attack_gpx_is_rejected() {
    let gpx_bytes = fixtures::xxe_attack().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/xxe.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
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
}

/// Proves excessive depth GPX is rejected.
#[tokio::test]
async fn excessive_depth_gpx_is_rejected() {
    let gpx_bytes = fixtures::excessive_depth().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/deep.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
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
}

/// Proves no-elevation GPX processes without error.
#[tokio::test]
async fn no_elevation_gpx_processes_successfully() {
    let gpx_bytes = fixtures::valid_no_elevation().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/no_elev.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

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

    assert!(matches!(result, ImportProcessingResult::Completed { .. }));

    let commit_data = committer.last_commit().unwrap();
    // No elevation data means no gain/loss computed
    assert!(commit_data.statistics.elevation_gain_meters.is_none());
    assert!(commit_data.statistics.elevation_loss_meters.is_none());
}

/// Proves no-timestamps GPX processes without error.
#[tokio::test]
async fn no_timestamps_gpx_processes_successfully() {
    let gpx_bytes = fixtures::valid_no_timestamps().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/no_time.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

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

    assert!(matches!(result, ImportProcessingResult::Completed { .. }));

    let commit_data = committer.last_commit().unwrap();
    // No timestamps means no duration computed
    assert!(commit_data.statistics.duration_seconds.is_none());
    assert!(commit_data.started_at.is_none());
    assert!(commit_data.ended_at.is_none());
}

/// Proves the large route (1000 points) processes successfully.
#[tokio::test]
async fn large_route_processes_successfully() {
    let gpx_bytes = fixtures::large_route().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/large.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

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

    assert!(matches!(result, ImportProcessingResult::Completed { .. }));

    let commit_data = committer.last_commit().unwrap();
    assert_eq!(commit_data.statistics.point_count, 1000);
    assert!(commit_data.statistics.distance_meters > 0.0);
}

/// Proves non-ASCII metadata GPX processes without error.
#[tokio::test]
async fn non_ascii_gpx_processes_successfully() {
    let gpx_bytes = fixtures::valid_non_ascii().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/non_ascii.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    let mut files = HashMap::new();
    files.insert(storage_key.to_string(), gpx_bytes);
    let object_store = InMemoryObjectStore { files };
    let duplicate_checker = InMemoryDuplicateChecker::new();
    let committer = RecordingCommitter::new();

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

    assert!(matches!(result, ImportProcessingResult::Completed { .. }));

    let commit_data = committer.last_commit().unwrap();
    // Should pick up the track name as suggested title
    assert_eq!(commit_data.activity_title, "Fuji-san Trailhead");
}
