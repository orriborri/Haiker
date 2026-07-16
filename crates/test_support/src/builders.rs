//! Test builder utilities for convenient test data construction.
//!
//! Provides fluent builder APIs for creating domain objects with sensible
//! defaults. Builders simplify test setup by only requiring specification
//! of the fields relevant to each test case.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use haiker_app::activity_catalog::{Activity, ActivityId, ActivityTitle, ActivityType};
use haiker_app::exports;
use haiker_app::identity::{Actor, UserId};
use haiker_app::imports::checksum::Checksum;
use haiker_app::imports::state_machine::ImportStatus;
use haiker_app::imports::{Import, ImportFormat, ImportId};
use haiker_app::recorded_activity::SourceArtifactId;
use haiker_app::route_versioning::RouteVersionId;

/// Builder for creating `Import` domain objects with sensible defaults.
///
/// # Example
/// ```ignore
/// let import = ImportBuilder::new()
///     .with_status(ImportStatus::Uploaded)
///     .with_owner(owner_id)
///     .build();
/// ```
pub struct ImportBuilder {
    owner_id: UserId,
    format: ImportFormat,
    idempotency_key: String,
    target_status: ImportStatus,
    checksum: Option<String>,
    payload_hash: Option<String>,
}

impl Default for ImportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ImportBuilder {
    /// Create a new ImportBuilder with default values.
    pub fn new() -> Self {
        Self {
            owner_id: UserId::new(Uuid::new_v4()),
            format: ImportFormat::Gpx,
            idempotency_key: Uuid::new_v4().to_string(),
            target_status: ImportStatus::Requested,
            checksum: None,
            payload_hash: None,
        }
    }

    /// Set the owner of the import.
    pub fn with_owner(mut self, owner_id: UserId) -> Self {
        self.owner_id = owner_id;
        self
    }

    /// Set the import format.
    pub fn with_format(mut self, format: ImportFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the idempotency key.
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = key.into();
        self
    }

    /// Set the target status to advance the import to.
    pub fn with_status(mut self, status: ImportStatus) -> Self {
        self.target_status = status;
        self
    }

    /// Set the checksum (used when advancing to Uploaded or beyond).
    pub fn with_checksum(mut self, checksum: impl Into<String>) -> Self {
        self.checksum = Some(checksum.into());
        self
    }

    /// Set the payload hash for idempotency verification.
    pub fn with_payload_hash(mut self, hash: impl Into<String>) -> Self {
        self.payload_hash = Some(hash.into());
        self
    }

    /// Build the Import, advancing through state transitions as needed.
    ///
    /// Panics if any transition fails (this is a test helper).
    pub fn build(self) -> Import {
        let mut import = Import::new(
            self.owner_id,
            self.format,
            self.idempotency_key,
            self.payload_hash,
        )
        .unwrap();

        let checksum_str = self.checksum.unwrap_or_else(|| "a".repeat(64));

        match self.target_status {
            ImportStatus::Requested => {}
            ImportStatus::Uploading => {
                import.start_upload().unwrap();
            }
            ImportStatus::Uploaded => {
                import.start_upload().unwrap();
                let artifact_id = SourceArtifactId::generate();
                let checksum = Checksum::new(checksum_str).unwrap();
                import.complete_upload(artifact_id, checksum).unwrap();
            }
            ImportStatus::Validating => {
                import.start_upload().unwrap();
                let artifact_id = SourceArtifactId::generate();
                let checksum = Checksum::new(checksum_str).unwrap();
                import.complete_upload(artifact_id, checksum).unwrap();
                import.start_validation().unwrap();
            }
            ImportStatus::Queued => {
                import.start_upload().unwrap();
                let artifact_id = SourceArtifactId::generate();
                let checksum = Checksum::new(checksum_str).unwrap();
                import.complete_upload(artifact_id, checksum).unwrap();
                import.start_validation().unwrap();
                import.queue_for_parsing().unwrap();
            }
            ImportStatus::Parsing => {
                import.start_upload().unwrap();
                let artifact_id = SourceArtifactId::generate();
                let checksum = Checksum::new(checksum_str).unwrap();
                import.complete_upload(artifact_id, checksum).unwrap();
                import.start_validation().unwrap();
                import.queue_for_parsing().unwrap();
                import.start_parsing().unwrap();
            }
            ImportStatus::Committing => {
                import.start_upload().unwrap();
                let artifact_id = SourceArtifactId::generate();
                let checksum = Checksum::new(checksum_str).unwrap();
                import.complete_upload(artifact_id, checksum).unwrap();
                import.start_validation().unwrap();
                import.queue_for_parsing().unwrap();
                import.start_parsing().unwrap();
                import.start_committing().unwrap();
            }
            ImportStatus::Completed => {
                import.start_upload().unwrap();
                let artifact_id = SourceArtifactId::generate();
                let checksum = Checksum::new(checksum_str).unwrap();
                import.complete_upload(artifact_id, checksum).unwrap();
                import.start_validation().unwrap();
                import.queue_for_parsing().unwrap();
                import.start_parsing().unwrap();
                import.start_committing().unwrap();
                import.complete().unwrap();
            }
            ImportStatus::Failed => {
                import.fail("test failure reason".to_string()).unwrap();
            }
            ImportStatus::Cancelled => {
                import.cancel().unwrap();
            }
        }

        import
    }
}

