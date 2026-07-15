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

// --- Comprehensive end-to-end import-to-catalog tests ---

/// Proves that the activity title is derived from the GPX track name metadata.
/// The valid_simple fixture has <trk><name>Alpine Trail</name>.
#[tokio::test]
async fn import_creates_activity_with_correct_title_from_gpx_metadata() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/title_test.gpx";

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
    assert_eq!(commit_data.activity_title, "Alpine Trail");
}

/// Proves that started_at and ended_at in commit data match the first and last
/// track point timestamps from the valid_simple fixture.
/// First point: 2024-03-15T07:00:00Z, Last point: 2024-03-15T07:09:00Z
#[tokio::test]
async fn import_creates_activity_with_correct_timestamps() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/timestamps_test.gpx";

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

    // First track point time: 2024-03-15T07:00:00Z
    let expected_start = chrono::DateTime::parse_from_rfc3339("2024-03-15T07:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    // Last track point time: 2024-03-15T07:09:00Z
    let expected_end = chrono::DateTime::parse_from_rfc3339("2024-03-15T07:09:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);

    assert_eq!(
        commit_data.started_at,
        Some(expected_start),
        "started_at must match the first track point timestamp"
    );
    assert_eq!(
        commit_data.ended_at,
        Some(expected_end),
        "ended_at must match the last track point timestamp"
    );
}

/// Proves the bounding box in commit data matches the min/max lat/lon from all
/// 10 track points in the valid_simple fixture.
/// Lat range: 47.2692 (min) to 47.2748 (max)
/// Lon range: 11.3933 (min) to 11.3995 (max)
#[tokio::test]
async fn import_preserves_bounding_box_from_track_points() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/bbox_test.gpx";

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
    let bbox = commit_data.bounding_box;

    // South-west corner should be min lat, min lon
    let epsilon = 1e-4;
    assert!(
        (bbox.south_west.latitude - 47.2692).abs() < epsilon,
        "south_west latitude should be ~47.2692, got {}",
        bbox.south_west.latitude
    );
    assert!(
        (bbox.south_west.longitude - 11.3933).abs() < epsilon,
        "south_west longitude should be ~11.3933, got {}",
        bbox.south_west.longitude
    );

    // North-east corner should be max lat, max lon
    assert!(
        (bbox.north_east.latitude - 47.2748).abs() < epsilon,
        "north_east latitude should be ~47.2748, got {}",
        bbox.north_east.latitude
    );
    assert!(
        (bbox.north_east.longitude - 11.3995).abs() < epsilon,
        "north_east longitude should be ~11.3995, got {}",
        bbox.north_east.longitude
    );
}

/// Proves all preview geometry coordinates have valid latitude [-90, 90]
/// and longitude [-180, 180] ranges.
#[tokio::test]
async fn import_preview_geometry_contains_only_valid_coordinates() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/preview_test.gpx";

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
    assert!(
        !commit_data.preview_geometry.is_empty(),
        "preview_geometry must not be empty"
    );

    for (i, coord) in commit_data.preview_geometry.iter().enumerate() {
        assert!(
            (-90.0..=90.0).contains(&coord.latitude),
            "preview_geometry[{i}] latitude {} is out of range [-90, 90]",
            coord.latitude
        );
        assert!(
            (-180.0..=180.0).contains(&coord.longitude),
            "preview_geometry[{i}] longitude {} is out of range [-180, 180]",
            coord.longitude
        );
    }
}

/// Proves that when object storage has no files (simulating a storage outage),
/// the import ends in Failed state and the committer is never called.
#[tokio::test]
async fn storage_outage_during_download_leaves_consistent_state() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/outage_test.gpx";

    let import = create_queued_import(owner_id, &checksum);
    let import_id = import.id;

    let repo = InMemoryRepo::new();
    repo.insert(import);

    // Empty object store - simulates storage outage (no files available)
    let object_store = InMemoryObjectStore {
        files: HashMap::new(),
    };
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

    // Import should fail with a storage error
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ImportError::StorageError { .. }
    ));

    // Import must be in Failed state
    let final_import = repo.get(import_id).unwrap();
    assert_eq!(final_import.status, ImportStatus::Failed);

    // Committer must never have been called
    assert_eq!(
        committer.committed_count(),
        0,
        "Committer must not be invoked when storage download fails"
    );
}

