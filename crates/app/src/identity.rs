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

    #[error("OIDC provider error: {0}")]
    ProviderError(String),

    #[error("Invalid state parameter")]
    InvalidState,

    #[error("Code exchange failed: {0}")]
    CodeExchangeFailed(String),
}

/// Trait for validating sessions and producing an authenticated Actor.
///
/// Implementations can validate JWT tokens, session cookies, or other credentials.
#[allow(async_fn_in_trait)]
pub trait SessionValidator {
    async fn validate(&self, token: &str) -> Result<Actor, AuthenticationError>;
}

/// Configuration for connecting to an OIDC identity provider.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// The OIDC issuer URL (e.g., https://accounts.google.com).
    pub issuer_url: String,
    /// OAuth2 client ID.
    pub client_id: String,
    /// OAuth2 client secret.
    pub client_secret: String,
    /// The redirect URI registered with the provider.
    pub redirect_uri: String,
}

/// Claims extracted from an OIDC ID token after verification.
#[derive(Debug, Clone)]
pub struct OidcClaims {
    /// The subject identifier (unique user ID from the provider).
    pub subject: String,
    /// The user's email address, if provided.
    pub email: Option<String>,
    /// The user's display name, if provided.
    pub name: Option<String>,
}

/// Trait for OIDC authentication operations.
///
/// This trait is infrastructure-free: implementations handle HTTP calls internally
/// but the interface uses only basic types (String, Result).
#[allow(async_fn_in_trait)]
pub trait OidcProvider {
    /// Generate an authorization URL for redirecting the user to the provider.
    ///
    /// Returns a tuple of (authorization_url, state, nonce).
    async fn authorization_url(&self) -> Result<(String, String, String), AuthenticationError>;

    /// Exchange an authorization code for user claims.
    ///
    /// The `state` parameter should be verified against the value returned
    /// by `authorization_url()`.
    async fn exchange_code(
        &self,
        code: &str,
        state: &str,
    ) -> Result<OidcClaims, AuthenticationError>;
}
