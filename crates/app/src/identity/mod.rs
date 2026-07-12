//! Identity & Access bounded context.
//!
//! Owns authentication, sessions, ownership, and authorization.

use thiserror::Error;

/// Errors that can occur in the identity context.
#[derive(Debug, Error)]
pub enum IdentityError {
    /// The requested user was not found.
    #[error("user not found")]
    UserNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = IdentityError::UserNotFound;
        assert_eq!(err.to_string(), "user not found");
    }
}