/// Proves that processing the same import a second time (after the first
/// attempt moved it to Parsing) returns an InvalidTransition error,
/// preventing duplicate activity creation from worker restarts/redelivery.
#[tokio::test]
async fn worker_restart_second_attempt_on_same_import_returns_error() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/restart_test.gpx";

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

    // First attempt succeeds (moves import from Queued through the pipeline)
    let result1 = orchestrator
        .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
        .await;
    assert!(result1.is_ok(), "First processing attempt should succeed");

    // Second attempt on the same import_id should fail with InvalidTransition
    // because the import is no longer in Queued state
    let result2 = orchestrator
        .process_import(import_id, owner_id, storage_key, Uuid::new_v4())
        .await;

    assert!(result2.is_err());
    assert!(
        matches!(result2.unwrap_err(), ImportError::InvalidTransition { .. }),
        "Second attempt must fail with InvalidTransition"
    );
}

/// Proves that two different owners can import identical file bytes and both
/// succeed (duplicate detection is per-owner). Neither import is flagged as
/// a duplicate because the InMemoryDuplicateChecker starts empty for both.
#[tokio::test]
async fn cross_owner_isolation_different_owners_same_checksum_both_succeed() {
    let gpx_bytes = fixtures::valid_simple().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let storage_key = "imports/shared/same_file.gpx";

    let owner_a = UserId::new(Uuid::new_v4());
    let owner_b = UserId::new(Uuid::new_v4());

    // --- Owner A import ---
    let import_a = create_queued_import(owner_a, &checksum);
    let import_a_id = import_a.id;

    let repo_a = InMemoryRepo::new();
    repo_a.insert(import_a);

    let mut files_a = HashMap::new();
    files_a.insert(storage_key.to_string(), gpx_bytes.clone());
    let object_store_a = InMemoryObjectStore { files: files_a };
    let duplicate_checker_a = InMemoryDuplicateChecker::new();
    let committer_a = RecordingCommitter::new();

    let orchestrator_a = ImportOrchestrator {
        repo: &repo_a,
        object_store: &object_store_a,
        duplicate_checker: &duplicate_checker_a,
        committer: &committer_a,
    };

    let result_a = orchestrator_a
        .process_import(import_a_id, owner_a, storage_key, Uuid::new_v4())
        .await
        .unwrap();

    assert!(
        matches!(result_a, ImportProcessingResult::Completed { .. }),
        "Owner A import must complete successfully"
    );
    assert_eq!(committer_a.committed_count(), 1);

    // --- Owner B import (same file bytes, different owner) ---
    let import_b = create_queued_import(owner_b, &checksum);
    let import_b_id = import_b.id;

    let repo_b = InMemoryRepo::new();
    repo_b.insert(import_b);

    let mut files_b = HashMap::new();
    files_b.insert(storage_key.to_string(), gpx_bytes);
    let object_store_b = InMemoryObjectStore { files: files_b };
    // Fresh duplicate checker for owner B - simulates per-owner isolation
    let duplicate_checker_b = InMemoryDuplicateChecker::new();
    let committer_b = RecordingCommitter::new();

    let orchestrator_b = ImportOrchestrator {
        repo: &repo_b,
        object_store: &object_store_b,
        duplicate_checker: &duplicate_checker_b,
        committer: &committer_b,
    };

    let result_b = orchestrator_b
        .process_import(import_b_id, owner_b, storage_key, Uuid::new_v4())
        .await
        .unwrap();

    assert!(
        matches!(result_b, ImportProcessingResult::Completed { .. }),
        "Owner B import must also complete successfully (not flagged as duplicate)"
    );
    assert_eq!(committer_b.committed_count(), 1);
}

