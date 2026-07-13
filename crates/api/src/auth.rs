//! Authentication middleware and extractors.
//!
//! Provides an Axum extractor that resolves the authenticated `Actor` from
//! a Bearer token in the Authorization header.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use uuid::Uuid;

use haiker_app::identity::{Actor, UserId};

use crate::error::ProblemDetailBody;

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
        let user_uuid = Uuid::parse_str(token).map_err(|_| AuthError::InvalidToken)?;

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
        let detail = match self {
            AuthError::MissingToken => "missing authorization header",
            AuthError::InvalidToken => "invalid bearer token",
        };

        let body = ProblemDetailBody {
            problem_type: "/problems/unauthorized".to_string(),
            title: "Unauthorized".to_string(),
            status: StatusCode::UNAUTHORIZED.as_u16(),
            code: "UNAUTHORIZED".to_string(),
            detail: detail.to_string(),
            request_id: Uuid::new_v4().to_string(),
        };

        (StatusCode::UNAUTHORIZED, Json(body)).into_response()
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
    Json(serde_json::json!({
        "user_id": actor.0.user_id.0
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Response as HttpResponse;

    async fn extract_problem_detail(response: HttpResponse<Body>) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn auth_error_missing_token_returns_problem_details() {
        let err = AuthError::MissingToken;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let json = extract_problem_detail(response).await;
        assert_eq!(json["type"], "/problems/unauthorized");
        assert_eq!(json["title"], "Unauthorized");
        assert_eq!(json["status"], 401);
        assert_eq!(json["code"], "UNAUTHORIZED");
        assert_eq!(json["detail"], "missing authorization header");
        assert!(json["requestId"].is_string());
        let request_id = json["requestId"].as_str().unwrap();
        assert!(Uuid::parse_str(request_id).is_ok());
    }

    #[tokio::test]
    async fn auth_error_invalid_token_returns_problem_details() {
        let err = AuthError::InvalidToken;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let json = extract_problem_detail(response).await;
        assert_eq!(json["type"], "/problems/unauthorized");
        assert_eq!(json["title"], "Unauthorized");
        assert_eq!(json["status"], 401);
        assert_eq!(json["code"], "UNAUTHORIZED");
        assert_eq!(json["detail"], "invalid bearer token");
        assert!(json["requestId"].is_string());
        let request_id = json["requestId"].as_str().unwrap();
        assert!(Uuid::parse_str(request_id).is_ok());
    }
}
