//! Import state machine transition logic.
//!
//! Defines the valid state transitions for the import lifecycle:
//! Requested -> Uploading -> Uploaded -> Validating -> Queued -> Parsing -> Committing -> Completed
//! Any non-terminal state can transition to Failed or Cancelled.
//!
//! NOTE: The domain model steering specifies a `DuplicateReview` state between
//! `Parsing` and `Committing` for probable duplicates that require user review.
//! This is intentionally omitted for MVP. The current implementation handles
//! exact duplicates automatically (returns existing activity reference without
//! user review) per the "Exact Duplicate" rule. Probable-duplicate review will
//! be added in a future iteration when user-facing review flows are built.

use serde::{Deserialize, Serialize};

use super::ImportError;

/// The status of an import job through its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    /// Import has been requested but upload has not started.
    Requested,
    /// File upload is in progress.
    Uploading,
    /// File has been successfully uploaded.
    Uploaded,
    /// File is being validated (format, size, checksum).
    Validating,
    /// Import is queued for parsing by a worker.
    Queued,
    /// Worker is actively parsing the file.
    Parsing,
    /// Parsed data is being committed to the database.
    Committing,
    /// Import completed successfully.
    Completed,
    /// Import failed with an error.
    Failed,
    /// Import was cancelled by the user.
    Cancelled,
}

impl ImportStatus {
    /// Returns true if this status is a terminal state (no further transitions allowed).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ImportStatus::Completed | ImportStatus::Failed | ImportStatus::Cancelled
        )
    }

    /// Attempt to transition from the current status to the target status.
    /// Returns the new status on success, or an error if the transition is invalid.
    pub fn transition_to(self, target: ImportStatus) -> Result<ImportStatus, ImportError> {
        if self.is_terminal() {
            return Err(ImportError::InvalidTransition {
                from: self.to_string(),
                to: target.to_string(),
            });
        }

        let is_valid = match (self, target) {
            // Happy path transitions
            (ImportStatus::Requested, ImportStatus::Uploading) => true,
            (ImportStatus::Uploading, ImportStatus::Uploaded) => true,
            (ImportStatus::Uploaded, ImportStatus::Validating) => true,
            (ImportStatus::Validating, ImportStatus::Queued) => true,
            (ImportStatus::Queued, ImportStatus::Parsing) => true,
            (ImportStatus::Parsing, ImportStatus::Committing) => true,
            (ImportStatus::Committing, ImportStatus::Completed) => true,

            // Any non-terminal state can fail or be cancelled
            (_, ImportStatus::Failed) => true,
            (_, ImportStatus::Cancelled) => true,

            // All other transitions are invalid
            _ => false,
        };

        if is_valid {
            Ok(target)
        } else {
            Err(ImportError::InvalidTransition {
                from: self.to_string(),
                to: target.to_string(),
            })
        }
    }
}

