//! Authentication middleware and extractors.
//!
//! Provides an Axum extractor that resolves the authenticated `Actor` from
//! a Bearer token in the Authorization header.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use haiker_app::identity::{Actor, UserId};

/// Axum extractor wrapper for the authenticated actor.
///
/// Wraps the domain `Actor` type so we can implement Axum's `FromRequestParts`
/// in this crate.
pub struct AuthenticatedActor(pub Actor);

impl<S> FromRequestParts<S> for AuthenticatedActor
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::MissingToken)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidToken)?;

        // In this placeholder implementation, the token is the user's UUID.
        // A real implementation would validate a JWT or session token.
        let user_uuid =
            Uuid::parse_str(token).map_err(|_| AuthError::InvalidToken)?;

        Ok(AuthenticatedActor(Actor {
            user_id: UserId(user_uuid),
        }))
    }
}

/// Authentication error type.
#[derive(Debug)]
pub enum AuthError {
    /// No Authorization header was provided.
    MissingToken,
    /// The token format is invalid.
    InvalidToken,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            AuthError::MissingToken => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "missing authorization header",
            ),
            AuthError::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "invalid bearer token",
            ),
        };

        let body = json!({
            "error": {
                "code": code,
                "message": message
            }
        });

        (status, Json(body)).into_response()
    }
}

/// Handler for GET /me endpoint.
///
/// Returns the authenticated user's information.
#[utoipa::path(
    get,
    path = "/me",
    responses(
        (status = 200, description = "Current user information"),
        (status = 401, description = "Missing or invalid authentication")
    ),
    security(
        ("bearer" = [])
    )
)]
pub async fn me(actor: AuthenticatedActor) -> Json<serde_json::Value> {
    Json(json!({
        "user_id": actor.0.user_id.0
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_missing_token_display() {
        let err = AuthError::MissingToken;
        // Just verify it can be converted to response
        let _response = err.into_response();
    }

    #[test]
    fn auth_error_invalid_token_display() {
        let err = AuthError::InvalidToken;
        let _response = err.into_response();
    }
}
