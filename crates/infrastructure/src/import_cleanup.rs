//! Platform-layer cleanup for abandoned imports.
//!
//! Orchestrates the detection and failure of imports that have been stuck in
//! processing states beyond the allowed timeout. Called periodically by the
//! worker's maintenance loop.

use chrono::Duration;
use tracing;

use haiker_app::imports::repository::ImportRepository;
use haiker_app::imports::{fail_abandoned_imports, ImportError};

/// Default timeout for abandoned import detection (30 minutes).
const ABANDONED_TIMEOUT_MINUTES: i64 = 30;

/// Scan for abandoned imports and transition them to Failed with a Timeout code.
///
/// Returns the number of imports that were cleaned up, or an error if the
/// repository query fails.
pub async fn cleanup_abandoned_imports(
    repository: &dyn ImportRepository,
) -> Result<usize, ImportError> {
    let timeout = Duration::minutes(ABANDONED_TIMEOUT_MINUTES);
    let mut abandoned = repository.find_abandoned(timeout).await?;

    if abandoned.is_empty() {
        return Ok(0);
    }

    let result = fail_abandoned_imports(&mut abandoned);

    // Persist each updated import
    for import in &abandoned {
        if import.status == haiker_app::imports::state_machine::ImportStatus::Failed {
            if let Err(e) = repository.update(import).await {
                tracing::warn!(
                    import_id = %import.id,
                    error = %e,
                    "Failed to persist abandoned import cleanup"
                );
            }
        }
    }

    if result.cleaned_up > 0 {
        tracing::info!(
            target: "metrics",
            count = result.cleaned_up,
            skipped = result.skipped,
            "Cleaned up abandoned imports"
        );
    }

    Ok(result.cleaned_up)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;
    use uuid::Uuid;

    use haiker_app::activity_catalog::ActivityId;
    use haiker_app::identity::UserId;
    use haiker_app::imports::checksum::Checksum;
    use haiker_app::imports::state_machine::ImportStatus;
    use haiker_app::imports::{Import, ImportFormat, ImportId};

    /// Mock repository for testing cleanup logic.
    struct MockImportRepository {
        imports: Mutex<Vec<Import>>,
    }

    impl MockImportRepository {
        fn new(imports: Vec<Import>) -> Self {
            Self {
                imports: Mutex::new(imports),
            }
        }

        fn get_imports(&self) -> Vec<Import> {
            self.imports.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ImportRepository for MockImportRepository {
        async fn save(&self, _import: &Import) -> Result<(), ImportError> {
            Ok(())
        }

        async fn find_by_id(&self, _id: ImportId) -> Result<Option<Import>, ImportError> {
            Ok(None)
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
            let mut imports = self.imports.lock().unwrap();
            if let Some(existing) = imports.iter_mut().find(|i| i.id == import.id) {
                existing.status = import.status;
                existing.failure_reason = import.failure_reason.clone();
                existing.failure_code = import.failure_code;
                existing.updated_at = import.updated_at;
            }
            Ok(())
        }

        async fn find_abandoned(&self, timeout: Duration) -> Result<Vec<Import>, ImportError> {
            let threshold = Utc::now() - timeout;
            let imports = self.imports.lock().unwrap();
            let abandoned: Vec<Import> = imports
                .iter()
                .filter(|i| {
                    matches!(
                        i.status,
                        ImportStatus::Validating
                            | ImportStatus::Queued
                            | ImportStatus::Parsing
                            | ImportStatus::Committing
                    ) && i.updated_at < threshold
                })
                .cloned()
                .collect();
            Ok(abandoned)
        }
    }

    fn make_stuck_import(status: ImportStatus, minutes_ago: i64) -> Import {
        let owner_id = UserId::new(Uuid::new_v4());
        let mut import = Import::new(
            owner_id,
            ImportFormat::Gpx,
            format!("key-{}", Uuid::new_v4()),
            None,
        )
        .unwrap();
        import.status = status;
        import.updated_at = Utc::now() - Duration::minutes(minutes_ago);
        import
    }

    #[tokio::test]
    async fn cleanup_finds_and_fails_stuck_imports() {
        let imports = vec![
            make_stuck_import(ImportStatus::Parsing, 60),
            make_stuck_import(ImportStatus::Committing, 45),
        ];
        let repo = MockImportRepository::new(imports);

        let cleaned = cleanup_abandoned_imports(&repo).await.unwrap();
        assert_eq!(cleaned, 2);

        let stored = repo.get_imports();
        for import in &stored {
            assert_eq!(import.status, ImportStatus::Failed);
            assert_eq!(
                import.failure_code,
                Some(haiker_app::imports::failure_code::FailureCode::Timeout)
            );
        }
    }

    #[tokio::test]
    async fn cleanup_ignores_recent_imports() {
        let imports = vec![
            make_stuck_import(ImportStatus::Parsing, 5), // Only 5 minutes old
        ];
        let repo = MockImportRepository::new(imports);

        let cleaned = cleanup_abandoned_imports(&repo).await.unwrap();
        assert_eq!(cleaned, 0);
    }

    #[tokio::test]
    async fn cleanup_ignores_terminal_states() {
        let imports = vec![
            make_stuck_import(ImportStatus::Completed, 60),
            make_stuck_import(ImportStatus::Failed, 60),
            make_stuck_import(ImportStatus::Cancelled, 60),
        ];
        let repo = MockImportRepository::new(imports);

        let cleaned = cleanup_abandoned_imports(&repo).await.unwrap();
        assert_eq!(cleaned, 0);
    }

    #[tokio::test]
    async fn cleanup_ignores_requested_and_uploading() {
        let imports = vec![
            make_stuck_import(ImportStatus::Requested, 60),
            make_stuck_import(ImportStatus::Uploading, 60),
        ];
        let repo = MockImportRepository::new(imports);

        let cleaned = cleanup_abandoned_imports(&repo).await.unwrap();
        assert_eq!(cleaned, 0);
    }

    #[tokio::test]
    async fn cleanup_returns_zero_when_no_abandoned_imports() {
        let repo = MockImportRepository::new(vec![]);

        let cleaned = cleanup_abandoned_imports(&repo).await.unwrap();
        assert_eq!(cleaned, 0);
    }
}
