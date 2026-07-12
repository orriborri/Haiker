//! Import command types and handlers.
//!
//! Pure domain logic for initiating and managing imports. Accepts trait-based
//! interfaces for repository and storage, with no infrastructure dependencies.

use async_trait::async_trait;

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

    // Check idempotency key for replay
    if let Some(existing) = repo
        .find_by_idempotency_key(cmd.owner_id, &cmd.idempotency_key)
        .await?
    {
        // Return existing import (idempotent replay)
        let storage_key = format!("imports/{}/{}", cmd.owner_id, existing.id);
        let upload_url = url_generator.generate_upload_url(&storage_key).await?;
        return Ok(StartImportResult {
            import: existing,
            upload_url,
        });
    }

    // Create new import
    let mut import = Import::new(cmd.owner_id, ImportFormat::Gpx, cmd.idempotency_key)?;
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
/// Validates ownership, transitions the import to Uploaded status.
pub async fn handle_complete_upload(
    cmd: CompleteUploadCommand,
    repo: &dyn ImportRepository,
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
    async fn complete_upload_succeeds() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
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

        let import = handle_complete_upload(complete_cmd, &repo).await.unwrap();
        assert_eq!(import.status, ImportStatus::Uploaded);
    }

    #[tokio::test]
    async fn complete_upload_rejects_wrong_owner() {
        let repo = InMemoryRepo::new();
        let url_gen = FakeUrlGenerator;
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

        let err = handle_complete_upload(complete_cmd, &repo)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::Unauthorized));
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
}