/// Builder for creating `Actor` instances for testing.
pub struct ActorBuilder {
    user_id: UserId,
}

impl Default for ActorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ActorBuilder {
    /// Create a new ActorBuilder with a random user ID.
    pub fn new() -> Self {
        Self {
            user_id: UserId::new(Uuid::new_v4()),
        }
    }

    /// Set the user ID for the actor.
    pub fn with_user_id(mut self, user_id: UserId) -> Self {
        self.user_id = user_id;
        self
    }

    /// Build the Actor.
    pub fn build(self) -> Actor {
        Actor::new(self.user_id)
    }

    /// Get the user ID (useful for referencing in related test data).
    pub fn user_id(&self) -> UserId {
        self.user_id
    }
}

/// Builder for creating `Activity` instances for testing.
pub struct ActivityBuilder {
    owner_id: UserId,
    title: String,
    activity_type: ActivityType,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
}

impl Default for ActivityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityBuilder {
    /// Create a new ActivityBuilder with default values.
    pub fn new() -> Self {
        Self {
            owner_id: UserId::new(Uuid::new_v4()),
            title: "Test Activity".to_string(),
            activity_type: ActivityType::Hike,
            started_at: None,
            ended_at: None,
        }
    }

    /// Set the owner of the activity.
    pub fn with_owner(mut self, owner_id: UserId) -> Self {
        self.owner_id = owner_id;
        self
    }

    /// Set the activity title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the activity type.
    pub fn with_type(mut self, activity_type: ActivityType) -> Self {
        self.activity_type = activity_type;
        self
    }

    /// Set the start time.
    pub fn with_started_at(mut self, started_at: DateTime<Utc>) -> Self {
        self.started_at = Some(started_at);
        self
    }

    /// Set the end time.
    pub fn with_ended_at(mut self, ended_at: DateTime<Utc>) -> Self {
        self.ended_at = Some(ended_at);
        self
    }

    /// Build the Activity.
    pub fn build(self) -> Activity {
        let title = ActivityTitle::new(self.title).unwrap();
        Activity::new(
            self.owner_id,
            title,
            self.activity_type,
            self.started_at,
            self.ended_at,
        )
    }
}

/// Builder for creating `ExportJob` domain objects with sensible defaults.
///
/// # Example
/// ```ignore
/// let export_job = ExportJobBuilder::new()
///     .with_status(ExportStatus::Ready)
///     .with_owner(owner_id)
///     .build();
/// ```
pub struct ExportJobBuilder {
    owner_id: UserId,
    activity_id: ActivityId,
    route_version_id: RouteVersionId,
    format: exports::ExportFormat,
    idempotency_key: String,
    payload_hash: Option<String>,
    target_status: exports::state_machine::ExportStatus,
    object_storage_key: Option<String>,
    checksum: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    failure_reason: Option<String>,
}

impl Default for ExportJobBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ExportJobBuilder {
    /// Create a new ExportJobBuilder with default values.
    pub fn new() -> Self {
        Self {
            owner_id: UserId::new(Uuid::new_v4()),
            activity_id: ActivityId::new(Uuid::new_v4()),
            route_version_id: RouteVersionId::new(Uuid::new_v4()),
            format: exports::ExportFormat::Gpx,
            idempotency_key: Uuid::new_v4().to_string(),
            payload_hash: None,
            target_status: exports::state_machine::ExportStatus::Queued,
            object_storage_key: None,
            checksum: None,
            expires_at: None,
            failure_reason: None,
        }
    }

    /// Set the owner of the export job.
    pub fn with_owner(mut self, owner_id: UserId) -> Self {
        self.owner_id = owner_id;
        self
    }

