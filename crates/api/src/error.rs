//! API error handling.
//!
//! Maps domain errors to structured JSON responses following the
//! [RFC 9457 Problem Details](https://www.rfc-editor.org/rfc/rfc9457) format.
//! Response shape: { type, title, status, code, detail, requestId }

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
    /// Human-readable error message (becomes `detail` in Problem Details).
    pub message: String,
    /// URI path identifying the problem type (e.g., "/problems/not-found").
    /// Falls back to a slug derived from `code` when not set.
    pub problem_type: Option<String>,
    /// Stable human-readable summary for the problem type.
    /// Falls back to the HTTP status's canonical reason when not set.
    pub title: Option<String>,
    /// Request ID for correlation, set from the request-id middleware extension.
    /// Falls back to a freshly generated ID if not set before the response is built.
    pub request_id: Option<String>,
    /// Optional additional details (unused in the Problem Details output, kept for internal use).
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// Create an ApiError with all Problem Details fields explicit.
    pub fn new(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
        problem_type: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            problem_type: Some(problem_type.into()),
            title: Some(title.into()),
            request_id: None,
            details: None,
        }
    }

    /// Attach a request ID to this error for correlation.
    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    /// Create an unauthorized error (for authentication failures).
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHORIZED".to_string(),
            message: message.into(),
            problem_type: Some("/problems/unauthorized".to_string()),
            title: Some("Unauthorized".to_string()),
            request_id: None,
            details: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let problem_type = self
            .problem_type
            .unwrap_or_else(|| default_problem_type(&self.code));
        let title = self.title.unwrap_or_else(|| default_title(self.status));
        let request_id = self
            .request_id
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let body = ProblemDetailBody {
            problem_type,
            title,
            status: self.status.as_u16(),
            code: self.code,
            detail: self.message,
            request_id,
        };

        (self.status, Json(body)).into_response()
    }
}

/// Derive a default problem type URI from the error code.
fn default_problem_type(code: &str) -> String {
    let slug = code.to_lowercase().replace('_', "-");
    format!("/problems/{slug}")
}

