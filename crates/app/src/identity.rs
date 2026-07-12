//! Identity bounded context - user authentication and authorization.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// A strongly-typed user identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

impl UserId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents an authenticated user making a request.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub user_id: UserId,
}

impl Actor {
    pub fn new(user_id: UserId) -> Self {
        Self { user_id }
    }
}

/// Errors that can occur during authentication.
#[derive(Debug, Error)]
pub enum AuthenticationError {
    #[error("Invalid session")]
    InvalidSession,

    #[error("Missing credentials")]
    MissingCredentials,

    #[error("Session expired")]
    Expired,
}

/// Trait for validating sessions and producing an authenticated Actor.
///
/// Implementations can validate JWT tokens, session cookies, or other credentials.
#[allow(async_fn_in_trait)]
pub trait SessionValidator {
    async fn validate(&self, token: &str) -> Result<Actor, AuthenticationError>;
}
