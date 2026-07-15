//! Import command types and handlers.
//!
//! Pure domain logic for initiating and managing imports. Accepts trait-based
//! interfaces for repository and storage, with no infrastructure dependencies.

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::identity::UserId;

use super::checksum::Checksum;
use super::repository::ImportRepository;
use super::state_machine::ImportStatus;
use super::{Import, ImportError, ImportFormat, ImportId};

/// Maximum allowed file size for upload: 50 MB.
pub const MAX_FILE_SIZE_BYTES: u64 = 50 * 1024 * 1024;

/// Allowed content types for GPX imports.
const ALLOWED_CONTENT_TYPES: &[&str] = &["application/gpx+xml", "application/xml"];

/// Command to start a new import.
#[derive(Debug, Clone)]
pub struct StartImportCommand {
    pub owner_id: UserId,
    pub idempotency_key: String,
    pub filename: String,
    pub content_type: String,
    pub file_size_bytes: u64,
}

/// Result of a successful start import operation.
#[derive(Debug, Clone)]
pub struct StartImportResult {
    pub import: Import,
    pub upload_url: String,
}

/// Command to finalize upload (mark as uploaded).
#[derive(Debug, Clone)]
pub struct CompleteUploadCommand {
    pub import_id: ImportId,
    pub owner_id: UserId,
    pub checksum: String,
}

/// Trait for generating presigned upload URLs.
///
/// This abstracts the object storage concern so domain code
/// does not depend on infrastructure.
#[async_trait]
pub trait UploadUrlGenerator: Send + Sync {
    /// Generate a presigned upload URL for the given storage key.
    async fn generate_upload_url(&self, key: &str) -> Result<String, ImportError>;
}

/// Metadata about an uploaded object retrieved from storage.
#[derive(Debug, Clone)]
pub struct UploadMetadata {
    /// Size of the uploaded object in bytes.
    pub content_length: u64,
    /// Content-Type of the uploaded object, if available.
    pub content_type: Option<String>,
}

/// Trait for verifying uploaded object metadata in storage.
///
/// This abstracts the storage verification concern so domain code
/// does not depend on infrastructure.
#[async_trait]
pub trait UploadVerifier: Send + Sync {
    /// Verify that an uploaded object exists and return its metadata.
    ///
    /// Returns `Err(ImportError::ObjectNotFound)` if the object does not exist.
    async fn verify_upload(&self, key: &str) -> Result<UploadMetadata, ImportError>;
}

