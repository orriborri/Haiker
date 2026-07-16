//! Route Exporting bounded context.
//!
//! Owns export requests, jobs, generated artifacts, download authorization,
//! and expiration.

pub mod commands;
pub mod job_types;
pub mod repository;
pub mod state_machine;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;
use crate::route_versioning::RouteVersionId;

use self::state_machine::ExportStatus;

// Re-export key types for consumers.
pub use self::commands::{handle_get_export, handle_request_export, RequestExportCommand};
pub use self::job_types::{GenerateExportJob, GENERATE_EXPORT_JOB_TYPE};
pub use self::repository::ExportRepository;
pub use self::state_machine::ExportStatus as Status;

/// A strongly-typed export job identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExportJobId(pub Uuid);

impl ExportJobId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for ExportJobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Supported file formats for export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// GPS Exchange Format.
    Gpx,
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportFormat::Gpx => write!(f, "gpx"),
        }
    }
}

/// Trait for verifying route version ownership.
///
/// This abstracts the cross-context lookup so domain code does not
/// depend on infrastructure or other bounded contexts directly.
#[async_trait]
pub trait RouteVersionGateway: Send + Sync {
    /// Verify that the route version belongs to the activity and the user owns
    /// the activity.
    ///
    /// Returns `Ok(())` if the verification passes, or an appropriate
    /// `ExportError` if it fails.
    async fn verify_owned_route_version(
        &self,
        activity_id: ActivityId,
        route_version_id: RouteVersionId,
        owner_id: UserId,
    ) -> Result<(), ExportError>;
}

