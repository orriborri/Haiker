//! Domain error types.
//!
//! Provides a unified error enum for domain operations that can be mapped
//! to appropriate HTTP responses by the API layer.

use thiserror::Error;

/// Errors that can occur in domain operations.
#[derive(Debug, Error)]
pub enum DomainError {
    /// The requested resource was not found.
    #[error("not found: {message}")]
    NotFound { message: String },

    /// The request lacks valid authentication credentials.
    #[error("unauthorized: {message}")]
    Unauthorized { message: String },

    /// The authenticated user does not have permission for this action.
    #[error("forbidden: {message}")]
    Forbidden { message: String },

    /// The request conflicts with the current state of the resource.
    #[error("conflict: {message}")]
    Conflict { message: String },

    /// The request contains invalid data.
    #[error("validation failed: {message}")]
    ValidationFailed { message: String },

    /// An unexpected internal error occurred.
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl DomainError {
    /// Create a not found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
        }
    }

    /// Create an unauthorized error.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            message: message.into(),
        }
    }

    /// Create a forbidden error.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            message: message.into(),
        }
    }

    /// Create a conflict error.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
        }
    }

    /// Create a validation failed error.
    pub fn validation_failed(message: impl Into<String>) -> Self {
        Self::ValidationFailed {
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
    fn not_found_error_display() {
        let err = DomainError::not_found("user 123");
        assert_eq!(err.to_string(), "not found: user 123");
    }

    #[test]
    fn unauthorized_error_display() {
        let err = DomainError::unauthorized("invalid token");
        assert_eq!(err.to_string(), "unauthorized: invalid token");
    }

    #[test]
    fn forbidden_error_display() {
        let err = DomainError::forbidden("insufficient permissions");
        assert_eq!(err.to_string(), "forbidden: insufficient permissions");
    }

    #[test]
    fn conflict_error_display() {
        let err = DomainError::conflict("duplicate email");
        assert_eq!(err.to_string(), "conflict: duplicate email");
    }

    #[test]
    fn validation_failed_error_display() {
        let err = DomainError::validation_failed("name is required");
        assert_eq!(err.to_string(), "validation failed: name is required");
    }

    #[test]
    fn internal_error_display() {
        let err = DomainError::internal("something went wrong");
        assert_eq!(err.to_string(), "internal error: something went wrong");
    }
}