/// Compute a deterministic SHA-256 hash of the import payload fields.
///
/// Hashes the canonical representation of (filename, content_type, file_size_bytes)
/// to produce a stable fingerprint for idempotency payload comparison.
pub fn compute_payload_hash(filename: &str, content_type: &str, file_size_bytes: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(filename.as_bytes());
    hasher.update(b"\x00");
    hasher.update(content_type.as_bytes());
    hasher.update(b"\x00");
    hasher.update(file_size_bytes.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

/// Handle the start import command.
///
/// Validates inputs, checks idempotency, creates the import aggregate,
/// and generates a presigned upload URL.
pub async fn handle_start_import(
    cmd: StartImportCommand,
    repo: &dyn ImportRepository,
    url_generator: &dyn UploadUrlGenerator,
) -> Result<StartImportResult, ImportError> {
    // Validate file size
    if cmd.file_size_bytes > MAX_FILE_SIZE_BYTES {
        return Err(ImportError::UploadTooLarge);
    }

    // Validate content type
    if !ALLOWED_CONTENT_TYPES.contains(&cmd.content_type.as_str()) {
        return Err(ImportError::InvalidMediaType);
    }

    // Validate filename is not empty
    if cmd.filename.trim().is_empty() {
        return Err(ImportError::ValidationFailed {
            message: "filename must not be empty".to_string(),
        });
    }

    // Compute payload hash for idempotency comparison
    let payload_hash = compute_payload_hash(&cmd.filename, &cmd.content_type, cmd.file_size_bytes);

    // Check idempotency key for replay
    if let Some(existing) = repo
        .find_by_idempotency_key(cmd.owner_id, &cmd.idempotency_key)
        .await?
    {
        // Verify payload matches the original request
        if let Some(ref stored_hash) = existing.payload_hash {
            if *stored_hash != payload_hash {
                return Err(ImportError::IdempotencyPayloadMismatch);
            }
        }

        // Return existing import (idempotent replay)
        let storage_key = format!("imports/{}/{}", cmd.owner_id, existing.id);
        let upload_url = url_generator.generate_upload_url(&storage_key).await?;
        return Ok(StartImportResult {
            import: existing,
            upload_url,
        });
    }

    // Create new import
    let mut import = Import::new(
        cmd.owner_id,
        ImportFormat::Gpx,
        cmd.idempotency_key,
        Some(payload_hash),
    )?;
    import.start_upload()?;

    // Generate presigned upload URL
    let storage_key = format!("imports/{}/{}", cmd.owner_id, import.id);
    let upload_url = url_generator.generate_upload_url(&storage_key).await?;

    // Persist
    repo.save(&import).await?;

    Ok(StartImportResult { import, upload_url })
}

/// Handle the complete upload command.
///
/// Validates ownership, verifies the uploaded object in storage,
/// and transitions the import to Uploaded status.
pub async fn handle_complete_upload(
    cmd: CompleteUploadCommand,
    repo: &dyn ImportRepository,
    upload_verifier: &dyn UploadVerifier,
) -> Result<Import, ImportError> {
    // Validate checksum format
    let checksum = Checksum::new(&cmd.checksum)?;

    // Find the import
    let mut import = repo
        .find_by_id(cmd.import_id)
        .await?
        .ok_or(ImportError::NotFound)?;

    // Verify ownership
    if import.owner_id != cmd.owner_id {
        return Err(ImportError::Unauthorized);
    }

    // Verify import is in correct state
    if import.status != ImportStatus::Uploading {
        return Err(ImportError::InvalidTransition {
            from: import.status.to_string(),
            to: "uploaded".to_string(),
        });
    }

    // Verify the uploaded object exists and check its metadata
    let storage_key = format!("imports/{}/{}", cmd.owner_id, cmd.import_id);
    let metadata = match upload_verifier.verify_upload(&storage_key).await {
        Ok(meta) => meta,
        Err(ImportError::StorageError { ref message }) => {
            // Transient error: leave import in Uploading state for retry
            tracing::warn!(
                error = %message,
                storage_key = %storage_key,
                "transient storage error during upload verification"
            );
            return Err(ImportError::StorageError {
                message: "storage temporarily unavailable".to_string(),
            });
        }
        Err(ImportError::ObjectNotFound) => {
            // Definitive: object missing, transition to Failed
            import
                .fail("upload not found in storage".to_string())
                .map_err(|_| ImportError::ObjectNotFound)?;
            repo.update(&import).await?;
            return Err(ImportError::ObjectNotFound);
        }
        Err(other) => {
            // Any other error from verifier is treated as definitive
            import.fail(other.to_string()).map_err(|_| other.clone())?;
            repo.update(&import).await?;
            return Err(other);
        }
    };

    // Verify file size is within the limit
    if metadata.content_length > MAX_FILE_SIZE_BYTES {
        import
            .fail("upload too large".to_string())
            .map_err(|_| ImportError::UploadTooLarge)?;
        repo.update(&import).await?;
        return Err(ImportError::UploadTooLarge);
    }

    // Reject zero-byte uploads
    if metadata.content_length == 0 {
        import
            .fail("upload is empty".to_string())
            .map_err(|_| ImportError::ValidationFailed {
                message: "upload is empty".to_string(),
            })?;
        repo.update(&import).await?;
        return Err(ImportError::ValidationFailed {
            message: "upload is empty".to_string(),
        });
    }

    // Verify content type is allowed (if present)
    if let Some(ref content_type) = metadata.content_type {
        if !ALLOWED_CONTENT_TYPES.contains(&content_type.as_str()) {
            import
                .fail("invalid media type".to_string())
                .map_err(|_| ImportError::InvalidMediaType)?;
            repo.update(&import).await?;
            return Err(ImportError::InvalidMediaType);
        }
    }

    // Transition to uploaded
    let artifact_id = crate::recorded_activity::SourceArtifactId::generate();
    import.complete_upload(artifact_id, checksum)?;

    // Persist
    repo.update(&import).await?;

    Ok(import)
}

/// Handle get import status.
///
/// Retrieves import by ID, verifying ownership.
pub async fn handle_get_import(
    import_id: ImportId,
    owner_id: UserId,
    repo: &dyn ImportRepository,
) -> Result<Import, ImportError> {
    let import = repo
        .find_by_id(import_id)
        .await?
        .ok_or(ImportError::NotFound)?;

    if import.owner_id != owner_id {
        return Err(ImportError::NotFound);
    }

    Ok(import)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::UserId;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    /// In-memory implementation of ImportRepository for tests.
    struct InMemoryRepo {
        imports: Mutex<HashMap<ImportId, Import>>,
    }

    impl InMemoryRepo {
        fn new() -> Self {
            Self {
                imports: Mutex::new(HashMap::new()),
            }
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
            owner_id: UserId,
            key: &str,
        ) -> Result<Option<Import>, ImportError> {
            Ok(self
                .imports
                .lock()
                .unwrap()
                .values()
                .find(|i| i.owner_id == owner_id && i.idempotency_key == key)
                .cloned())
        }

        async fn find_by_checksum(
            &self,
            owner_id: UserId,
            checksum: &Checksum,
        ) -> Result<Option<Import>, ImportError> {
            Ok(self
                .imports
                .lock()
                .unwrap()
                .values()
                .find(|i| i.owner_id == owner_id && i.checksum.as_ref() == Some(checksum))
                .cloned())
        }

        async fn find_completed_by_checksum(
            &self,
            _owner_id: UserId,
            _checksum: &Checksum,
        ) -> Result<Option<(ImportId, Option<crate::activity_catalog::ActivityId>)>, ImportError>
        {
            Ok(None)
        }

        async fn update(&self, import: &Import) -> Result<(), ImportError> {
            self.imports
                .lock()
                .unwrap()
                .insert(import.id, import.clone());
            Ok(())
        }
    }

    struct FakeUrlGenerator;

    #[async_trait]
    impl UploadUrlGenerator for FakeUrlGenerator {
        async fn generate_upload_url(&self, key: &str) -> Result<String, ImportError> {
            Ok(format!("https://storage.example.com/{key}?signed=true"))
        }
    }

    /// Configurable fake upload verifier for testing.
    struct FakeUploadVerifier {
        result: Mutex<Result<UploadMetadata, ImportError>>,
    }

    impl FakeUploadVerifier {
        fn success(content_length: u64, content_type: Option<&str>) -> Self {
            Self {
                result: Mutex::new(Ok(UploadMetadata {
                    content_length,
                    content_type: content_type.map(|s| s.to_string()),
                })),
            }
        }

        fn failure(err: ImportError) -> Self {
            Self {
                result: Mutex::new(Err(err)),
            }
        }
    }

    #[async_trait]
    impl UploadVerifier for FakeUploadVerifier {
        async fn verify_upload(&self, _key: &str) -> Result<UploadMetadata, ImportError> {
            self.result.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn start_import_succeeds() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;

        let cmd = StartImportCommand {
            owner_id: UserId::new(Uuid::new_v4()),
            idempotency_key: "idem-1".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        assert_eq!(result.import.status, ImportStatus::Uploading);
        assert!(result.upload_url.contains("signed=true"));
    }

    #[tokio::test]
    async fn start_import_rejects_too_large() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;

        let cmd = StartImportCommand {
            owner_id: UserId::new(Uuid::new_v4()),
            idempotency_key: "idem-1".to_string(),
            filename: "big.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: MAX_FILE_SIZE_BYTES + 1,
        };

        let err = handle_start_import(cmd, &repo, &url_gen).await.unwrap_err();
        assert!(matches!(err, ImportError::UploadTooLarge));
    }

    #[tokio::test]
    async fn start_import_rejects_invalid_content_type() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;

        let cmd = StartImportCommand {
            owner_id: UserId::new(Uuid::new_v4()),
            idempotency_key: "idem-1".to_string(),
            filename: "data.json".to_string(),
            content_type: "application/json".to_string(),
            file_size_bytes: 1024,
        };

        let err = handle_start_import(cmd, &repo, &url_gen).await.unwrap_err();
        assert!(matches!(err, ImportError::InvalidMediaType));
    }

    #[tokio::test]
    async fn start_import_idempotency_returns_existing() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-dup".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let first = handle_start_import(cmd.clone(), &repo, &url_gen)
            .await
            .unwrap();
        let second = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        assert_eq!(first.import.id, second.import.id);
    }

    #[tokio::test]
    async fn start_import_same_key_different_filename_returns_payload_mismatch() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let owner = UserId::new(Uuid::new_v4());

        let cmd1 = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-mismatch-fn".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        handle_start_import(cmd1, &repo, &url_gen).await.unwrap();

        let cmd2 = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-mismatch-fn".to_string(),
            filename: "different.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let err = handle_start_import(cmd2, &repo, &url_gen)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::IdempotencyPayloadMismatch));
    }

    #[tokio::test]
    async fn start_import_same_key_different_file_size_returns_payload_mismatch() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let owner = UserId::new(Uuid::new_v4());

        let cmd1 = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-mismatch-sz".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        handle_start_import(cmd1, &repo, &url_gen).await.unwrap();

        let cmd2 = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-mismatch-sz".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 2048,
        };

        let err = handle_start_import(cmd2, &repo, &url_gen)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::IdempotencyPayloadMismatch));
    }

    #[tokio::test]
    async fn start_import_same_key_different_content_type_returns_payload_mismatch() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let owner = UserId::new(Uuid::new_v4());

        let cmd1 = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-mismatch-ct".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        handle_start_import(cmd1, &repo, &url_gen).await.unwrap();

        let cmd2 = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-mismatch-ct".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/xml".to_string(),
            file_size_bytes: 1024,
        };

        let err = handle_start_import(cmd2, &repo, &url_gen)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::IdempotencyPayloadMismatch));
    }

    #[tokio::test]
    async fn start_import_same_key_same_payload_replays_original() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-replay".to_string(),
            filename: "trail.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 4096,
        };

        let first = handle_start_import(cmd.clone(), &repo, &url_gen)
            .await
            .unwrap();
        let second = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        assert_eq!(first.import.id, second.import.id);
        assert_eq!(first.import.status, second.import.status);
    }

    #[tokio::test]
    async fn compute_payload_hash_is_deterministic() {
        let hash1 = compute_payload_hash("file.gpx", "application/gpx+xml", 1024);
        let hash2 = compute_payload_hash("file.gpx", "application/gpx+xml", 1024);
        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn compute_payload_hash_differs_for_different_inputs() {
        let hash1 = compute_payload_hash("file.gpx", "application/gpx+xml", 1024);
        let hash2 = compute_payload_hash("other.gpx", "application/gpx+xml", 1024);
        assert_ne!(hash1, hash2);
    }

    #[tokio::test]
    async fn complete_upload_succeeds() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::success(1024, Some("application/gpx+xml"));
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-c".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        let complete_cmd = CompleteUploadCommand {
            import_id: start_result.import.id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let import = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap();
        assert_eq!(import.status, ImportStatus::Uploaded);
    }

    #[tokio::test]
    async fn complete_upload_rejects_wrong_owner() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::success(1024, Some("application/gpx+xml"));
        let owner = UserId::new(Uuid::new_v4());
        let other = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-o".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        let complete_cmd = CompleteUploadCommand {
            import_id: start_result.import.id,
            owner_id: other,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::Unauthorized));
    }

    #[tokio::test]
    async fn complete_upload_fails_when_object_missing() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::failure(ImportError::ObjectNotFound);
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-missing".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        let complete_cmd = CompleteUploadCommand {
            import_id: start_result.import.id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::ObjectNotFound));
    }

    #[tokio::test]
    async fn complete_upload_fails_when_object_too_large() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier =
            FakeUploadVerifier::success(MAX_FILE_SIZE_BYTES + 1, Some("application/gpx+xml"));
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-large".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        let complete_cmd = CompleteUploadCommand {
            import_id: start_result.import.id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::UploadTooLarge));
    }

    #[tokio::test]
    async fn complete_upload_fails_when_content_type_invalid() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::success(1024, Some("text/plain"));
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-ct".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        let complete_cmd = CompleteUploadCommand {
            import_id: start_result.import.id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::InvalidMediaType));
    }

    #[tokio::test]
    async fn complete_upload_succeeds_when_content_type_absent() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::success(1024, None);
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-no-ct".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        let complete_cmd = CompleteUploadCommand {
            import_id: start_result.import.id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let import = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap();
        assert_eq!(import.status, ImportStatus::Uploaded);
    }

    #[tokio::test]
    async fn get_import_returns_not_found_for_wrong_owner() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let owner = UserId::new(Uuid::new_v4());
        let other = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-g".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();

        let err = handle_get_import(start_result.import.id, other, &repo)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::NotFound));
    }

    #[tokio::test]
    async fn start_import_rejects_empty_filename() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;

        let cmd = StartImportCommand {
            owner_id: UserId::new(Uuid::new_v4()),
            idempotency_key: "idem-f".to_string(),
            filename: "  ".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let err = handle_start_import(cmd, &repo, &url_gen).await.unwrap_err();
        assert!(matches!(err, ImportError::ValidationFailed { .. }));
    }

    #[tokio::test]
    async fn start_import_accepts_application_xml() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;

        let cmd = StartImportCommand {
            owner_id: UserId::new(Uuid::new_v4()),
            idempotency_key: "idem-xml".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/xml".to_string(),
            file_size_bytes: 1024,
        };

        let result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        assert_eq!(result.import.status, ImportStatus::Uploading);
    }

    #[tokio::test]
    async fn complete_upload_storage_error_leaves_import_in_uploading() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::failure(ImportError::StorageError {
            message: "connection timeout".to_string(),
        });
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-transient".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        let import_id = start_result.import.id;

        let complete_cmd = CompleteUploadCommand {
            import_id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::StorageError { .. }));

        // Import should still be in Uploading state (not transitioned)
        let persisted = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(persisted.status, ImportStatus::Uploading);
        assert!(persisted.failure_reason.is_none());
    }

    #[tokio::test]
    async fn complete_upload_object_not_found_transitions_to_failed() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::failure(ImportError::ObjectNotFound);
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-obj-missing".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        let import_id = start_result.import.id;

        let complete_cmd = CompleteUploadCommand {
            import_id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::ObjectNotFound));

        // Import should have transitioned to Failed with descriptive reason
        let persisted = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(persisted.status, ImportStatus::Failed);
        assert_eq!(
            persisted.failure_reason.as_deref(),
            Some("upload not found in storage")
        );
    }

    #[tokio::test]
    async fn complete_upload_oversized_object_transitions_to_failed() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier =
            FakeUploadVerifier::success(MAX_FILE_SIZE_BYTES + 1, Some("application/gpx+xml"));
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-oversized".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        let import_id = start_result.import.id;

        let complete_cmd = CompleteUploadCommand {
            import_id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::UploadTooLarge));

        // Import should have transitioned to Failed
        let persisted = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(persisted.status, ImportStatus::Failed);
        assert_eq!(
            persisted.failure_reason.as_deref(),
            Some("upload too large")
        );
    }

    #[tokio::test]
    async fn complete_upload_invalid_content_type_transitions_to_failed() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::success(1024, Some("text/plain"));
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-bad-ct".to_string(),
            filename: "hike.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        let import_id = start_result.import.id;

        let complete_cmd = CompleteUploadCommand {
            import_id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::InvalidMediaType));

        // Import should have transitioned to Failed
        let persisted = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(persisted.status, ImportStatus::Failed);
        assert_eq!(
            persisted.failure_reason.as_deref(),
            Some("invalid media type")
        );
    }

    #[tokio::test]
    async fn complete_upload_rejects_zero_byte_upload() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
        let verifier = FakeUploadVerifier::success(0, Some("application/gpx+xml"));
        let owner = UserId::new(Uuid::new_v4());

        let cmd = StartImportCommand {
            owner_id: owner,
            idempotency_key: "idem-zero".to_string(),
            filename: "empty.gpx".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
        };

        let start_result = handle_start_import(cmd, &repo, &url_gen).await.unwrap();
        let import_id = start_result.import.id;

        let complete_cmd = CompleteUploadCommand {
            import_id,
            owner_id: owner,
            checksum: "a".repeat(64),
        };

        let err = handle_complete_upload(complete_cmd, &repo, &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::ValidationFailed { .. }));

        // Import should have transitioned to Failed
        let persisted = repo
            .imports
            .lock()
            .unwrap()
            .get(&import_id)
            .unwrap()
            .clone();
        assert_eq!(persisted.status, ImportStatus::Failed);
        assert_eq!(persisted.failure_reason.as_deref(), Some("upload is empty"));
    }
}
