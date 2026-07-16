//! Authorization integration tests for GPX export download, get, expire, and request handlers.
//!
//! Verifies that every export state (Queued, Generating, Ready, Failed, Expired)
//! is handled correctly for download, and that ownership checks are enforced.

use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::exports::state_machine::ExportStatus;
use haiker_app::exports::{
    handle_download_export, handle_expire_export, handle_get_export, handle_request_export,
    ArtifactStore, ExportError, ExportFormat, ExportJob, ExportJobId, ExportRepository,
    RequestExportCommand, RouteVersionGateway,
};
use haiker_app::identity::UserId;
use haiker_app::route_versioning::RouteVersionId;
use haiker_test_support::builders::ExportJobBuilder;

// --- Test infrastructure ---

/// In-memory implementation of ExportRepository for tests.
struct InMemoryExportRepo {
    exports: Mutex<HashMap<ExportJobId, ExportJob>>,
}

impl InMemoryExportRepo {
    fn new() -> Self {
        Self {
            exports: Mutex::new(HashMap::new()),
        }
    }

    fn insert(&self, job: ExportJob) {
        self.exports.lock().unwrap().insert(job.id, job);
    }
}

#[async_trait]
impl ExportRepository for InMemoryExportRepo {
    async fn save(&self, export_job: &ExportJob) -> Result<(), ExportError> {
        self.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: ExportJobId) -> Result<Option<ExportJob>, ExportError> {
        Ok(self.exports.lock().unwrap().get(&id).cloned())
    }

    async fn find_by_idempotency_key(
        &self,
        owner_id: UserId,
        key: &str,
    ) -> Result<Option<ExportJob>, ExportError> {
        Ok(self
            .exports
            .lock()
            .unwrap()
            .values()
            .find(|e| e.requested_by == owner_id && e.idempotency_key == key)
            .cloned())
    }

    async fn update(&self, export_job: &ExportJob) -> Result<(), ExportError> {
        self.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());
        Ok(())
    }
}

/// Fake gateway that rejects with RouteVersionNotFound.
struct FakeGatewayNotFound;

#[async_trait]
impl RouteVersionGateway for FakeGatewayNotFound {
    async fn verify_owned_route_version(
        &self,
        _activity_id: ActivityId,
        _route_version_id: RouteVersionId,
        _owner_id: UserId,
    ) -> Result<(), ExportError> {
        Err(ExportError::RouteVersionNotFound)
    }
}

/// In-memory ArtifactStore for tests.
struct InMemoryArtifactStore {
    deleted_keys: Mutex<Vec<String>>,
}

impl InMemoryArtifactStore {
    fn new() -> Self {
        Self {
            deleted_keys: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ArtifactStore for InMemoryArtifactStore {
    async fn delete_artifact(&self, key: &str) -> Result<(), ExportError> {
        self.deleted_keys.lock().unwrap().push(key.to_string());
        Ok(())
    }
}

// --- Tests ---

#[tokio::test]
async fn download_authorized_for_owner_in_ready_state() {
    let owner = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Ready)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_download_export(job.id, owner, &repo).await;
    assert!(result.is_ok());
    let fetched = result.unwrap();
    assert_eq!(fetched.id, job.id);
    assert_eq!(fetched.status, ExportStatus::Ready);
}

#[tokio::test]
async fn download_returns_not_found_for_non_owner_in_ready_state() {
    let owner = UserId::new(Uuid::new_v4());
    let other = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Ready)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_download_export(job.id, other, &repo).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ExportError::NotFound));
}

#[tokio::test]
async fn download_returns_not_ready_for_queued_state() {
    let owner = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Queued)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_download_export(job.id, owner, &repo).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ExportError::NotReady { status } => assert_eq!(status, "queued"),
        other => panic!("expected NotReady, got: {other:?}"),
    }
}

#[tokio::test]
async fn download_returns_not_ready_for_generating_state() {
    let owner = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Generating)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_download_export(job.id, owner, &repo).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ExportError::NotReady { status } => assert_eq!(status, "generating"),
        other => panic!("expected NotReady, got: {other:?}"),
    }
}

#[tokio::test]
async fn download_returns_not_ready_for_failed_state() {
    let owner = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Failed)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_download_export(job.id, owner, &repo).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ExportError::NotReady { status } => assert_eq!(status, "failed"),
        other => panic!("expected NotReady, got: {other:?}"),
    }
}

#[tokio::test]
async fn download_returns_artifact_expired_for_expired_timestamp() {
    let owner = UserId::new(Uuid::new_v4());
    // Build a Ready job with an expires_at in the past
    let expired_at = Utc::now() - Duration::hours(1);
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Ready)
        .with_expires_at(expired_at)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_download_export(job.id, owner, &repo).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ExportError::ArtifactExpired));
}

#[tokio::test]
async fn get_export_authorized_for_owner() {
    let owner = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Ready)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_get_export(job.id, owner, &repo).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().id, job.id);
}

#[tokio::test]
async fn get_export_returns_not_found_for_non_owner() {
    let owner = UserId::new(Uuid::new_v4());
    let other = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Ready)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());

    let result = handle_get_export(job.id, other, &repo).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ExportError::NotFound));
}

#[tokio::test]
async fn expire_export_succeeds_for_ready_job() {
    let owner = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Ready)
        .with_object_storage_key("exports/user/file.gpx")
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());
    let artifact_store = InMemoryArtifactStore::new();

    let result = handle_expire_export(job.id, &repo, &artifact_store).await;
    assert!(result.is_ok());

    // Verify it is now expired
    let updated = repo.find_by_id(job.id).await.unwrap().unwrap();
    assert_eq!(updated.status, ExportStatus::Expired);
}

#[tokio::test]
async fn expire_export_is_idempotent_on_already_expired() {
    let owner = UserId::new(Uuid::new_v4());
    let job = ExportJobBuilder::new()
        .with_owner(owner)
        .with_status(ExportStatus::Expired)
        .build();

    let repo = InMemoryExportRepo::new();
    repo.insert(job.clone());
    let artifact_store = InMemoryArtifactStore::new();

    let result = handle_expire_export(job.id, &repo, &artifact_store).await;
    assert!(result.is_ok());

    // No artifact deletion should have occurred (early return)
    let deleted = artifact_store.deleted_keys.lock().unwrap().clone();
    assert!(deleted.is_empty());
}

#[tokio::test]
async fn request_export_rejects_unauthorized_route_version() {
    let owner = UserId::new(Uuid::new_v4());
    let repo = InMemoryExportRepo::new();
    let gateway = FakeGatewayNotFound;

    let cmd = RequestExportCommand {
        owner_id: owner,
        activity_id: ActivityId::new(Uuid::new_v4()),
        route_version_id: RouteVersionId::new(Uuid::new_v4()),
        format: ExportFormat::Gpx,
        idempotency_key: "auth-reject-test".to_string(),
    };

    let result = handle_request_export(cmd, &repo, &gateway).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ExportError::RouteVersionNotFound
    ));
}

#[tokio::test]
async fn download_returns_not_found_for_nonexistent_export_id() {
    let owner = UserId::new(Uuid::new_v4());
    let repo = InMemoryExportRepo::new();
    let fake_id = ExportJobId::generate();

    let result = handle_download_export(fake_id, owner, &repo).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ExportError::NotFound));
}