/// The ExportJob aggregate representing an export job through its lifecycle.
#[derive(Debug, Clone)]
pub struct ExportJob {
    pub id: ExportJobId,
    pub activity_id: ActivityId,
    pub route_version_id: RouteVersionId,
    pub requested_by: UserId,
    pub format: ExportFormat,
    pub status: ExportStatus,
    pub idempotency_key: String,
    /// SHA-256 hash of the canonical request payload (activity_id, route_version_id, format)
    /// used for idempotency payload mismatch detection.
    pub payload_hash: Option<String>,
    /// The object storage key where the generated export file is stored.
    pub object_storage_key: Option<String>,
    /// Checksum of the generated export file.
    pub checksum: Option<String>,
    /// Reason for failure, if the export failed.
    pub failure_reason: Option<String>,
    /// When the export file expires and is no longer available for download.
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ExportJob {
    /// Create a new export job in the Queued state.
    pub fn new(
        requested_by: UserId,
        activity_id: ActivityId,
        route_version_id: RouteVersionId,
        format: ExportFormat,
        idempotency_key: String,
        payload_hash: Option<String>,
    ) -> Result<Self, ExportError> {
        if idempotency_key.trim().is_empty() {
            return Err(ExportError::ValidationFailed {
                message: "idempotency key must not be empty".to_string(),
            });
        }

        let now = Utc::now();
        Ok(Self {
            id: ExportJobId::generate(),
            activity_id,
            route_version_id,
            requested_by,
            format,
            status: ExportStatus::Queued,
            idempotency_key,
            payload_hash,
            object_storage_key: None,
            checksum: None,
            failure_reason: None,
            expires_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Transition to Generating state (worker has picked up the job).
    pub fn start_generating(&mut self) -> Result<(), ExportError> {
        self.status = self.status.transition_to(ExportStatus::Generating)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Ready state after successful generation.
    pub fn complete(
        &mut self,
        object_storage_key: String,
        checksum: String,
        expires_at: DateTime<Utc>,
    ) -> Result<(), ExportError> {
        self.status = self.status.transition_to(ExportStatus::Ready)?;
        self.object_storage_key = Some(object_storage_key);
        self.checksum = Some(checksum);
        self.expires_at = Some(expires_at);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Failed state with a reason.
    pub fn fail(&mut self, reason: String) -> Result<(), ExportError> {
        self.status = self.status.transition_to(ExportStatus::Failed)?;
        self.failure_reason = Some(reason);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Expired state.
    pub fn expire(&mut self) -> Result<(), ExportError> {
        self.status = self.status.transition_to(ExportStatus::Expired)?;
        self.updated_at = Utc::now();
        Ok(())
    }
}

/// Errors that can occur in the exports context.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ExportError {
    /// The requested export was not found.
    #[error("export not found")]
    NotFound,

    /// The user is not authorized to access this export.
    #[error("unauthorized")]
    Unauthorized,

    /// The state transition is not valid.
    #[error("invalid transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },

    /// An export with this idempotency key already exists with a different payload.
    #[error("idempotency key reused with different payload")]
    IdempotencyPayloadMismatch,

    /// The referenced route version was not found.
    #[error("route version not found")]
    RouteVersionNotFound,

    /// The referenced activity was not found.
    #[error("activity not found")]
    ActivityNotFound,

    /// A validation error occurred.
    #[error("validation failed: {message}")]
    ValidationFailed { message: String },

    /// A persistence error occurred.
    #[error("persistence error: {message}")]
    PersistenceError { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_job_creation_succeeds_with_valid_inputs() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let export_job = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "key-123".to_string(),
            None,
        )
        .unwrap();

        assert_eq!(export_job.requested_by, owner);
        assert_eq!(export_job.activity_id, activity_id);
        assert_eq!(export_job.route_version_id, route_version_id);
        assert_eq!(export_job.format, ExportFormat::Gpx);
        assert_eq!(export_job.status, ExportStatus::Queued);
        assert_eq!(export_job.idempotency_key, "key-123");
        assert!(export_job.payload_hash.is_none());
        assert!(export_job.object_storage_key.is_none());
        assert!(export_job.checksum.is_none());
        assert!(export_job.failure_reason.is_none());
        assert!(export_job.expires_at.is_none());
    }

    #[test]
    fn export_job_creation_fails_with_empty_idempotency_key() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let result = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "".to_string(),
            None,
        );
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExportError::ValidationFailed { .. }
        ));
    }

    #[test]
    fn export_job_creation_fails_with_whitespace_idempotency_key() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let result = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "   ".to_string(),
            None,
        );
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExportError::ValidationFailed { .. }
        ));
    }

    #[test]
    fn export_job_happy_path_lifecycle() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "key-lifecycle".to_string(),
            None,
        )
        .unwrap();

        assert_eq!(export_job.status, ExportStatus::Queued);

        export_job.start_generating().unwrap();
        assert_eq!(export_job.status, ExportStatus::Generating);

        let expires = Utc::now() + chrono::Duration::hours(24);
        export_job
            .complete(
                "exports/user-1/export-1.gpx".to_string(),
                "abc123".to_string(),
                expires,
            )
            .unwrap();
        assert_eq!(export_job.status, ExportStatus::Ready);
        assert_eq!(
            export_job.object_storage_key.as_deref(),
            Some("exports/user-1/export-1.gpx")
        );
        assert_eq!(export_job.checksum.as_deref(), Some("abc123"));
        assert_eq!(export_job.expires_at, Some(expires));

        export_job.expire().unwrap();
        assert_eq!(export_job.status, ExportStatus::Expired);
    }

    #[test]
    fn export_job_can_fail_from_queued() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "key-fail-q".to_string(),
            None,
        )
        .unwrap();

        export_job.fail("worker unavailable".to_string()).unwrap();
        assert_eq!(export_job.status, ExportStatus::Failed);
        assert_eq!(
            export_job.failure_reason.as_deref(),
            Some("worker unavailable")
        );
    }

    #[test]
    fn export_job_can_fail_from_generating() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "key-fail-g".to_string(),
            None,
        )
        .unwrap();

        export_job.start_generating().unwrap();
        export_job.fail("generation error".to_string()).unwrap();
        assert_eq!(export_job.status, ExportStatus::Failed);
        assert_eq!(
            export_job.failure_reason.as_deref(),
            Some("generation error")
        );
    }

    #[test]
    fn export_job_cannot_transition_from_failed() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "key-term-f".to_string(),
            None,
        )
        .unwrap();

        export_job.fail("reason".to_string()).unwrap();

        assert!(export_job.start_generating().is_err());
        assert!(export_job.fail("again".to_string()).is_err());
    }

    #[test]
    fn export_job_cannot_transition_from_expired() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "key-term-e".to_string(),
            None,
        )
        .unwrap();

        export_job.start_generating().unwrap();
        let expires = Utc::now() + chrono::Duration::hours(24);
        export_job
            .complete("key".to_string(), "hash".to_string(), expires)
            .unwrap();
        export_job.expire().unwrap();

        assert!(export_job.start_generating().is_err());
        assert!(export_job.fail("x".to_string()).is_err());
        assert!(export_job.expire().is_err());
    }

    #[test]
    fn export_job_queued_cannot_skip_to_ready() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "key-skip".to_string(),
            None,
        )
        .unwrap();

        let expires = Utc::now() + chrono::Duration::hours(24);
        let result = export_job.complete("key".to_string(), "hash".to_string(), expires);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExportError::InvalidTransition { .. }
        ));
    }

    #[test]
    fn export_format_display() {
        assert_eq!(ExportFormat::Gpx.to_string(), "gpx");
    }

    #[test]
    fn export_job_id_display() {
        let id = Uuid::new_v4();
        let export_job_id = ExportJobId::new(id);
        assert_eq!(export_job_id.to_string(), id.to_string());
    }

    #[test]
    fn error_display() {
        let err = ExportError::NotFound;
        assert_eq!(err.to_string(), "export not found");

        let err = ExportError::Unauthorized;
        assert_eq!(err.to_string(), "unauthorized");

        let err = ExportError::InvalidTransition {
            from: "queued".to_string(),
            to: "ready".to_string(),
        };
        assert_eq!(err.to_string(), "invalid transition from queued to ready");

        let err = ExportError::IdempotencyPayloadMismatch;
        assert_eq!(
            err.to_string(),
            "idempotency key reused with different payload"
        );

        let err = ExportError::RouteVersionNotFound;
        assert_eq!(err.to_string(), "route version not found");

        let err = ExportError::ActivityNotFound;
        assert_eq!(err.to_string(), "activity not found");

        let err = ExportError::ValidationFailed {
            message: "bad input".to_string(),
        };
        assert_eq!(err.to_string(), "validation failed: bad input");

        let err = ExportError::PersistenceError {
            message: "db down".to_string(),
        };
        assert_eq!(err.to_string(), "persistence error: db down");
    }
}
