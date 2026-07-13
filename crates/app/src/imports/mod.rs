//! Activity Importing bounded context.
//!
//! Owns upload, import state machine, validation, parsing orchestration,
//! and duplicate detection.

pub mod checksum;
pub mod commands;
pub mod commit;
pub mod duplicate_detection;
pub mod gpx_parser;
pub mod job_types;
pub mod orchestrator;
pub mod repository;
pub mod state_machine;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::identity::UserId;
use crate::recorded_activity::SourceArtifactId;

use self::checksum::Checksum;
use self::state_machine::ImportStatus;

// Re-export key types for consumers.
pub use self::checksum::Checksum as ImportChecksum;
pub use self::commit::{CommitImport, ImportCommitData};
pub use self::duplicate_detection::{CheckDuplicate, DuplicateCheckResult};
pub use self::gpx_parser::{
    parse_gpx, GpxMetadata, GpxParseError, GpxParseErrorCode, GpxParseResult, GpxTrack,
    GpxTrackPoint, GpxTrackSegment, GpxVersion,
};
pub use self::orchestrator::{ImportOrchestrator, ImportProcessingResult, ObjectStore};
pub use self::repository::ImportRepository;
pub use self::state_machine::ImportStatus as Status;

/// A strongly-typed import identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImportId(pub Uuid);

impl ImportId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for ImportId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Supported file formats for import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    /// GPS Exchange Format.
    Gpx,
}

impl std::fmt::Display for ImportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportFormat::Gpx => write!(f, "gpx"),
        }
    }
}

/// The Import aggregate representing an import job through its lifecycle.
#[derive(Debug, Clone)]
pub struct Import {
    pub id: ImportId,
    pub owner_id: UserId,
    pub source_artifact_id: Option<SourceArtifactId>,
    pub format: ImportFormat,
    pub status: ImportStatus,
    pub checksum: Option<Checksum>,
    pub failure_reason: Option<String>,
    pub idempotency_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Import {
    /// Create a new import in the Requested state.
    pub fn new(
        owner_id: UserId,
        format: ImportFormat,
        idempotency_key: String,
    ) -> Result<Self, ImportError> {
        if idempotency_key.trim().is_empty() {
            return Err(ImportError::ValidationFailed {
                message: "idempotency key must not be empty".to_string(),
            });
        }

        let now = Utc::now();
        Ok(Self {
            id: ImportId::generate(),
            owner_id,
            source_artifact_id: None,
            format,
            status: ImportStatus::Requested,
            checksum: None,
            failure_reason: None,
            idempotency_key,
            created_at: now,
            updated_at: now,
        })
    }

    /// Transition to Uploading state.
    pub fn start_upload(&mut self) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Uploading)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Uploaded state after successful upload.
    pub fn complete_upload(
        &mut self,
        source_artifact_id: SourceArtifactId,
        checksum: Checksum,
    ) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Uploaded)?;
        self.source_artifact_id = Some(source_artifact_id);
        self.checksum = Some(checksum);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Validating state.
    pub fn start_validation(&mut self) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Validating)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Queued state after validation passes.
    pub fn queue_for_parsing(&mut self) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Queued)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Parsing state when worker picks up the job.
    pub fn start_parsing(&mut self) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Parsing)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Committing state when parsing succeeds.
    pub fn start_committing(&mut self) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Committing)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Completed state.
    pub fn complete(&mut self) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Completed)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Failed state with a reason.
    pub fn fail(&mut self, reason: String) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Failed)?;
        self.failure_reason = Some(reason);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition to Cancelled state.
    pub fn cancel(&mut self) -> Result<(), ImportError> {
        self.status = self.status.transition_to(ImportStatus::Cancelled)?;
        self.updated_at = Utc::now();
        Ok(())
    }
}

