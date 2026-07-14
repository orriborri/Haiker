//! API error handling.
//!
//! Maps domain errors to structured JSON responses following the
//! [RFC 9457 Problem Details](https://www.rfc-editor.org/rfc/rfc9457) format.

#![allow(dead_code)]

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use haiker_app::error::DomainError;

/// RFC 9457 Problem Details response body.
///
/// All API errors are serialized to this envelope.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetailBody {
    /// A URI path that identifies the problem type.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// A short, human-readable summary of the problem type.
    pub title: String,
    /// The HTTP status code for this occurrence.
    pub status: u16,
    /// An application-specific error code in UPPER_SNAKE_CASE.
    pub code: String,
    /// A human-readable explanation specific to this occurrence of the problem.
    pub detail: String,
    /// A unique request identifier for correlation.
    pub request_id: String,
}

/// API error type that produces RFC 9457 Problem Details JSON responses.
///
/// Constructed by API handlers and error-mapping functions. The `IntoResponse`
/// implementation serializes this to the `ProblemDetailBody` format.
#[derive(Debug)]
pub struct ApiError {
    /// HTTP status code.
    pub status: StatusCode,
    /// Machine-readable error code (UPPER_SNAKE_CASE).
    pub code: String,
    /// Instance-specific error explanation.
    pub detail: String,
    /// URI path identifying the problem type (e.g., "/problems/not-found").
    pub problem_type: String,
    /// Stable human-readable summary for the problem type.
    pub title: String,
}

impl ApiError {
    /// Create an ApiError with all required Problem Details fields.
    pub fn new(
        status: StatusCode,
        code: impl Into<String>,
        detail: impl Into<String>,
        problem_type: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code: code.into(),
            detail: detail.into(),
            problem_type: problem_type.into(),
            title: title.into(),
        }
    }

    /// Create an unauthorized error (for authentication failures).
    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHORIZED".to_string(),
            detail: detail.into(),
            problem_type: "/problems/unauthorized".to_string(),
            title: "Unauthorized".to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ProblemDetailBody {
            problem_type: self.problem_type,
            title: self.title,
            status: self.status.as_u16(),
            code: self.code,
            detail: self.detail,
            request_id: Uuid::new_v4().to_string(),
        };

        (self.status, Json(body)).into_response()
    }
}

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::NotFound { message } => ApiError {
                status: StatusCode::NOT_FOUND,
                code: "NOT_FOUND".to_string(),
                detail: message,
                problem_type: "/problems/not-found".to_string(),
                title: "Not Found".to_string(),
            },
            DomainError::Unauthorized { message } => ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: "UNAUTHORIZED".to_string(),
                detail: message,
                problem_type: "/problems/unauthorized".to_string(),
                title: "Unauthorized".to_string(),
            },
            DomainError::Forbidden { message } => ApiError {
                status: StatusCode::FORBIDDEN,
                code: "FORBIDDEN".to_string(),
                detail: message,
                problem_type: "/problems/forbidden".to_string(),
                title: "Forbidden".to_string(),
            },
            DomainError::Conflict { message } => ApiError {
                status: StatusCode::CONFLICT,
                code: "CONFLICT".to_string(),
                detail: message,
                problem_type: "/problems/conflict".to_string(),
                title: "Conflict".to_string(),
            },
            DomainError::ValidationFailed { message } => ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "VALIDATION_FAILED".to_string(),
                detail: message,
                problem_type: "/problems/validation-failed".to_string(),
                title: "Validation Failed".to_string(),
            },
            DomainError::Internal { message } => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL_ERROR".to_string(),
                detail: message,
                problem_type: "/problems/internal-error".to_string(),
                title: "Internal Server Error".to_string(),
            },
        }
    }
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
    async fn api_error_serializes_to_problem_details_format() {
        let err = ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            detail: "resource not found".to_string(),
            problem_type: "/problems/not-found".to_string(),
            title: "Not Found".to_string(),
        };

        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let json = extract_problem_detail(response).await;
        assert_eq!(json["type"], "/problems/not-found");
        assert_eq!(json["title"], "Not Found");
        assert_eq!(json["status"], 404);
        assert_eq!(json["code"], "NOT_FOUND");
        assert_eq!(json["detail"], "resource not found");
        assert!(json["requestId"].is_string());
        // Verify requestId is a valid UUID
        let request_id = json["requestId"].as_str().unwrap();
        assert!(Uuid::parse_str(request_id).is_ok());
    }

    #[tokio::test]
    async fn domain_not_found_maps_to_404_problem_details() {
        let err: ApiError = DomainError::not_found("resource").into();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.problem_type, "/problems/not-found");
        assert_eq!(err.title, "Not Found");
        assert_eq!(err.detail, "resource");
    }

    #[tokio::test]
    async fn domain_unauthorized_maps_to_401_problem_details() {
        let err: ApiError = DomainError::unauthorized("bad token").into();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.code, "UNAUTHORIZED");
        assert_eq!(err.problem_type, "/problems/unauthorized");
        assert_eq!(err.title, "Unauthorized");
        assert_eq!(err.detail, "bad token");
    }

    #[tokio::test]
    async fn domain_forbidden_maps_to_403_problem_details() {
        let err: ApiError = DomainError::forbidden("no access").into();
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert_eq!(err.code, "FORBIDDEN");
        assert_eq!(err.problem_type, "/problems/forbidden");
        assert_eq!(err.title, "Forbidden");
        assert_eq!(err.detail, "no access");
    }

    #[tokio::test]
    async fn domain_conflict_maps_to_409_problem_details() {
        let err: ApiError = DomainError::conflict("duplicate").into();
        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.code, "CONFLICT");
        assert_eq!(err.problem_type, "/problems/conflict");
        assert_eq!(err.title, "Conflict");
        assert_eq!(err.detail, "duplicate");
    }

    #[tokio::test]
    async fn domain_validation_maps_to_422_problem_details() {
        let err: ApiError = DomainError::validation_failed("bad data").into();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err.code, "VALIDATION_FAILED");
        assert_eq!(err.problem_type, "/problems/validation-failed");
        assert_eq!(err.title, "Validation Failed");
        assert_eq!(err.detail, "bad data");
    }

    #[tokio::test]
    async fn domain_internal_maps_to_500_problem_details() {
        let err: ApiError = DomainError::internal("oops").into();
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.code, "INTERNAL_ERROR");
        assert_eq!(err.problem_type, "/problems/internal-error");
        assert_eq!(err.title, "Internal Server Error");
        assert_eq!(err.detail, "oops");
    }

    #[tokio::test]
    async fn unauthorized_helper_produces_correct_problem_details() {
        let err = ApiError::unauthorized("missing token");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let json = extract_problem_detail(response).await;
        assert_eq!(json["type"], "/problems/unauthorized");
        assert_eq!(json["title"], "Unauthorized");
        assert_eq!(json["status"], 401);
        assert_eq!(json["code"], "UNAUTHORIZED");
        assert_eq!(json["detail"], "missing token");
    }
}
