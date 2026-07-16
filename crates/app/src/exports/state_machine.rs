//! Export state machine transition logic.
//!
//! Defines the valid state transitions for the export lifecycle:
//! Queued -> Generating -> Ready
//!                      -> Failed
//! Ready -> Expired
//! Any non-terminal state can transition to Failed.
//!
//! Terminal states: Ready, Failed, Expired.

use serde::{Deserialize, Serialize};

use super::ExportError;

/// The status of an export job through its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportStatus {
    /// Export has been requested, waiting for worker pickup.
    Queued,
    /// Worker is actively generating the export file.
    Generating,
    /// Export file is available for download.
    Ready,
    /// Export generation failed.
    Failed,
    /// Export file has expired and is no longer available.
    Expired,
}

impl ExportStatus {
    /// Returns true if this status is a terminal state (no further transitions allowed).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ExportStatus::Ready | ExportStatus::Failed | ExportStatus::Expired
        )
    }

    /// Attempt to transition from the current status to the target status.
    /// Returns the new status on success, or an error if the transition is invalid.
    pub fn transition_to(self, target: ExportStatus) -> Result<ExportStatus, ExportError> {
        if self.is_terminal() {
            // Special case: Ready -> Expired is allowed
            if self == ExportStatus::Ready && target == ExportStatus::Expired {
                return Ok(target);
            }
            return Err(ExportError::InvalidTransition {
                from: self.to_string(),
                to: target.to_string(),
            });
        }

        let is_valid = match (self, target) {
            // Happy path transitions
            (ExportStatus::Queued, ExportStatus::Generating) => true,
            (ExportStatus::Generating, ExportStatus::Ready) => true,

            // Ready -> Expired
            (ExportStatus::Ready, ExportStatus::Expired) => true,

            // Any non-terminal state can fail
            (_, ExportStatus::Failed) => true,

            // All other transitions are invalid
            _ => false,
        };

        if is_valid {
            Ok(target)
        } else {
            Err(ExportError::InvalidTransition {
                from: self.to_string(),
                to: target.to_string(),
            })
        }
    }
}

impl std::fmt::Display for ExportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ExportStatus::Queued => "queued",
            ExportStatus::Generating => "generating",
            ExportStatus::Ready => "ready",
            ExportStatus::Failed => "failed",
            ExportStatus::Expired => "expired",
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
            (ExportStatus::Queued, ExportStatus::Generating),
            (ExportStatus::Generating, ExportStatus::Ready),
            (ExportStatus::Ready, ExportStatus::Expired),
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
        let non_terminal = [ExportStatus::Queued, ExportStatus::Generating];

        for from in non_terminal {
            let result = from.transition_to(ExportStatus::Failed);
            assert_eq!(
                result,
                Ok(ExportStatus::Failed),
                "Transition from {from} to Failed should succeed"
            );
        }
    }

    #[test]
    fn generating_to_failed_succeeds() {
        let result = ExportStatus::Generating.transition_to(ExportStatus::Failed);
        assert_eq!(result, Ok(ExportStatus::Failed));
    }

    #[test]
    fn terminal_states_cannot_transition_except_ready_to_expired() {
        let terminal = [ExportStatus::Failed, ExportStatus::Expired];

        let all_targets = [
            ExportStatus::Queued,
            ExportStatus::Generating,
            ExportStatus::Ready,
            ExportStatus::Failed,
            ExportStatus::Expired,
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
    fn ready_cannot_transition_to_generating() {
        let result = ExportStatus::Ready.transition_to(ExportStatus::Generating);
        assert!(result.is_err());
    }

    #[test]
    fn ready_cannot_transition_to_queued() {
        let result = ExportStatus::Ready.transition_to(ExportStatus::Queued);
        assert!(result.is_err());
    }

    #[test]
    fn ready_can_transition_to_expired() {
        let result = ExportStatus::Ready.transition_to(ExportStatus::Expired);
        assert_eq!(result, Ok(ExportStatus::Expired));
    }

    #[test]
    fn invalid_forward_skips_are_rejected() {
        let invalid_transitions = [
            (ExportStatus::Queued, ExportStatus::Ready),
            (ExportStatus::Queued, ExportStatus::Expired),
        ];

        for (from, to) in invalid_transitions {
            let result = from.transition_to(to);
            assert!(
                result.is_err(),
                "Transition from {from} to {to} should be rejected"
            );
            if let Err(ExportError::InvalidTransition {
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
            (ExportStatus::Generating, ExportStatus::Queued),
            (ExportStatus::Ready, ExportStatus::Generating),
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
        assert!(!ExportStatus::Queued.is_terminal());
        assert!(!ExportStatus::Generating.is_terminal());
        assert!(ExportStatus::Ready.is_terminal());
        assert!(ExportStatus::Failed.is_terminal());
        assert!(ExportStatus::Expired.is_terminal());
    }

    #[test]
    fn display_formatting() {
        assert_eq!(ExportStatus::Queued.to_string(), "queued");
        assert_eq!(ExportStatus::Generating.to_string(), "generating");
        assert_eq!(ExportStatus::Ready.to_string(), "ready");
        assert_eq!(ExportStatus::Failed.to_string(), "failed");
        assert_eq!(ExportStatus::Expired.to_string(), "expired");
    }
}
