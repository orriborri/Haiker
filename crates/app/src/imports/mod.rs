//! Activity Importing bounded context.
//!
//! Owns upload, import state machine, validation, parsing orchestration,
//! and duplicate detection.

use thiserror::Error;

/// Errors that can occur in the imports context.
#[derive(Debug, Error)]
pub enum ImportError {
    /// The file format is not supported for import.
    #[error("unsupported file format")]
    UnsupportedFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = ImportError::UnsupportedFormat;
        assert_eq!(err.to_string(), "unsupported file format");
    }
}
