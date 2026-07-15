//! Domain logic for cleaning up abandoned imports.
//!
//! Provides a pure domain function that transitions stuck imports to Failed
//! with a Timeout failure code.

use super::failure_code::FailureCode;
use super::{Import, ImportError};

/// Result of cleaning up abandoned imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupResult {
    /// Number of imports successfully transitioned to Failed.
    pub cleaned_up: usize,
    /// Number of imports that could not be transitioned (already terminal).
    pub skipped: usize,
}

/// Transition a list of abandoned imports to Failed with a Timeout failure code.
///
/// Each import is moved to the Failed state with a descriptive reason. Imports
/// that cannot be transitioned (e.g., already in a terminal state due to a race
/// condition) are skipped rather than causing the entire operation to fail.
///
/// Returns the count of imports that were successfully cleaned up and those skipped.
pub fn fail_abandoned_imports(imports: &mut [Import]) -> CleanupResult {
    let mut cleaned_up = 0;
    let mut skipped = 0;

    for import in imports.iter_mut() {
        let result = import.fail_with_code(
            "import processing timed out".to_string(),
            FailureCode::Timeout,
        );
        match result {
            Ok(()) => cleaned_up += 1,
            Err(ImportError::InvalidTransition { .. }) => skipped += 1,
            Err(_) => skipped += 1,
        }
    }

    CleanupResult {
        cleaned_up,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use crate::identity::UserId;
    use crate::imports::state_machine::ImportStatus;
    use crate::imports::{Import, ImportFormat};

    use super::*;

    fn make_import_with_status(status: ImportStatus) -> Import {
        let owner_id = UserId::new(Uuid::new_v4());
        let mut import =
            Import::new(owner_id, ImportFormat::Gpx, "key-test".to_string(), None).unwrap();
        import.status = status;
        import.updated_at = Utc::now() - Duration::minutes(60);
        import
    }

    #[test]
    fn fail_abandoned_imports_transitions_processing_states() {
        let processing_states = [
            ImportStatus::Validating,
            ImportStatus::Queued,
            ImportStatus::Parsing,
            ImportStatus::Committing,
        ];

        for status in processing_states {
            let mut imports = vec![make_import_with_status(status)];
            let result = fail_abandoned_imports(&mut imports);

            assert_eq!(result.cleaned_up, 1);
            assert_eq!(result.skipped, 0);
            assert_eq!(imports[0].status, ImportStatus::Failed);
            assert_eq!(imports[0].failure_code, Some(FailureCode::Timeout));
            assert_eq!(
                imports[0].failure_reason.as_deref(),
                Some("import processing timed out")
            );
        }
    }

    #[test]
    fn fail_abandoned_imports_skips_terminal_states() {
        let terminal_states = [
            ImportStatus::Completed,
            ImportStatus::Failed,
            ImportStatus::Cancelled,
        ];

        for status in terminal_states {
            let mut imports = vec![make_import_with_status(status)];
            let result = fail_abandoned_imports(&mut imports);

            assert_eq!(result.cleaned_up, 0, "Should skip terminal state {status}");
            assert_eq!(result.skipped, 1);
        }
    }

    #[test]
    fn fail_abandoned_imports_handles_mixed_states() {
        let mut imports = vec![
            make_import_with_status(ImportStatus::Parsing),
            make_import_with_status(ImportStatus::Completed),
            make_import_with_status(ImportStatus::Committing),
            make_import_with_status(ImportStatus::Failed),
        ];

        let result = fail_abandoned_imports(&mut imports);

        assert_eq!(result.cleaned_up, 2);
        assert_eq!(result.skipped, 2);
    }

    #[test]
    fn fail_abandoned_imports_handles_empty_list() {
        let mut imports: Vec<Import> = vec![];
        let result = fail_abandoned_imports(&mut imports);

        assert_eq!(result.cleaned_up, 0);
        assert_eq!(result.skipped, 0);
    }
}
