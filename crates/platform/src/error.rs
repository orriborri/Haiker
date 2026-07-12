//! Platform-level error types.
//!
//! These represent infrastructure errors that occur in the platform layer,
//! distinct from domain errors in the app crate.

use thiserror::Error;

/// Errors that can occur in platform infrastructure operations.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// The caller does not have permission to perform this action.
    #[error("forbidden: {message}")]
    Forbidden { message: String },

    /// The requested resource was not found.
    #[error("not found: {message}")]
    NotFound { message: String },

    /// An unexpected internal error occurred.
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl PlatformError {
    /// Create a forbidden error.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            message: message.into(),
        }
    }

    /// Create a not found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
        }
    }

    /// Create an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_error_display() {
        let err = PlatformError::forbidden("access denied");
        assert_eq!(err.to_string(), "forbidden: access denied");
    }

    #[test]
    fn not_found_error_display() {
        let err = PlatformError::not_found("session not found");
        assert_eq!(err.to_string(), "not found: session not found");
    }

    #[test]
    fn internal_error_display() {
        let err = PlatformError::internal("database error");
        assert_eq!(err.to_string(), "internal error: database error");
    }
}