/// Derive a default title from the HTTP status code.
fn default_title(status: StatusCode) -> String {
    status.canonical_reason().unwrap_or("Error").to_string()
}

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::NotFound { message } => ApiError {
                status: StatusCode::NOT_FOUND,
                code: "NOT_FOUND".to_string(),
                message,
                problem_type: Some("/problems/not-found".to_string()),
                title: Some("Not Found".to_string()),
                request_id: None,
                details: None,
            },
            DomainError::Unauthorized { message } => ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: "UNAUTHORIZED".to_string(),
                message,
                problem_type: Some("/problems/unauthorized".to_string()),
                title: Some("Unauthorized".to_string()),
                request_id: None,
                details: None,
            },
            DomainError::Forbidden { message } => ApiError {
                status: StatusCode::FORBIDDEN,
                code: "FORBIDDEN".to_string(),
                message,
                problem_type: Some("/problems/forbidden".to_string()),
                title: Some("Forbidden".to_string()),
                request_id: None,
                details: None,
            },
            DomainError::Conflict { message } => ApiError {
                status: StatusCode::CONFLICT,
                code: "CONFLICT".to_string(),
                message,
                problem_type: Some("/problems/conflict".to_string()),
                title: Some("Conflict".to_string()),
                request_id: None,
                details: None,
            },
            DomainError::ValidationFailed { message } => ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "VALIDATION_FAILED".to_string(),
                message,
                problem_type: Some("/problems/validation-failed".to_string()),
                title: Some("Validation Failed".to_string()),
                request_id: None,
                details: None,
            },
            DomainError::Internal { message } => {
                tracing::error!(error = %message, "internal domain error");
                ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: "INTERNAL_ERROR".to_string(),
                    message: "an unexpected error occurred".to_string(),
                    problem_type: Some("/problems/internal-error".to_string()),
                    title: Some("Internal Server Error".to_string()),
                    request_id: None,
                    details: None,
                }
            }
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
            message: "resource not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
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
        assert!(uuid::Uuid::parse_str(request_id).is_ok());
    }

    #[tokio::test]
    async fn api_error_with_request_id_preserves_it_in_response() {
        let err = ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "resource not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: Some("abc-123".to_string()),
            details: None,
        };

        let response = err.into_response();
        let json = extract_problem_detail(response).await;
        assert_eq!(json["requestId"], "abc-123");
    }

    #[tokio::test]
    async fn domain_not_found_maps_to_404_problem_details() {
        let err: ApiError = DomainError::not_found("resource").into();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.problem_type, Some("/problems/not-found".to_string()));
        assert_eq!(err.title, Some("Not Found".to_string()));
        assert_eq!(err.message, "resource");
    }

    #[tokio::test]
    async fn domain_unauthorized_maps_to_401_problem_details() {
        let err: ApiError = DomainError::unauthorized("bad token").into();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.code, "UNAUTHORIZED");
        assert_eq!(err.problem_type, Some("/problems/unauthorized".to_string()));
        assert_eq!(err.title, Some("Unauthorized".to_string()));
        assert_eq!(err.message, "bad token");
    }

    #[tokio::test]
    async fn domain_forbidden_maps_to_403_problem_details() {
        let err: ApiError = DomainError::forbidden("no access").into();
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert_eq!(err.code, "FORBIDDEN");
        assert_eq!(err.problem_type, Some("/problems/forbidden".to_string()));
        assert_eq!(err.title, Some("Forbidden".to_string()));
        assert_eq!(err.message, "no access");
    }

    #[tokio::test]
    async fn domain_conflict_maps_to_409_problem_details() {
        let err: ApiError = DomainError::conflict("duplicate").into();
        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.code, "CONFLICT");
        assert_eq!(err.problem_type, Some("/problems/conflict".to_string()));
        assert_eq!(err.title, Some("Conflict".to_string()));
        assert_eq!(err.message, "duplicate");
    }

    #[tokio::test]
    async fn domain_validation_maps_to_422_problem_details() {
        let err: ApiError = DomainError::validation_failed("bad data").into();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err.code, "VALIDATION_FAILED");
        assert_eq!(
            err.problem_type,
            Some("/problems/validation-failed".to_string())
        );
        assert_eq!(err.title, Some("Validation Failed".to_string()));
        assert_eq!(err.message, "bad data");
    }

    #[tokio::test]
    async fn domain_internal_maps_to_500_with_sanitized_message() {
        let err: ApiError = DomainError::internal("database pool exhausted").into();
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.code, "INTERNAL_ERROR");
        assert_eq!(
            err.problem_type,
            Some("/problems/internal-error".to_string())
        );
        assert_eq!(err.title, Some("Internal Server Error".to_string()));
        // Internal details must never leak to the client
        assert_eq!(err.message, "an unexpected error occurred");
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

    #[test]
    fn into_response_produces_problem_details_shape() {
        let err = ApiError {
            status: StatusCode::CONFLICT,
            code: "REVISION_CONFLICT".to_string(),
            message: "revision conflict".to_string(),
            problem_type: Some("/problems/stale-route-draft".to_string()),
            title: Some("Route draft revision is stale".to_string()),
            request_id: Some("abc-123".to_string()),
            details: None,
        };

        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn default_problem_type_derives_from_code() {
        assert_eq!(
            default_problem_type("REVISION_CONFLICT"),
            "/problems/revision-conflict"
        );
        assert_eq!(default_problem_type("NOT_FOUND"), "/problems/not-found");
    }

    #[test]
    fn default_title_derives_from_status() {
        assert_eq!(default_title(StatusCode::NOT_FOUND), "Not Found");
        assert_eq!(
            default_title(StatusCode::INTERNAL_SERVER_ERROR),
            "Internal Server Error"
        );
    }

    #[test]
    fn new_constructor_sets_all_fields() {
        let err = ApiError::new(
            StatusCode::CONFLICT,
            "REVISION_CONFLICT",
            "revision conflict",
            "/problems/stale-route-draft",
            "Route draft revision is stale",
        );
        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.code, "REVISION_CONFLICT");
        assert_eq!(err.message, "revision conflict");
        assert_eq!(
            err.problem_type,
            Some("/problems/stale-route-draft".to_string())
        );
        assert_eq!(err.title, Some("Route draft revision is stale".to_string()));
        assert_eq!(err.request_id, None);
    }

    #[test]
    fn with_request_id_attaches_id() {
        let err = ApiError::unauthorized("missing token").with_request_id(Some("req-1".into()));
        assert_eq!(err.request_id, Some("req-1".to_string()));
    }
}
