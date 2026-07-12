//! Identity & Access bounded context.
//!
//! Owns authentication, sessions, ownership, and authorization.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// A strongly-typed user identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

impl UserId {
    /// Create a new random user ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The authenticated actor making a request.
#[derive(Debug, Clone)]
pub struct Actor {
    /// The user's unique identifier.
    pub user_id: UserId,
}

/// Errors that can occur in the identity context.
#[derive(Debug, Error)]
pub enum IdentityError {
    /// The requested user was not found.
    #[error("user not found")]
    UserNotFound,

    /// The provided authentication token is invalid.
    #[error("invalid token")]
    InvalidToken,

    /// The authentication token has expired.
    #[error("token expired")]
    TokenExpired,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = IdentityError::UserNotFound;
        assert_eq!(err.to_string(), "user not found");
    }

    #[test]
    fn user_id_display() {
        let id = UserId(Uuid::nil());
        assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn user_id_new_is_unique() {
        let a = UserId::new();
        let b = UserId::new();
        assert_ne!(a, b);
    }
}
