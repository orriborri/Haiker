//! API error handling.
//!
//! Maps domain errors to structured JSON responses with appropriate HTTP status codes.

#![allow(dead_code)]

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;

use haiker_app::error::DomainError;

/// API error response schema for OpenAPI documentation.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorResponse {
    /// Error details
    pub error: ApiErrorDetail,
}

/// Detailed error information returned by the API.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorDetail {
    /// Machine-readable error code
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Optional additional details about the error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// API error type that produces structured JSON error responses.
#[derive(Debug)]
pub struct ApiError {
    /// HTTP status code.
    pub status: StatusCode,
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional details.
    pub details: Option<serde_json::Value>,
}

/// Structured error response body.
#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorContent,
}

#[derive(Debug, Serialize)]
struct ErrorContent {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorContent {
                code: self.code,
                message: self.message,
                details: self.details,
            },
        };

        (self.status, Json(json!(body))).into_response()
    }
}

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::NotFound { message } => ApiError {
                status: StatusCode::NOT_FOUND,
                code: "NOT_FOUND".to_string(),
                message,
                details: None,
            },
            DomainError::Unauthorized { message } => ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: "UNAUTHORIZED".to_string(),
                message,
                details: None,
            },
            DomainError::Forbidden { message } => ApiError {
                status: StatusCode::FORBIDDEN,
                code: "FORBIDDEN".to_string(),
                message,
                details: None,
            },
            DomainError::Conflict { message } => ApiError {
                status: StatusCode::CONFLICT,
                code: "CONFLICT".to_string(),
                message,
                details: None,
            },
            DomainError::ValidationFailed { message } => ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "VALIDATION_FAILED".to_string(),
                message,
                details: None,
            },
            DomainError::Internal { message } => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL_ERROR".to_string(),
                message,
                details: None,
            },
        }
    }
}

impl ApiError {
    /// Create an unauthorized error (for authentication failures).
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHORIZED".to_string(),
            message: message.into(),
            details: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_not_found_maps_to_404() {
        let err: ApiError = DomainError::not_found("resource").into();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert_eq!(err.code, "NOT_FOUND");
    }

    #[test]
    fn domain_unauthorized_maps_to_401() {
        let err: ApiError = DomainError::unauthorized("bad token").into();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.code, "UNAUTHORIZED");
    }

    #[test]
    fn domain_forbidden_maps_to_403() {
        let err: ApiError = DomainError::forbidden("no access").into();
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert_eq!(err.code, "FORBIDDEN");
    }

    #[test]
    fn domain_conflict_maps_to_409() {
        let err: ApiError = DomainError::conflict("duplicate").into();
        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.code, "CONFLICT");
    }

    #[test]
    fn domain_validation_maps_to_422() {
        let err: ApiError = DomainError::validation_failed("bad data").into();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err.code, "VALIDATION_FAILED");
    }

    #[test]
    fn domain_internal_maps_to_500() {
        let err: ApiError = DomainError::internal("oops").into();
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.code, "INTERNAL_ERROR");
    }
}