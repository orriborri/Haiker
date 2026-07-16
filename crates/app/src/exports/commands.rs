//! Export command types and handlers.
//!
//! Pure domain logic for requesting and querying exports. Accepts trait-based
//! interfaces for repository and gateway, with no infrastructure dependencies.

use sha2::{Digest, Sha256};

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;
use crate::route_versioning::RouteVersionId;

use super::repository::ExportRepository;
use super::{
    ArtifactStore, ExportError, ExportFormat, ExportJob, ExportJobId, RouteVersionGateway,
};

/// Command to request a new export.
#[derive(Debug, Clone)]
pub struct RequestExportCommand {
    pub owner_id: UserId,
    pub activity_id: ActivityId,
    pub route_version_id: RouteVersionId,
    pub format: ExportFormat,
    pub idempotency_key: String,
}

/// Compute a deterministic SHA-256 hash of the export request payload fields.
///
/// Hashes the canonical representation of (activity_id, route_version_id, format)
/// to produce a stable fingerprint for idempotency payload comparison.
pub fn compute_payload_hash(
    activity_id: ActivityId,
    route_version_id: RouteVersionId,
    format: &ExportFormat,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(activity_id.0.as_bytes());
    hasher.update(b"\x00");
    hasher.update(route_version_id.0.as_bytes());
    hasher.update(b"\x00");
    hasher.update(format.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Handle the request export command.
///
/// Validates inputs, checks idempotency, verifies ownership of the route version,
/// creates the export job aggregate, and persists it.
pub async fn handle_request_export(
    cmd: RequestExportCommand,
    repo: &dyn ExportRepository,
    gateway: &dyn RouteVersionGateway,
) -> Result<ExportJob, ExportError> {
    // Validate idempotency key is not empty
    if cmd.idempotency_key.trim().is_empty() {
        return Err(ExportError::ValidationFailed {
            message: "idempotency key must not be empty".to_string(),
        });
    }

    // Compute payload hash for idempotency comparison
    let payload_hash = compute_payload_hash(cmd.activity_id, cmd.route_version_id, &cmd.format);

    // Check idempotency key for replay
    if let Some(existing) = repo
        .find_by_idempotency_key(cmd.owner_id, &cmd.idempotency_key)
        .await?
    {
        // Verify payload matches the original request
        if let Some(ref stored_hash) = existing.payload_hash {
            if *stored_hash != payload_hash {
                return Err(ExportError::IdempotencyPayloadMismatch);
            }
        }

        // Return existing export job (idempotent replay)
        return Ok(existing);
    }

    // Verify the route version belongs to the activity and the user owns the activity
    gateway
        .verify_owned_route_version(cmd.activity_id, cmd.route_version_id, cmd.owner_id)
        .await?;

    // Create new export job
    let export_job = ExportJob::new(
        cmd.owner_id,
        cmd.activity_id,
        cmd.route_version_id,
        cmd.format,
        cmd.idempotency_key,
        Some(payload_hash),
    )?;

    // Persist
    repo.save(&export_job).await?;

    Ok(export_job)
}

/// Handle get export status.
///
/// Retrieves export job by ID, verifying ownership.
pub async fn handle_get_export(
    export_id: ExportJobId,
    owner_id: UserId,
    repo: &dyn ExportRepository,
) -> Result<ExportJob, ExportError> {
    let export_job = repo
        .find_by_id(export_id)
        .await?
        .ok_or(ExportError::NotFound)?;

    if export_job.requested_by != owner_id {
        return Err(ExportError::NotFound);
    }

    Ok(export_job)
}

/// Handle export expiration.
///
/// Loads the export, deletes the artifact from storage, transitions to Expired,
/// and persists. This operation is idempotent: if the export is already expired,
/// returns Ok without error.
pub async fn handle_expire_export(
    export_id: ExportJobId,
    repo: &dyn ExportRepository,
    artifact_store: &dyn ArtifactStore,
) -> Result<(), ExportError> {
    let mut export_job = repo
        .find_by_id(export_id)
        .await?
        .ok_or(ExportError::NotFound)?;

    // Idempotent: already expired, nothing to do
    if export_job.status == super::state_machine::ExportStatus::Expired {
        return Ok(());
    }

    // Delete the artifact from storage if a key is present
    if let Some(ref key) = export_job.object_storage_key {
        artifact_store.delete_artifact(key).await?;
    }

    // Transition to Expired
    export_job.expire()?;

    // Persist
    repo.update(&export_job).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_catalog::ActivityId;
    use crate::exports::state_machine::ExportStatus;
    use crate::identity::UserId;
    use crate::route_versioning::RouteVersionId;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

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

    /// Fake gateway that always succeeds.
    struct FakeGatewayOk;

    #[async_trait]
    impl RouteVersionGateway for FakeGatewayOk {
        async fn verify_owned_route_version(
            &self,
            _activity_id: ActivityId,
            _route_version_id: RouteVersionId,
            _owner_id: UserId,
        ) -> Result<(), ExportError> {
            Ok(())
        }
    }

    /// Fake gateway that returns RouteVersionNotFound.
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

    /// Fake gateway that returns ActivityNotFound.
    struct FakeGatewayActivityNotFound;

    #[async_trait]
    impl RouteVersionGateway for FakeGatewayActivityNotFound {
        async fn verify_owned_route_version(
            &self,
            _activity_id: ActivityId,
            _route_version_id: RouteVersionId,
            _owner_id: UserId,
        ) -> Result<(), ExportError> {
            Err(ExportError::ActivityNotFound)
        }
    }

    /// Fake gateway that returns Unauthorized.
    struct FakeGatewayUnauthorized;

    #[async_trait]
    impl RouteVersionGateway for FakeGatewayUnauthorized {
        async fn verify_owned_route_version(
            &self,
            _activity_id: ActivityId,
            _route_version_id: RouteVersionId,
            _owner_id: UserId,
        ) -> Result<(), ExportError> {
            Err(ExportError::Unauthorized)
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

        fn deleted_keys(&self) -> Vec<String> {
            self.deleted_keys.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ArtifactStore for InMemoryArtifactStore {
        async fn delete_artifact(&self, key: &str) -> Result<(), ExportError> {
            self.deleted_keys.lock().unwrap().push(key.to_string());
            Ok(())
        }
    }

    /// In-memory ArtifactStore that always fails on delete_artifact.
    struct FailingArtifactStore;

    #[async_trait]
    impl ArtifactStore for FailingArtifactStore {
        async fn delete_artifact(&self, _key: &str) -> Result<(), ExportError> {
            Err(ExportError::PersistenceError {
                message: "storage unavailable".to_string(),
            })
        }
    }

    fn make_cmd(owner_id: UserId, idempotency_key: &str) -> RequestExportCommand {
        RequestExportCommand {
            owner_id,
            activity_id: ActivityId::new(Uuid::new_v4()),
            route_version_id: RouteVersionId::new(Uuid::new_v4()),
            format: ExportFormat::Gpx,
            idempotency_key: idempotency_key.to_string(),
        }
    }

    #[tokio::test]
    async fn request_export_succeeds() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = make_cmd(owner, "export-key-1");
        let result = handle_request_export(cmd, &repo, &gateway).await.unwrap();

        assert_eq!(result.status, ExportStatus::Queued);
        assert_eq!(result.requested_by, owner);
        assert_eq!(result.format, ExportFormat::Gpx);
    }

    #[tokio::test]
    async fn request_export_idempotency_returns_existing() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let cmd = RequestExportCommand {
            owner_id: owner,
            activity_id,
            route_version_id,
            format: ExportFormat::Gpx,
            idempotency_key: "export-idem-1".to_string(),
        };

        let first = handle_request_export(cmd.clone(), &repo, &gateway)
            .await
            .unwrap();
        let second = handle_request_export(cmd, &repo, &gateway).await.unwrap();

        assert_eq!(first.id, second.id);
    }

    #[tokio::test]
    async fn request_export_idempotency_mismatch_with_different_activity() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let cmd1 = RequestExportCommand {
            owner_id: owner,
            activity_id: ActivityId::new(Uuid::new_v4()),
            route_version_id,
            format: ExportFormat::Gpx,
            idempotency_key: "export-mismatch-1".to_string(),
        };

        handle_request_export(cmd1, &repo, &gateway).await.unwrap();

        let cmd2 = RequestExportCommand {
            owner_id: owner,
            activity_id: ActivityId::new(Uuid::new_v4()),
            route_version_id,
            format: ExportFormat::Gpx,
            idempotency_key: "export-mismatch-1".to_string(),
        };

        let err = handle_request_export(cmd2, &repo, &gateway)
            .await
            .unwrap_err();
        assert!(matches!(err, ExportError::IdempotencyPayloadMismatch));
    }

    #[tokio::test]
    async fn request_export_idempotency_mismatch_with_different_route_version() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());

        let cmd1 = RequestExportCommand {
            owner_id: owner,
            activity_id,
            route_version_id: RouteVersionId::new(Uuid::new_v4()),
            format: ExportFormat::Gpx,
            idempotency_key: "export-mismatch-2".to_string(),
        };

        handle_request_export(cmd1, &repo, &gateway).await.unwrap();

        let cmd2 = RequestExportCommand {
            owner_id: owner,
            activity_id,
            route_version_id: RouteVersionId::new(Uuid::new_v4()),
            format: ExportFormat::Gpx,
            idempotency_key: "export-mismatch-2".to_string(),
        };

        let err = handle_request_export(cmd2, &repo, &gateway)
            .await
            .unwrap_err();
        assert!(matches!(err, ExportError::IdempotencyPayloadMismatch));
    }

    #[tokio::test]
    async fn request_export_rejects_empty_idempotency_key() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = make_cmd(owner, "   ");
        let err = handle_request_export(cmd, &repo, &gateway)
            .await
            .unwrap_err();
        assert!(matches!(err, ExportError::ValidationFailed { .. }));
    }

    #[tokio::test]
    async fn request_export_fails_when_route_version_not_found() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayNotFound;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = make_cmd(owner, "export-rv-missing");
        let err = handle_request_export(cmd, &repo, &gateway)
            .await
            .unwrap_err();
        assert!(matches!(err, ExportError::RouteVersionNotFound));
    }

    #[tokio::test]
    async fn request_export_fails_when_activity_not_found() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayActivityNotFound;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = make_cmd(owner, "export-act-missing");
        let err = handle_request_export(cmd, &repo, &gateway)
            .await
            .unwrap_err();
        assert!(matches!(err, ExportError::ActivityNotFound));
    }

    #[tokio::test]
    async fn request_export_fails_when_unauthorized() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayUnauthorized;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = make_cmd(owner, "export-unauth");
        let err = handle_request_export(cmd, &repo, &gateway)
            .await
            .unwrap_err();
        assert!(matches!(err, ExportError::Unauthorized));
    }

    #[tokio::test]
    async fn get_export_returns_export_for_owner() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = make_cmd(owner, "export-get-1");
        let created = handle_request_export(cmd, &repo, &gateway).await.unwrap();

        let fetched = handle_get_export(created.id, owner, &repo).await.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn get_export_returns_not_found_for_wrong_owner() {
        let repo = InMemoryExportRepo::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());
        let other = UserId::new(Uuid::new_v4());

        let cmd = make_cmd(owner, "export-get-2");
        let created = handle_request_export(cmd, &repo, &gateway).await.unwrap();

        let err = handle_get_export(created.id, other, &repo)
            .await
            .unwrap_err();
        assert!(matches!(err, ExportError::NotFound));
    }

    #[tokio::test]
    async fn get_export_returns_not_found_for_missing_id() {
        let repo = InMemoryExportRepo::new();
        let owner = UserId::new(Uuid::new_v4());
        let fake_id = ExportJobId::generate();

        let err = handle_get_export(fake_id, owner, &repo).await.unwrap_err();
        assert!(matches!(err, ExportError::NotFound));
    }

    #[tokio::test]
    async fn compute_payload_hash_is_deterministic() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());
        let format = ExportFormat::Gpx;

        let hash1 = compute_payload_hash(activity_id, route_version_id, &format);
        let hash2 = compute_payload_hash(activity_id, route_version_id, &format);
        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn compute_payload_hash_differs_for_different_inputs() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id1 = RouteVersionId::new(Uuid::new_v4());
        let route_version_id2 = RouteVersionId::new(Uuid::new_v4());
        let format = ExportFormat::Gpx;

        let hash1 = compute_payload_hash(activity_id, route_version_id1, &format);
        let hash2 = compute_payload_hash(activity_id, route_version_id2, &format);
        assert_ne!(hash1, hash2);
    }

    #[tokio::test]
    async fn handle_expire_export_transitions_to_expired_and_deletes_artifact() {
        let repo = InMemoryExportRepo::new();
        let artifact_store = InMemoryArtifactStore::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());

        // Create and bring to Ready state
        let cmd = make_cmd(owner, "expire-test-1");
        let mut export_job = handle_request_export(cmd, &repo, &gateway).await.unwrap();
        export_job.start_generating().unwrap();
        let expires = chrono::Utc::now() + chrono::Duration::hours(24);
        export_job
            .complete(
                "exports/user/file.gpx".to_string(),
                "somehash".to_string(),
                expires,
            )
            .unwrap();
        repo.update(&export_job).await.unwrap();

        // Expire the export
        let result = handle_expire_export(export_job.id, &repo, &artifact_store).await;
        assert!(result.is_ok());

        // Verify it's expired in the repo
        let updated = repo.find_by_id(export_job.id).await.unwrap().unwrap();
        assert_eq!(updated.status, ExportStatus::Expired);

        // Verify artifact was deleted
        let deleted = artifact_store.deleted_keys();
        assert_eq!(deleted, vec!["exports/user/file.gpx"]);
    }

    #[tokio::test]
    async fn handle_expire_export_is_idempotent_on_already_expired() {
        let repo = InMemoryExportRepo::new();
        let artifact_store = InMemoryArtifactStore::new();
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());

        // Create, bring to Ready, then expire
        let cmd = make_cmd(owner, "expire-idem-1");
        let mut export_job = handle_request_export(cmd, &repo, &gateway).await.unwrap();
        export_job.start_generating().unwrap();
        let expires = chrono::Utc::now() + chrono::Duration::hours(24);
        export_job
            .complete(
                "exports/user/file2.gpx".to_string(),
                "hash2".to_string(),
                expires,
            )
            .unwrap();
        export_job.expire().unwrap();
        repo.update(&export_job).await.unwrap();

        // Call expire again - should be idempotent
        let result = handle_expire_export(export_job.id, &repo, &artifact_store).await;
        assert!(result.is_ok());

        // No artifact deletion should have occurred (early return)
        let deleted = artifact_store.deleted_keys();
        assert!(deleted.is_empty());
    }

    #[tokio::test]
    async fn handle_expire_export_returns_not_found_for_missing_export() {
        let repo = InMemoryExportRepo::new();
        let artifact_store = InMemoryArtifactStore::new();
        let fake_id = ExportJobId::generate();

        let result = handle_expire_export(fake_id, &repo, &artifact_store).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ExportError::NotFound));
    }

    #[tokio::test]
    async fn handle_expire_export_propagates_artifact_deletion_failure() {
        let repo = InMemoryExportRepo::new();
        let artifact_store = FailingArtifactStore;
        let gateway = FakeGatewayOk;
        let owner = UserId::new(Uuid::new_v4());

        // Create and bring to Ready state
        let cmd = make_cmd(owner, "expire-fail-delete");
        let mut export_job = handle_request_export(cmd, &repo, &gateway).await.unwrap();
        export_job.start_generating().unwrap();
        let expires = chrono::Utc::now() + chrono::Duration::hours(24);
        export_job
            .complete(
                "exports/user/fail.gpx".to_string(),
                "somehash".to_string(),
                expires,
            )
            .unwrap();
        repo.update(&export_job).await.unwrap();

        // Attempt to expire - should propagate the deletion error
        let result = handle_expire_export(export_job.id, &repo, &artifact_store).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExportError::PersistenceError { .. }
        ));

        // Verify the job stays in Ready state (not transitioned)
        let updated = repo.find_by_id(export_job.id).await.unwrap().unwrap();
        assert_eq!(updated.status, ExportStatus::Ready);
    }
}