impl std::fmt::Display for ImportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ImportStatus::Requested => "requested",
            ImportStatus::Uploading => "uploading",
            ImportStatus::Uploaded => "uploaded",
            ImportStatus::Validating => "validating",
            ImportStatus::Queued => "queued",
            ImportStatus::Parsing => "parsing",
            ImportStatus::Committing => "committing",
            ImportStatus::Completed => "completed",
            ImportStatus::Failed => "failed",
            ImportStatus::Cancelled => "cancelled",
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_happy_path_transitions() {
        let transitions = [
            (ImportStatus::Requested, ImportStatus::Uploading),
            (ImportStatus::Uploading, ImportStatus::Uploaded),
            (ImportStatus::Uploaded, ImportStatus::Validating),
            (ImportStatus::Validating, ImportStatus::Queued),
            (ImportStatus::Queued, ImportStatus::Parsing),
            (ImportStatus::Parsing, ImportStatus::Committing),
            (ImportStatus::Committing, ImportStatus::Completed),
        ];

        for (from, to) in transitions {
            let result = from.transition_to(to);
            assert_eq!(
                result,
                Ok(to),
                "Transition from {from} to {to} should succeed"
            );
        }
    }

    #[test]
    fn any_non_terminal_can_transition_to_failed() {
        let non_terminal = [
            ImportStatus::Requested,
            ImportStatus::Uploading,
            ImportStatus::Uploaded,
            ImportStatus::Validating,
            ImportStatus::Queued,
            ImportStatus::Parsing,
            ImportStatus::Committing,
        ];

        for from in non_terminal {
            let result = from.transition_to(ImportStatus::Failed);
            assert_eq!(
                result,
                Ok(ImportStatus::Failed),
                "Transition from {from} to Failed should succeed"
            );
        }
    }

    #[test]
    fn any_non_terminal_can_transition_to_cancelled() {
        let non_terminal = [
            ImportStatus::Requested,
            ImportStatus::Uploading,
            ImportStatus::Uploaded,
            ImportStatus::Validating,
            ImportStatus::Queued,
            ImportStatus::Parsing,
            ImportStatus::Committing,
        ];

        for from in non_terminal {
            let result = from.transition_to(ImportStatus::Cancelled);
            assert_eq!(
                result,
                Ok(ImportStatus::Cancelled),
                "Transition from {from} to Cancelled should succeed"
            );
        }
    }

    #[test]
    fn terminal_states_cannot_transition() {
        let terminal = [
            ImportStatus::Completed,
            ImportStatus::Failed,
            ImportStatus::Cancelled,
        ];

        let all_targets = [
            ImportStatus::Requested,
            ImportStatus::Uploading,
            ImportStatus::Uploaded,
            ImportStatus::Validating,
            ImportStatus::Queued,
            ImportStatus::Parsing,
            ImportStatus::Committing,
            ImportStatus::Completed,
            ImportStatus::Failed,
            ImportStatus::Cancelled,
        ];

        for from in terminal {
            for to in all_targets {
                let result = from.transition_to(to);
                assert!(
                    result.is_err(),
                    "Transition from {from} to {to} should be rejected"
                );
            }
        }
    }

    #[test]
    fn invalid_forward_skips_are_rejected() {
        let invalid_transitions = [
            (ImportStatus::Requested, ImportStatus::Parsing),
            (ImportStatus::Requested, ImportStatus::Completed),
            (ImportStatus::Requested, ImportStatus::Uploaded),
            (ImportStatus::Uploading, ImportStatus::Validating),
            (ImportStatus::Uploading, ImportStatus::Queued),
            (ImportStatus::Uploaded, ImportStatus::Parsing),
            (ImportStatus::Validating, ImportStatus::Committing),
            (ImportStatus::Queued, ImportStatus::Completed),
        ];

        for (from, to) in invalid_transitions {
            let result = from.transition_to(to);
            assert!(
                result.is_err(),
                "Transition from {from} to {to} should be rejected"
            );
            if let Err(ImportError::InvalidTransition {
                from: err_from,
                to: err_to,
            }) = result
            {
                assert_eq!(err_from, from.to_string());
                assert_eq!(err_to, to.to_string());
            } else {
                panic!("Expected InvalidTransition error");
            }
        }
    }

    #[test]
    fn backward_transitions_are_rejected() {
        let backward_transitions = [
            (ImportStatus::Uploading, ImportStatus::Requested),
            (ImportStatus::Uploaded, ImportStatus::Uploading),
            (ImportStatus::Validating, ImportStatus::Uploaded),
            (ImportStatus::Queued, ImportStatus::Validating),
            (ImportStatus::Parsing, ImportStatus::Queued),
            (ImportStatus::Committing, ImportStatus::Parsing),
        ];

        for (from, to) in backward_transitions {
            let result = from.transition_to(to);
            assert!(
                result.is_err(),
                "Backward transition from {from} to {to} should be rejected"
            );
        }
    }

    #[test]
    fn is_terminal_returns_correct_values() {
        assert!(!ImportStatus::Requested.is_terminal());
        assert!(!ImportStatus::Uploading.is_terminal());
        assert!(!ImportStatus::Uploaded.is_terminal());
        assert!(!ImportStatus::Validating.is_terminal());
        assert!(!ImportStatus::Queued.is_terminal());
        assert!(!ImportStatus::Parsing.is_terminal());
        assert!(!ImportStatus::Committing.is_terminal());
        assert!(ImportStatus::Completed.is_terminal());
        assert!(ImportStatus::Failed.is_terminal());
        assert!(ImportStatus::Cancelled.is_terminal());
    }

    #[test]
    fn display_formatting() {
        assert_eq!(ImportStatus::Requested.to_string(), "requested");
        assert_eq!(ImportStatus::Uploading.to_string(), "uploading");
        assert_eq!(ImportStatus::Uploaded.to_string(), "uploaded");
        assert_eq!(ImportStatus::Validating.to_string(), "validating");
        assert_eq!(ImportStatus::Queued.to_string(), "queued");
        assert_eq!(ImportStatus::Parsing.to_string(), "parsing");
        assert_eq!(ImportStatus::Committing.to_string(), "committing");
        assert_eq!(ImportStatus::Completed.to_string(), "completed");
        assert_eq!(ImportStatus::Failed.to_string(), "failed");
        assert_eq!(ImportStatus::Cancelled.to_string(), "cancelled");
    }
}