    /// Set the activity ID.
    pub fn with_activity_id(mut self, activity_id: ActivityId) -> Self {
        self.activity_id = activity_id;
        self
    }

    /// Set the route version ID.
    pub fn with_route_version_id(mut self, route_version_id: RouteVersionId) -> Self {
        self.route_version_id = route_version_id;
        self
    }

    /// Set the export format.
    pub fn with_format(mut self, format: exports::ExportFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the idempotency key.
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = key.into();
        self
    }

    /// Set the target status to advance the export job to.
    pub fn with_status(mut self, status: exports::state_machine::ExportStatus) -> Self {
        self.target_status = status;
        self
    }

    /// Set the object storage key (used when status is Ready or beyond).
    pub fn with_object_storage_key(mut self, key: impl Into<String>) -> Self {
        self.object_storage_key = Some(key.into());
        self
    }

    /// Set the checksum (used when status is Ready or beyond).
    pub fn with_checksum(mut self, checksum: impl Into<String>) -> Self {
        self.checksum = Some(checksum.into());
        self
    }

    /// Set the expiration time (used when status is Ready or beyond).
    pub fn with_expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set the failure reason (used when status is Failed).
    pub fn with_failure_reason(mut self, reason: impl Into<String>) -> Self {
        self.failure_reason = Some(reason.into());
        self
    }

    /// Build the ExportJob, advancing through state transitions as needed.
    ///
    /// Panics if any transition fails (this is a test helper).
    pub fn build(self) -> exports::ExportJob {
        use exports::state_machine::ExportStatus;

        let mut job = exports::ExportJob::new(
            self.owner_id,
            self.activity_id,
            self.route_version_id,
            self.format,
            self.idempotency_key,
            self.payload_hash,
        )
        .unwrap();

        let storage_key = self
            .object_storage_key
            .unwrap_or_else(|| format!("exports/{}/{}.gpx", self.owner_id.0, job.id.0));
        let checksum = self.checksum.unwrap_or_else(|| "a".repeat(64));
        let expires_at = self.expires_at.unwrap_or_else(|| {
            if self.target_status == ExportStatus::Expired {
                Utc::now() - chrono::Duration::hours(1)
            } else {
                Utc::now() + chrono::Duration::hours(24)
            }
        });

        match self.target_status {
            ExportStatus::Queued => {}
            ExportStatus::Generating => {
                job.start_generating().unwrap();
            }
            ExportStatus::Ready => {
                job.start_generating().unwrap();
                job.complete(storage_key, checksum, expires_at).unwrap();
            }
            ExportStatus::Failed => {
                let reason = self
                    .failure_reason
                    .unwrap_or_else(|| "test failure reason".to_string());
                job.start_generating().unwrap();
                job.fail(reason).unwrap();
            }
            ExportStatus::Expired => {
                job.start_generating().unwrap();
                job.complete(storage_key, checksum, expires_at).unwrap();
                job.expire().unwrap();
            }
        }

        job
    }
}

/// Helper to generate a random valid SHA-256 checksum string.
pub fn random_checksum() -> String {
    format!("{:064x}", Uuid::new_v4().as_u128())
}

/// Helper to generate a test user ID.
pub fn test_user_id() -> UserId {
    UserId::new(Uuid::new_v4())
}

/// Helper to generate a test import ID.
pub fn test_import_id() -> ImportId {
    ImportId::generate()
}