/// Errors that can occur in the imports context.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImportError {
    /// The file format is not supported for import.
    #[error("unsupported file format")]
    UnsupportedFormat,

    /// The state transition is not valid.
    #[error("invalid transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },

    /// The requested import was not found.
    #[error("import not found")]
    NotFound,

    /// The user is not authorized to access this import.
    #[error("unauthorized")]
    Unauthorized,

    /// An import with this idempotency key already exists.
    #[error("duplicate idempotency key")]
    DuplicateIdempotencyKey,

    /// An import with this checksum already exists for this user.
    #[error("duplicate checksum")]
    DuplicateChecksum,

    /// The uploaded file checksum does not match the expected checksum.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    /// The uploaded file exceeds the maximum size.
    #[error("upload too large")]
    UploadTooLarge,

    /// The uploaded file has an invalid media type.
    #[error("invalid media type")]
    InvalidMediaType,

    /// Parsing of the import file failed.
    #[error("parsing failed: {reason}")]
    ParsingFailed { reason: String },

    /// An error occurred during object storage operations.
    #[error("storage error: {message}")]
    StorageError { message: String },

    /// A validation error occurred.
    #[error("validation failed: {message}")]
    ValidationFailed { message: String },

    /// An idempotency key was reused with a different payload.
    #[error("idempotency key reused with different payload")]
    IdempotencyPayloadMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_creation_succeeds_with_valid_inputs() {
        let owner_id = UserId::new(Uuid::new_v4());
        let import = Import::new(owner_id, ImportFormat::Gpx, "key-123".to_string()).unwrap();

        assert_eq!(import.owner_id, owner_id);
        assert_eq!(import.format, ImportFormat::Gpx);
        assert_eq!(import.status, ImportStatus::Requested);
        assert_eq!(import.idempotency_key, "key-123");
        assert!(import.source_artifact_id.is_none());
        assert!(import.checksum.is_none());
        assert!(import.failure_reason.is_none());
    }

    #[test]
    fn import_creation_fails_with_empty_idempotency_key() {
        let owner_id = UserId::new(Uuid::new_v4());
        let result = Import::new(owner_id, ImportFormat::Gpx, "".to_string());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImportError::ValidationFailed { .. }
        ));
    }

    #[test]
    fn import_creation_fails_with_whitespace_idempotency_key() {
        let owner_id = UserId::new(Uuid::new_v4());
        let result = Import::new(owner_id, ImportFormat::Gpx, "   ".to_string());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ImportError::ValidationFailed { .. }
        ));
    }

    #[test]
    fn import_happy_path_lifecycle() {
        let owner_id = UserId::new(Uuid::new_v4());
        let mut import = Import::new(owner_id, ImportFormat::Gpx, "key-1".to_string()).unwrap();

        assert_eq!(import.status, ImportStatus::Requested);

        import.start_upload().unwrap();
        assert_eq!(import.status, ImportStatus::Uploading);

        let artifact_id = SourceArtifactId::new(Uuid::new_v4());
        let checksum = Checksum::new("a".repeat(64)).unwrap();
        import.complete_upload(artifact_id, checksum).unwrap();
        assert_eq!(import.status, ImportStatus::Uploaded);
        assert_eq!(import.source_artifact_id, Some(artifact_id));

        import.start_validation().unwrap();
        assert_eq!(import.status, ImportStatus::Validating);

        import.queue_for_parsing().unwrap();
        assert_eq!(import.status, ImportStatus::Queued);

        import.start_parsing().unwrap();
        assert_eq!(import.status, ImportStatus::Parsing);

        import.start_committing().unwrap();
        assert_eq!(import.status, ImportStatus::Committing);

        import.complete().unwrap();
        assert_eq!(import.status, ImportStatus::Completed);
    }

    #[test]
    fn import_can_fail_from_any_non_terminal_state() {
        let non_terminal_states = [
            ImportStatus::Requested,
            ImportStatus::Uploading,
            ImportStatus::Uploaded,
            ImportStatus::Validating,
            ImportStatus::Queued,
            ImportStatus::Parsing,
            ImportStatus::Committing,
        ];

        for status in non_terminal_states {
            let owner_id = UserId::new(Uuid::new_v4());
            let mut import = Import::new(owner_id, ImportFormat::Gpx, "key-1".to_string()).unwrap();
            import.status = status;

            let result = import.fail("something went wrong".to_string());
            assert!(result.is_ok(), "Should be able to fail from {status}");
            assert_eq!(import.status, ImportStatus::Failed);
            assert_eq!(
                import.failure_reason.as_deref(),
                Some("something went wrong")
            );
        }
    }

    #[test]
    fn import_can_cancel_from_any_non_terminal_state() {
        let non_terminal_states = [
            ImportStatus::Requested,
            ImportStatus::Uploading,
            ImportStatus::Uploaded,
            ImportStatus::Validating,
            ImportStatus::Queued,
            ImportStatus::Parsing,
            ImportStatus::Committing,
        ];

        for status in non_terminal_states {
            let owner_id = UserId::new(Uuid::new_v4());
            let mut import = Import::new(owner_id, ImportFormat::Gpx, "key-1".to_string()).unwrap();
            import.status = status;

            let result = import.cancel();
            assert!(result.is_ok(), "Should be able to cancel from {status}");
            assert_eq!(import.status, ImportStatus::Cancelled);
        }
    }

    #[test]
    fn import_cannot_transition_from_terminal_states() {
        let terminal_states = [
            ImportStatus::Completed,
            ImportStatus::Failed,
            ImportStatus::Cancelled,
        ];

        for status in terminal_states {
            let owner_id = UserId::new(Uuid::new_v4());
            let mut import = Import::new(owner_id, ImportFormat::Gpx, "key-1".to_string()).unwrap();
            import.status = status;

            assert!(import.start_upload().is_err());
            assert!(import.fail("x".to_string()).is_err());
            assert!(import.cancel().is_err());
        }
    }

    #[test]
    fn import_format_display() {
        assert_eq!(ImportFormat::Gpx.to_string(), "gpx");
    }

    #[test]
    fn import_id_display() {
        let id = Uuid::new_v4();
        let import_id = ImportId::new(id);
        assert_eq!(import_id.to_string(), id.to_string());
    }

    #[test]
    fn error_display() {
        let err = ImportError::UnsupportedFormat;
        assert_eq!(err.to_string(), "unsupported file format");

        let err = ImportError::InvalidTransition {
            from: "requested".to_string(),
            to: "parsing".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "invalid transition from requested to parsing"
        );

        let err = ImportError::ChecksumMismatch {
            expected: "abc".to_string(),
            actual: "def".to_string(),
        };
        assert_eq!(err.to_string(), "checksum mismatch: expected abc, got def");
    }
}