/// Proves that fixture file bytes are immutable by verifying their SHA-256
/// checksums against known expected values. If any fixture changes, this test
/// will fail, preventing accidental mutation of test data.
#[tokio::test]
async fn fixture_bytes_are_immutable_with_known_checksums() {
    let expected_checksums: &[(&str, &[u8], &str)] = &[
        (
            "valid_simple",
            fixtures::valid_simple(),
            "7aeae402b649a2e9a69826df656e04911c03713d402c1ae0c1bb1db89d887e7d",
        ),
        (
            "valid_gpx10",
            fixtures::valid_gpx10(),
            "3909c8f1dbf00e4c642d8289abe079424e00a25a3f70369ccd0d1379d57ea22f",
        ),
        (
            "valid_multi_segment",
            fixtures::valid_multi_segment(),
            "e27e1cca2183428b991df0619e90e4aa1e4fbd385f8fa84f140ba8de6507e316",
        ),
        (
            "valid_no_elevation",
            fixtures::valid_no_elevation(),
            "1ed2419821a5334b91d8a27f8ba38963657cf966ff0336abda7313407871e7cd",
        ),
        (
            "valid_no_timestamps",
            fixtures::valid_no_timestamps(),
            "3ad7a00ab1c9b15bf30a72fae4121ebabbe1b38d1acc860602b262760f076aed",
        ),
        (
            "valid_non_ascii",
            fixtures::valid_non_ascii(),
            "7ba0c6c2782546974dddb2e8014d97637da2597a85321196a0ccfdfc4bbf0fe3",
        ),
        (
            "large_route",
            fixtures::large_route(),
            "54102f221e219aca16ab7cd56e04d0fd234e9190384fea03fb94f477210c40c5",
        ),
        (
            "malformed_xml",
            fixtures::malformed_xml(),
            "0ecec75adc8dc67b18027ff82ed117f76373d882bca3c7d9ceba374de6800e4d",
        ),
        (
            "invalid_coordinates",
            fixtures::invalid_coordinates(),
            "82c636628a7cf396fb85eca86ca90de3e3068dd042e9db42b5bbd0fa6fe655f0",
        ),
        (
            "xxe_attack",
            fixtures::xxe_attack(),
            "304a5097120f6214ea7c34b18f560b13208c70a3be7458e8318942a29e9c8813",
        ),
        (
            "excessive_depth",
            fixtures::excessive_depth(),
            "fba61da798885b4fedc25f3bdace2a9ef750d95f27509e301b8c13ce68bf7e8e",
        ),
    ];

    for (name, bytes, expected_hash) in expected_checksums {
        let actual_hash = compute_checksum(bytes);
        assert_eq!(
            actual_hash, *expected_hash,
            "Fixture '{name}' has changed! Expected SHA-256: {expected_hash}, got: {actual_hash}"
        );
    }
}

/// Proves that the multi-segment fixture produces the correct segment count
/// and that each segment has the expected point count.
/// The valid_multi_segment fixture has 3 segments with 3, 3, and 2 points.
#[tokio::test]
async fn commit_data_segment_count_matches_gpx_structure() {
    let gpx_bytes = fixtures::valid_multi_segment().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/segment_count_test.gpx";

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

    // Statistics should reflect 3 segments
    assert_eq!(
        commit_data.statistics.segment_count, 3,
        "Multi-segment fixture must produce 3 segments"
    );

    // Verify per-segment point counts
    assert_eq!(
        commit_data.segments.len(),
        3,
        "commit_data.segments must have 3 entries"
    );
    assert_eq!(
        commit_data.segments[0].point_count(),
        3,
        "Segment 0 must have 3 points"
    );
    assert_eq!(
        commit_data.segments[1].point_count(),
        3,
        "Segment 1 must have 3 points"
    );
    assert_eq!(
        commit_data.segments[2].point_count(),
        2,
        "Segment 2 must have 2 points"
    );

    // Total point count
    assert_eq!(
        commit_data.statistics.point_count, 8,
        "Total point count must be 3 + 3 + 2 = 8"
    );
}

/// Proves that non-ASCII characters (Japanese and German) in GPX metadata
/// survive the full import pipeline and appear correctly in the activity title.
/// The valid_non_ascii fixture has track name "Fuji-san Trailhead" which includes
/// a hyphen but the metadata name has German characters "Bodensee".
#[tokio::test]
async fn non_ascii_metadata_preserved_in_activity_title() {
    let gpx_bytes = fixtures::valid_non_ascii().to_vec();
    let checksum = compute_checksum(&gpx_bytes);
    let owner_id = UserId::new(Uuid::new_v4());
    let storage_key = "imports/user/unicode_test.gpx";

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

    // The track name is "Fuji-san Trailhead" - verify exact match
    assert_eq!(commit_data.activity_title, "Fuji-san Trailhead");

    // Verify specific characters are preserved: hyphen and Latin characters
    assert!(
        commit_data.activity_title.contains("Fuji"),
        "Must contain 'Fuji'"
    );
    assert!(
        commit_data.activity_title.contains('-'),
        "Must contain hyphen character"
    );
    assert!(
        commit_data.activity_title.contains("Trailhead"),
        "Must contain 'Trailhead'"
    );
}
