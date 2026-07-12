//! Route Exporting bounded context.
//!
//! Owns export requests, jobs, generated artifacts, download authorization,
//! and expiration.

use thiserror::Error;

/// Errors that can occur in the exports context.
#[derive(Debug, Error)]
pub enum ExportError {
    /// The requested export was not found.
    #[error("export not found")]
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = ExportError::NotFound;
        assert_eq!(err.to_string(), "export not found");
    }
}