/// Helper to generate a test activity ID.
pub fn test_activity_id() -> ActivityId {
    ActivityId::generate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_builder_default_creates_requested_import() {
        let import = ImportBuilder::new().build();
        assert_eq!(import.status, ImportStatus::Requested);
        assert_eq!(import.format, ImportFormat::Gpx);
    }

    #[test]
    fn import_builder_with_status_uploaded() {
        let import = ImportBuilder::new()
            .with_status(ImportStatus::Uploaded)
            .build();
        assert_eq!(import.status, ImportStatus::Uploaded);
        assert!(import.checksum.is_some());
        assert!(import.source_artifact_id.is_some());
    }

    #[test]
    fn import_builder_with_status_queued() {
        let import = ImportBuilder::new()
            .with_status(ImportStatus::Queued)
            .build();
        assert_eq!(import.status, ImportStatus::Queued);
    }

    #[test]
    fn import_builder_with_status_completed() {
        let import = ImportBuilder::new()
            .with_status(ImportStatus::Completed)
            .build();
        assert_eq!(import.status, ImportStatus::Completed);
    }

    #[test]
    fn import_builder_with_status_failed() {
        let import = ImportBuilder::new()
            .with_status(ImportStatus::Failed)
            .build();
        assert_eq!(import.status, ImportStatus::Failed);
        assert!(import.failure_reason.is_some());
    }

    #[test]
    fn import_builder_with_custom_owner() {
        let owner = test_user_id();
        let import = ImportBuilder::new().with_owner(owner).build();
        assert_eq!(import.owner_id, owner);
    }

    #[test]
    fn import_builder_with_custom_checksum() {
        let checksum = "b".repeat(64);
        let import = ImportBuilder::new()
            .with_status(ImportStatus::Uploaded)
            .with_checksum(&checksum)
            .build();
        assert_eq!(import.checksum.unwrap().as_str(), checksum);
    }

    #[test]
    fn actor_builder_creates_valid_actor() {
        let actor = ActorBuilder::new().build();
        assert_ne!(actor.user_id.0, Uuid::nil());
    }

    #[test]
    fn actor_builder_with_custom_user_id() {
        let user_id = test_user_id();
        let actor = ActorBuilder::new().with_user_id(user_id).build();
        assert_eq!(actor.user_id, user_id);
    }

    #[test]
    fn activity_builder_creates_valid_activity() {
        let activity = ActivityBuilder::new().build();
        assert_eq!(activity.title.as_str(), "Test Activity");
        assert_eq!(activity.activity_type, ActivityType::Hike);
    }

    #[test]
    fn activity_builder_with_custom_fields() {
        let owner = test_user_id();
        let activity = ActivityBuilder::new()
            .with_owner(owner)
            .with_title("Evening Run")
            .with_type(ActivityType::Run)
            .build();
        assert_eq!(activity.owner_id, owner);
        assert_eq!(activity.title.as_str(), "Evening Run");
        assert_eq!(activity.activity_type, ActivityType::Run);
    }

    #[test]
    fn random_checksum_is_valid_hex() {
        let checksum = random_checksum();
        assert_eq!(checksum.len(), 64);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn export_job_builder_default_creates_queued_job() {
        use haiker_app::exports::state_machine::ExportStatus;

        let job = ExportJobBuilder::new().build();
        assert_eq!(job.status, ExportStatus::Queued);
        assert_eq!(job.format, haiker_app::exports::ExportFormat::Gpx);
    }

    #[test]
    fn export_job_builder_with_status_generating() {
        use haiker_app::exports::state_machine::ExportStatus;

        let job = ExportJobBuilder::new()
            .with_status(ExportStatus::Generating)
            .build();
        assert_eq!(job.status, ExportStatus::Generating);
    }

    #[test]
    fn export_job_builder_with_status_ready() {
        use haiker_app::exports::state_machine::ExportStatus;

        let job = ExportJobBuilder::new()
            .with_status(ExportStatus::Ready)
            .build();
        assert_eq!(job.status, ExportStatus::Ready);
        assert!(job.object_storage_key.is_some());
        assert!(job.checksum.is_some());
        assert!(job.expires_at.is_some());
    }

    #[test]
    fn export_job_builder_with_status_failed() {
        use haiker_app::exports::state_machine::ExportStatus;

        let job = ExportJobBuilder::new()
            .with_status(ExportStatus::Failed)
            .build();
        assert_eq!(job.status, ExportStatus::Failed);
        assert!(job.failure_reason.is_some());
    }

    #[test]
    fn export_job_builder_with_status_expired() {
        use haiker_app::exports::state_machine::ExportStatus;

        let job = ExportJobBuilder::new()
            .with_status(ExportStatus::Expired)
            .build();
        assert_eq!(job.status, ExportStatus::Expired);
    }

    #[test]
    fn export_job_builder_with_custom_owner() {
        let owner = test_user_id();
        let job = ExportJobBuilder::new().with_owner(owner).build();
        assert_eq!(job.requested_by, owner);
    }

    #[test]
    fn export_job_builder_with_custom_route_version_id() {
        let rv_id = RouteVersionId::new(Uuid::new_v4());
        let job = ExportJobBuilder::new().with_route_version_id(rv_id).build();
        assert_eq!(job.route_version_id, rv_id);
    }

    #[test]
    fn export_job_builder_with_custom_failure_reason() {
        use haiker_app::exports::state_machine::ExportStatus;

        let job = ExportJobBuilder::new()
            .with_status(ExportStatus::Failed)
            .with_failure_reason("custom failure")
            .build();
        assert_eq!(job.failure_reason.as_deref(), Some("custom failure"));
    }
}
