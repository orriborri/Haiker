//! Recorded Activity bounded context.
//!
//! Owns source artifacts, source revisions, recorded tracks, segments,
//! point streams, and sensor samples.

use thiserror::Error;

/// Errors that can occur in the recorded activity context.
#[derive(Debug, Error)]
pub enum RecordedActivityError {
    /// The requested recorded activity was not found.
    #[error("recorded activity not found")]
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = RecordedActivityError::NotFound;
        assert_eq!(err.to_string(), "recorded activity not found");
    }
}
