//! Export API handlers.
//!
//! Implements POST /v1/activities/{activityId}/exports, GET /v1/exports/{exportId},
//! and GET /v1/exports/{exportId}/download.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::exports::commands::{
    handle_download_export, handle_get_export, handle_request_export, RequestExportCommand,
};
use haiker_app::exports::job_types::{GenerateExportJob, GENERATE_EXPORT_JOB_TYPE};
use haiker_app::exports::repository::ExportRepository;
use haiker_app::exports::{
    DownloadUrlGenerator, ExportError, ExportFormat, ExportJob, ExportJobId, RouteVersionGateway,
};
use haiker_app::route_versioning::RouteVersionId;

use crate::auth::AuthenticatedActor;
use crate::error::ApiError;
use crate::exports_dto::{
    ExportDownloadUrlResponse, ExportStatusResponse, RequestExportRequest, RequestExportResponse,
};
use crate::imports::JobEnqueuer;

/// Shared application state for export handlers.
#[derive(Clone)]
pub struct ExportAppState {
    pub repo: Arc<dyn ExportRepository>,
    pub route_version_gateway: Arc<dyn RouteVersionGateway>,
    pub job_queue: Option<Arc<dyn JobEnqueuer>>,
    pub download_url_generator: Option<Arc<dyn DownloadUrlGenerator>>,
    pub audit_sink: Option<Arc<dyn ExportAuditSink>>,
}

/// Trait for audit logging within the export subsystem.
///
/// Abstracts the audit infrastructure so the API handler does not depend
/// directly on the platform audit implementation.
#[async_trait::async_trait]
pub trait ExportAuditSink: Send + Sync {
    /// Record an audit event for an export action.
    async fn record(
        &self,
        actor_id: Uuid,
        action: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), ExportError>;
}

/// Presigned download URL time-to-live in seconds.
///
/// Controls how long the presigned URL remains valid after generation.
/// Kept short to limit the exposure window if a URL is inadvertently leaked.
const DOWNLOAD_URL_TTL_SECONDS: u64 = 300;

/// Known client-safe failure reason prefixes for export errors.
const SAFE_EXPORT_FAILURE_REASONS: &[&str] = &[
    "generation failed",
    "format not supported",
    "route version unavailable",
    "export file corrupted",
];

/// Sanitize a failure reason before exposing it in API responses.
fn sanitize_failure_reason(reason: &str) -> String {
    let lower = reason.to_lowercase();
    for safe in SAFE_EXPORT_FAILURE_REASONS {
        if lower.starts_with(safe) {
            return reason.to_string();
        }
    }
    "an internal error occurred".to_string()
}

/// Convert an ExportJob domain model to the API response DTO.
fn export_to_status_response(export: &ExportJob) -> ExportStatusResponse {
    ExportStatusResponse {
        id: export.id.0,
        status: export.status.to_string(),
        format: export.format.to_string(),
        route_version_id: export.route_version_id.0,
        activity_id: export.activity_id.0,
        failure_reason: export
            .failure_reason
            .as_deref()
            .map(sanitize_failure_reason),
        checksum: export.checksum.clone(),
        expires_at: export.expires_at,
        created_at: export.created_at,
        updated_at: export.updated_at,
    }
}

/// Extract the Idempotency-Key header value.
#[allow(clippy::result_large_err)]
fn extract_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header is required".to_string(),
            problem_type: Some("/problems/missing-idempotency-key".to_string()),
            title: Some("Missing Idempotency Key".to_string()),
            request_id: None,
            details: None,
        })?;

    if key.trim().is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header must not be empty".to_string(),
            problem_type: Some("/problems/missing-idempotency-key".to_string()),
            title: Some("Missing Idempotency Key".to_string()),
            request_id: None,
            details: None,
        });
    }

    Ok(key.to_string())
}

/// Convert an ExportError to an ApiError.
fn export_error_to_api_error(err: ExportError) -> ApiError {
    match err {
        ExportError::NotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "export not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::Unauthorized => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "export not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::InvalidTransition { from, to } => ApiError {
            status: StatusCode::CONFLICT,
            code: "INVALID_STATE_TRANSITION".to_string(),
            message: format!("cannot transition from {from} to {to}"),
            problem_type: Some("/problems/invalid-state-transition".to_string()),
            title: Some("Invalid State Transition".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::IdempotencyPayloadMismatch => ApiError {
            status: StatusCode::CONFLICT,
            code: "IDEMPOTENCY_PAYLOAD_MISMATCH".to_string(),
            message: "idempotency key reused with different payload".to_string(),
            problem_type: Some("/problems/idempotency-conflict".to_string()),
            title: Some("Idempotency Conflict".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::RouteVersionNotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "route version not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::ActivityNotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "activity not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::ValidationFailed { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "VALIDATION_FAILED".to_string(),
            message,
            problem_type: Some("/problems/validation-failed".to_string()),
            title: Some("Validation Failed".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::PersistenceError { message } => {
            tracing::error!(error = %message, "persistence error during export operation");
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
        ExportError::ChecksumMismatch { .. } => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: "an unexpected error occurred".to_string(),
            problem_type: Some("/problems/internal-error".to_string()),
            title: Some("Internal Server Error".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::ArtifactExpired => ApiError {
            status: StatusCode::GONE,
            code: "ARTIFACT_EXPIRED".to_string(),
            message: "export artifact has expired and is no longer available".to_string(),
            problem_type: Some("/problems/artifact-expired".to_string()),
            title: Some("Artifact Expired".to_string()),
            request_id: None,
            details: None,
        },
        ExportError::NotReady { status } => ApiError {
            status: StatusCode::CONFLICT,
            code: "EXPORT_NOT_READY".to_string(),
            message: format!("export is not ready for download: current status is {status}"),
            problem_type: Some("/problems/export-not-ready".to_string()),
            title: Some("Export Not Ready".to_string()),
            request_id: None,
            details: None,
        },
    }
}

/// Parse the export format string into the domain ExportFormat enum.
#[allow(clippy::result_large_err)]
fn parse_export_format(format: &str) -> Result<ExportFormat, ApiError> {
    match format.to_lowercase().as_str() {
        "gpx" => Ok(ExportFormat::Gpx),
        _ => Err(ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "VALIDATION_FAILED".to_string(),
            message: format!("unsupported export format: {format}"),
            problem_type: Some("/problems/validation-failed".to_string()),
            title: Some("Validation Failed".to_string()),
            request_id: None,
            details: None,
        }),
    }
}

/// POST /v1/activities/{activityId}/exports
///
/// Request a new export. Requires Idempotency-Key header.
/// Returns 202 Accepted with the export ID and status.
pub async fn post_request_export(
    State(state): State<ExportAppState>,
    actor: AuthenticatedActor,
    Path(activity_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<RequestExportRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let idempotency_key = extract_idempotency_key(&headers)?;
    let format = parse_export_format(&body.format)?;

    let cmd = RequestExportCommand {
        owner_id: actor.0.user_id,
        activity_id: ActivityId::new(activity_id),
        route_version_id: RouteVersionId::new(body.route_version_id),
        format,
        idempotency_key,
    };

    let export_job = handle_request_export(
        cmd,
        state.repo.as_ref(),
        state.route_version_gateway.as_ref(),
    )
    .await
    .map_err(export_error_to_api_error)?;

    // Enqueue background generation job if queue is available
    if let Some(ref job_queue) = state.job_queue {
        let correlation_id = export_job.id.0;

        let job_payload = GenerateExportJob {
            export_job_id: export_job.id.0,
            activity_id: export_job.activity_id.0,
            route_version_id: export_job.route_version_id.0,
            owner_id: export_job.requested_by.0,
            format: export_job.format.to_string(),
            correlation_id,
        };

        let payload_json = serde_json::to_value(&job_payload).map_err(|e| {
            tracing::error!("failed to serialize export job payload: {e}");
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL_ERROR".to_string(),
                message: "an unexpected error occurred".to_string(),
                problem_type: Some("/problems/internal-error".to_string()),
                title: Some("Internal Server Error".to_string()),
                request_id: None,
                details: None,
            }
        })?;

        job_queue
            .enqueue(GENERATE_EXPORT_JOB_TYPE, payload_json, correlation_id)
            .await
            .map_err(|e| {
                tracing::error!("failed to enqueue export generation job: {e}");
                ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: "JOB_ENQUEUE_FAILED".to_string(),
                    message: "an unexpected error occurred".to_string(),
                    problem_type: Some("/problems/internal-error".to_string()),
                    title: Some("Internal Server Error".to_string()),
                    request_id: None,
                    details: None,
                }
            })?;
    }

    let response = RequestExportResponse {
        export_id: export_job.id.0,
        status: export_job.status.to_string(),
    };

    Ok((StatusCode::ACCEPTED, Json(response)))
}

/// GET /v1/exports/{exportId}
///
/// Get the current status of an export.
pub async fn get_export_status(
    State(state): State<ExportAppState>,
    actor: AuthenticatedActor,
    Path(export_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let export_job = handle_get_export(
        ExportJobId::new(export_id),
        actor.0.user_id,
        state.repo.as_ref(),
    )
    .await
    .map_err(export_error_to_api_error)?;

    Ok((StatusCode::OK, Json(export_to_status_response(&export_job))))
}

/// Sanitize a string into a safe filename component.
///
/// Keeps ASCII alphanumeric characters and hyphens, replaces everything else
/// with hyphens, collapses consecutive hyphens, trims hyphens from both ends,
/// truncates to max 100 characters (including extension), and defaults to "export"
/// if the result is empty.
fn sanitize_filename(name: &str, extension: &str) -> String {
    let max_base_len = 100 - extension.len() - 1; // account for the dot and extension

    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    let result = result.trim_matches('-').to_string();

    // Truncate if too long
    let result = if result.len() > max_base_len {
        result[..max_base_len].trim_end_matches('-').to_string()
    } else {
        result
    };

    // Default if empty
    let base = if result.is_empty() {
        "export".to_string()
    } else {
        result
    };

    format!("{base}.{extension}")
}

/// GET /v1/exports/{exportId}/download
///
/// Authorize and return a short-lived presigned download URL for the export.
pub async fn get_export_download(
    State(state): State<ExportAppState>,
    actor: AuthenticatedActor,
    Path(export_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate authorization, status, and expiry via domain command
    let export_job = handle_download_export(
        ExportJobId::new(export_id),
        actor.0.user_id,
        state.repo.as_ref(),
    )
    .await
    .map_err(export_error_to_api_error)?;

    // Extract required fields from the export job
    let storage_key = export_job.object_storage_key.ok_or_else(|| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "INTERNAL_ERROR".to_string(),
        message: "an unexpected error occurred".to_string(),
        problem_type: Some("/problems/internal-error".to_string()),
        title: Some("Internal Server Error".to_string()),
        request_id: None,
        details: None,
    })?;

    let checksum = export_job.checksum.unwrap_or_default();

    // Determine content type and extension based on format
    let (content_type, extension) = match export_job.format {
        ExportFormat::Gpx => ("application/gpx+xml", "gpx"),
    };

    // TODO: The filename defaults to "export.gpx" because the ExportJob aggregate
    // does not currently store the activity title. Once the domain model is extended
    // to include the activity title, pass it here for a more descriptive filename.
    let filename = sanitize_filename("export", extension);

    // Generate presigned download URL
    let download_url_generator = state
        .download_url_generator
        .as_ref()
        .ok_or_else(|| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: "an unexpected error occurred".to_string(),
            problem_type: Some("/problems/internal-error".to_string()),
            title: Some("Internal Server Error".to_string()),
            request_id: None,
            details: None,
        })?;

    let download_info = download_url_generator
        .generate_download_url(
            &storage_key,
            &filename,
            content_type,
            DOWNLOAD_URL_TTL_SECONDS,
        )
        .await
        .map_err(export_error_to_api_error)?;

    // Log audit event (do NOT log the URL). Fail-open for availability but
    // emit a warning so audit pipeline failures are observable.
    if let Some(ref audit) = state.audit_sink {
        if let Err(e) = audit
            .record(
                actor.0.user_id.0,
                "export.download",
                "export_job",
                &export_id.to_string(),
            )
            .await
        {
            tracing::warn!(
                error = %e,
                export_id = %export_id,
                "failed to record audit event for export download"
            );
        }
    }

    let response = ExportDownloadUrlResponse {
        download_url: download_info.url,
        filename,
        expires_at: download_info.expires_at,
        checksum,
        content_type: content_type.to_string(),
    };

    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));

    Ok((StatusCode::OK, headers, Json(response)))
}

#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

/// In-memory export repository for testing.
#[cfg(test)]
pub struct InMemoryExportRepository {
    exports: Mutex<HashMap<ExportJobId, ExportJob>>,
}

#[cfg(test)]
impl InMemoryExportRepository {
    pub fn new() -> Self {
        Self {
            exports: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl ExportRepository for InMemoryExportRepository {
    async fn save(&self, export_job: &ExportJob) -> Result<(), ExportError> {
        self.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: ExportJobId) -> Result<Option<ExportJob>, ExportError> {
        Ok(self.exports.lock().unwrap().get(&id).cloned())
    }

    async fn find_by_idempotency_key(
        &self,
        owner_id: haiker_app::identity::UserId,
        key: &str,
    ) -> Result<Option<ExportJob>, ExportError> {
        Ok(self
            .exports
            .lock()
            .unwrap()
            .values()
            .find(|e| e.requested_by == owner_id && e.idempotency_key == key)
            .cloned())
    }

    async fn update(&self, export_job: &ExportJob) -> Result<(), ExportError> {
        self.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());
        Ok(())
    }
}

/// Stub route version gateway that always succeeds.
#[cfg(test)]
pub struct StubRouteVersionGateway;

#[cfg(test)]
#[async_trait]
impl RouteVersionGateway for StubRouteVersionGateway {
    async fn verify_owned_route_version(
        &self,
        _activity_id: ActivityId,
        _route_version_id: RouteVersionId,
        _owner_id: haiker_app::identity::UserId,
    ) -> Result<(), ExportError> {
        Ok(())
    }
}

/// Fake download URL generator for testing.
#[cfg(test)]
pub struct FakeDownloadUrlGenerator;

#[cfg(test)]
#[async_trait]
impl DownloadUrlGenerator for FakeDownloadUrlGenerator {
    async fn generate_download_url(
        &self,
        _key: &str,
        _filename: &str,
        _content_type: &str,
        expires_in_seconds: u64,
    ) -> Result<haiker_app::exports::DownloadInfo, ExportError> {
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in_seconds as i64);
        Ok(haiker_app::exports::DownloadInfo {
            url: "https://storage.example.com/presigned-download-url?token=abc123".to_string(),
            expires_at,
        })
    }
}

/// Fake audit sink for testing that records calls.
#[cfg(test)]
pub struct FakeAuditSink {
    records: Mutex<Vec<(Uuid, String, String, String)>>,
}

#[cfg(test)]
impl FakeAuditSink {
    pub fn new() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
        }
    }

    pub fn records(&self) -> Vec<(Uuid, String, String, String)> {
        self.records.lock().unwrap().clone()
    }
}

#[cfg(test)]
#[async_trait]
impl ExportAuditSink for FakeAuditSink {
    async fn record(
        &self,
        actor_id: Uuid,
        action: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), ExportError> {
        self.records.lock().unwrap().push((
            actor_id,
            action.to_string(),
            resource_type.to_string(),
            resource_id.to_string(),
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    fn test_app() -> Router {
        let state = ExportAppState {
            repo: Arc::new(InMemoryExportRepository::new()),
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: Some(Arc::new(FakeDownloadUrlGenerator)),
            audit_sink: Some(Arc::new(FakeAuditSink::new())),
        };

        Router::new()
            .route(
                "/v1/activities/{activityId}/exports",
                post(post_request_export),
            )
            .route("/v1/exports/{exportId}", get(get_export_status))
            .route("/v1/exports/{exportId}/download", get(get_export_download))
            .with_state(state)
    }

    fn auth_header() -> (String, String) {
        let user_id = Uuid::new_v4();
        ("Authorization".to_string(), format!("Bearer {user_id}"))
    }

    fn auth_header_for(user_id: Uuid) -> (String, String) {
        ("Authorization".to_string(), format!("Bearer {user_id}"))
    }

    /// Extract the response body as a JSON value.
    async fn response_json(response: axum::http::Response<Body>) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// Assert that a JSON value matches the ProblemDetail schema.
    fn assert_problem_detail(json: &serde_json::Value, expected_status: u16) {
        let problem_type = json["type"]
            .as_str()
            .expect("ProblemDetail must have 'type' as string");
        assert!(
            problem_type.starts_with("/problems/"),
            "type must start with /problems/, got: {problem_type}"
        );

        let title = json["title"]
            .as_str()
            .expect("ProblemDetail must have 'title' as string");
        assert!(!title.is_empty(), "title must not be empty");

        let status = json["status"]
            .as_u64()
            .expect("ProblemDetail must have 'status' as integer");
        assert_eq!(status, expected_status as u64);

        let code = json["code"]
            .as_str()
            .expect("ProblemDetail must have 'code' as string");
        assert!(!code.is_empty(), "code must not be empty");
        assert!(
            code.chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
            "code must be UPPER_SNAKE_CASE, got: {code}"
        );

        json["detail"]
            .as_str()
            .expect("ProblemDetail must have 'detail' as string");

        let request_id = json["requestId"]
            .as_str()
            .expect("ProblemDetail must have 'requestId' as string");
        assert!(
            Uuid::parse_str(request_id).is_ok(),
            "requestId must be a valid UUID, got: {request_id}"
        );
    }

    // ===== POST /v1/activities/{activityId}/exports tests =====

    #[tokio::test]
    async fn post_request_export_returns_202() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let activity_id = Uuid::new_v4();

        let body = serde_json::json!({
            "routeVersionId": Uuid::new_v4().to_string(),
            "format": "gpx"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "test-export-key-1")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let json = response_json(response).await;
        assert!(json["exportId"].is_string());
        assert!(Uuid::parse_str(json["exportId"].as_str().unwrap()).is_ok());
        assert_eq!(json["status"], "queued");
    }

    #[tokio::test]
    async fn post_request_export_missing_idempotency_key_returns_400() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let activity_id = Uuid::new_v4();

        let body = serde_json::json!({
            "routeVersionId": Uuid::new_v4().to_string(),
            "format": "gpx"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_problem_detail(&json, 400);
        assert_eq!(json["code"], "MISSING_IDEMPOTENCY_KEY");
    }

    #[tokio::test]
    async fn post_request_export_without_auth_returns_401() {
        let app = test_app();
        let activity_id = Uuid::new_v4();

        let body = serde_json::json!({
            "routeVersionId": Uuid::new_v4().to_string(),
            "format": "gpx"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header("content-type", "application/json")
                    .header("idempotency-key", "test-noauth")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json = response_json(response).await;
        assert_problem_detail(&json, 401);
        assert_eq!(json["code"], "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn post_request_export_idempotency_replay_returns_same_export() {
        let state = ExportAppState {
            repo: Arc::new(InMemoryExportRepository::new()),
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: None,
            audit_sink: None,
        };

        let app = Router::new()
            .route(
                "/v1/activities/{activityId}/exports",
                post(post_request_export),
            )
            .with_state(state);

        let user_id = Uuid::new_v4();
        let (auth_key, auth_val) = auth_header_for(user_id);
        let activity_id = Uuid::new_v4();
        let route_version_id = Uuid::new_v4();

        let body = serde_json::json!({
            "routeVersionId": route_version_id.to_string(),
            "format": "gpx"
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        // First request
        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "replay-key")
                    .body(Body::from(body_bytes.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::ACCEPTED);
        let json1 = response_json(response1).await;

        // Second request with same key and payload
        let response2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "replay-key")
                    .body(Body::from(body_bytes))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::ACCEPTED);
        let json2 = response_json(response2).await;

        assert_eq!(json1["exportId"], json2["exportId"]);
    }

    #[tokio::test]
    async fn post_request_export_idempotency_mismatch_returns_409() {
        let state = ExportAppState {
            repo: Arc::new(InMemoryExportRepository::new()),
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: None,
            audit_sink: None,
        };

        let app = Router::new()
            .route(
                "/v1/activities/{activityId}/exports",
                post(post_request_export),
            )
            .with_state(state);

        let user_id = Uuid::new_v4();
        let (auth_key, auth_val) = auth_header_for(user_id);
        let activity_id = Uuid::new_v4();

        let body1 = serde_json::json!({
            "routeVersionId": Uuid::new_v4().to_string(),
            "format": "gpx"
        });

        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "mismatch-key")
                    .body(Body::from(serde_json::to_vec(&body1).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::ACCEPTED);

        // Second request with different routeVersionId
        let body2 = serde_json::json!({
            "routeVersionId": Uuid::new_v4().to_string(),
            "format": "gpx"
        });

        let response2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "mismatch-key")
                    .body(Body::from(serde_json::to_vec(&body2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::CONFLICT);
        let json = response_json(response2).await;
        assert_problem_detail(&json, 409);
        assert_eq!(json["code"], "IDEMPOTENCY_PAYLOAD_MISMATCH");
    }

    // ===== GET /v1/exports/{exportId} tests =====

    #[tokio::test]
    async fn get_export_status_returns_200() {
        let state = ExportAppState {
            repo: Arc::new(InMemoryExportRepository::new()),
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: None,
            audit_sink: None,
        };

        let app = Router::new()
            .route(
                "/v1/activities/{activityId}/exports",
                post(post_request_export),
            )
            .route("/v1/exports/{exportId}", get(get_export_status))
            .with_state(state);

        let user_id = Uuid::new_v4();
        let (auth_key, auth_val) = auth_header_for(user_id);
        let activity_id = Uuid::new_v4();

        let body = serde_json::json!({
            "routeVersionId": Uuid::new_v4().to_string(),
            "format": "gpx"
        });

        // Create an export
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "get-test-key")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let create_json = response_json(response).await;
        let export_id = create_json["exportId"].as_str().unwrap();

        // Get the export status
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{export_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["id"], export_id);
        assert_eq!(json["status"], "queued");
        assert_eq!(json["format"], "gpx");
        assert!(json["routeVersionId"].is_string());
        assert_eq!(json["activityId"], activity_id.to_string());
        assert!(json["createdAt"].is_string());
        assert!(json["updatedAt"].is_string());
        // Optional fields should be absent for queued export
        assert!(json.get("failureReason").is_none() || json["failureReason"].is_null());
        assert!(json.get("checksum").is_none() || json["checksum"].is_null());
        assert!(json.get("expiresAt").is_none() || json["expiresAt"].is_null());
    }

    #[tokio::test]
    async fn get_export_not_found_returns_404() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let random_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{random_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_problem_detail(&json, 404);
        assert_eq!(json["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn get_export_wrong_owner_returns_404() {
        let state = ExportAppState {
            repo: Arc::new(InMemoryExportRepository::new()),
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: None,
            audit_sink: None,
        };

        let app = Router::new()
            .route(
                "/v1/activities/{activityId}/exports",
                post(post_request_export),
            )
            .route("/v1/exports/{exportId}", get(get_export_status))
            .with_state(state);

        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();
        let activity_id = Uuid::new_v4();

        let body = serde_json::json!({
            "routeVersionId": Uuid::new_v4().to_string(),
            "format": "gpx"
        });

        // Create export as user1
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/activities/{activity_id}/exports"))
                    .header("Authorization", format!("Bearer {user1}"))
                    .header("content-type", "application/json")
                    .header("idempotency-key", "owner-test-key")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let create_json = response_json(response).await;
        let export_id = create_json["exportId"].as_str().unwrap();

        // GET as user2 should return 404 (not 403)
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{export_id}"))
                    .header("Authorization", format!("Bearer {user2}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_problem_detail(&json, 404);
        assert_eq!(json["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn get_failed_export_has_sanitized_failure_reason() {
        let repo = Arc::new(InMemoryExportRepository::new());

        // Create a failed export with internal error details
        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner_id,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "fail-key".to_string(),
            None,
        )
        .unwrap();
        export_job
            .fail("sqlx error: connection refused at /var/lib/postgresql".to_string())
            .unwrap();

        repo.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: None,
            audit_sink: None,
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}", get(get_export_status))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["status"], "failed");
        let failure_reason = json["failureReason"].as_str().unwrap();
        assert_eq!(failure_reason, "an internal error occurred");
        assert!(!failure_reason.contains("sqlx"));
        assert!(!failure_reason.contains("postgresql"));
    }

    #[tokio::test]
    async fn get_failed_export_passes_through_safe_failure_reason() {
        let repo = Arc::new(InMemoryExportRepository::new());

        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner_id,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "safe-fail-key".to_string(),
            None,
        )
        .unwrap();
        export_job
            .fail("generation failed: route has no coordinates".to_string())
            .unwrap();

        repo.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: None,
            audit_sink: None,
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}", get(get_export_status))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(
            json["failureReason"],
            "generation failed: route has no coordinates"
        );
    }

    // ===== Sanitize failure reason unit tests =====

    #[test]
    fn sanitize_passes_through_known_safe_reasons() {
        let safe_reasons = vec![
            "generation failed: timeout",
            "format not supported: kml",
            "route version unavailable: deleted",
            "export file corrupted: checksum mismatch",
        ];

        for reason in safe_reasons {
            assert_eq!(
                sanitize_failure_reason(reason),
                reason,
                "Expected safe reason to pass through: {reason}"
            );
        }
    }

    #[test]
    fn sanitize_replaces_internal_errors() {
        let internal_reasons = vec![
            "sqlx error: connection refused",
            "IO error: /tmp/exports/file.gpx: permission denied",
            "thread 'main' panicked at src/exports.rs:42",
            "connection pool timeout: postgres://user:pass@host:5432/db",
        ];

        for reason in internal_reasons {
            assert_eq!(
                sanitize_failure_reason(reason),
                "an internal error occurred",
                "Internal reason should be sanitized: {reason}"
            );
        }
    }

    #[tokio::test]
    async fn get_ready_export_does_not_include_object_storage_key() {
        let repo = Arc::new(InMemoryExportRepository::new());

        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner_id,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "ready-key-test".to_string(),
            None,
        )
        .unwrap();
        export_job.start_generating().unwrap();
        let expires = chrono::Utc::now() + chrono::Duration::hours(24);
        export_job
            .complete(
                "exports/private/secret-key.gpx".to_string(),
                "abc123def456".to_string(),
                expires,
            )
            .unwrap();

        repo.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: None,
            audit_sink: None,
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}", get(get_export_status))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;

        // Verify the response has expected Ready fields
        assert_eq!(json["status"], "ready");
        assert_eq!(json["checksum"], "abc123def456");
        assert!(json["expiresAt"].is_string());

        // CRITICAL: objectStorageKey must NOT be present in the response
        assert!(
            json.get("objectStorageKey").is_none(),
            "objectStorageKey must not be exposed in the API response"
        );
        assert!(
            json.get("object_storage_key").is_none(),
            "object_storage_key must not be exposed in the API response"
        );

        // Also verify the entire JSON does not contain the storage key value
        let json_str = serde_json::to_string(&json).unwrap();
        assert!(
            !json_str.contains("exports/private/secret-key.gpx"),
            "storage key value must not appear anywhere in the response"
        );
    }

    // ===== GET /v1/exports/{exportId}/download tests =====

    /// Helper to create a ready export in the given repo and return (export_job, owner_id).
    fn create_ready_export(
        repo: &InMemoryExportRepository,
        storage_key: &str,
    ) -> (ExportJob, haiker_app::identity::UserId) {
        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner_id,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            format!("download-test-{}", Uuid::new_v4()),
            None,
        )
        .unwrap();
        export_job.start_generating().unwrap();
        let expires = chrono::Utc::now() + chrono::Duration::hours(24);
        export_job
            .complete(
                storage_key.to_string(),
                "sha256-abcdef123456".to_string(),
                expires,
            )
            .unwrap();

        repo.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());

        (export_job, owner_id)
    }

    #[tokio::test]
    async fn download_export_happy_path_returns_200_with_expected_fields() {
        let repo = Arc::new(InMemoryExportRepository::new());
        let audit_sink = Arc::new(FakeAuditSink::new());

        let (export_job, owner_id) = create_ready_export(&repo, "exports/user/my-export.gpx");

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: Some(Arc::new(FakeDownloadUrlGenerator)),
            audit_sink: Some(audit_sink.clone()),
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}/download", get(get_export_download))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}/download", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify Cache-Control: no-store header
        let cache_control = response
            .headers()
            .get("cache-control")
            .expect("Cache-Control header must be present")
            .to_str()
            .unwrap();
        assert_eq!(cache_control, "no-store");

        let json = response_json(response).await;

        // Verify all expected fields
        assert_eq!(
            json["downloadUrl"],
            "https://storage.example.com/presigned-download-url?token=abc123"
        );
        assert_eq!(json["filename"], "export.gpx");
        assert!(json["expiresAt"].is_string());
        assert_eq!(json["checksum"], "sha256-abcdef123456");
        assert_eq!(json["contentType"], "application/gpx+xml");

        // Verify audit was recorded
        let records = audit_sink.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, owner_id.0);
        assert_eq!(records[0].1, "export.download");
        assert_eq!(records[0].2, "export_job");
        assert_eq!(records[0].3, export_job.id.0.to_string());
    }

    #[tokio::test]
    async fn download_export_cross_owner_returns_404() {
        let repo = Arc::new(InMemoryExportRepository::new());
        let (export_job, _owner_id) = create_ready_export(&repo, "exports/user/file.gpx");

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: Some(Arc::new(FakeDownloadUrlGenerator)),
            audit_sink: None,
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}/download", get(get_export_download))
            .with_state(state);

        let other_user = Uuid::new_v4();
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}/download", export_job.id.0))
                    .header("Authorization", format!("Bearer {other_user}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_problem_detail(&json, 404);
        assert_eq!(json["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn download_export_non_ready_returns_409() {
        let repo = Arc::new(InMemoryExportRepository::new());

        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        // Create a queued (non-ready) export
        let export_job = ExportJob::new(
            owner_id,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "queued-download-test".to_string(),
            None,
        )
        .unwrap();

        repo.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: Some(Arc::new(FakeDownloadUrlGenerator)),
            audit_sink: None,
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}/download", get(get_export_download))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}/download", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let json = response_json(response).await;
        assert_problem_detail(&json, 409);
        assert_eq!(json["code"], "EXPORT_NOT_READY");
    }

    #[tokio::test]
    async fn download_export_expired_returns_410() {
        let repo = Arc::new(InMemoryExportRepository::new());

        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::new(Uuid::new_v4());
        let route_version_id = RouteVersionId::new(Uuid::new_v4());

        let mut export_job = ExportJob::new(
            owner_id,
            activity_id,
            route_version_id,
            ExportFormat::Gpx,
            "expired-download-test".to_string(),
            None,
        )
        .unwrap();
        export_job.start_generating().unwrap();
        // Set expires_at to the past
        let expired_at = chrono::Utc::now() - chrono::Duration::hours(1);
        export_job
            .complete(
                "exports/user/expired.gpx".to_string(),
                "hash".to_string(),
                expired_at,
            )
            .unwrap();

        repo.exports
            .lock()
            .unwrap()
            .insert(export_job.id, export_job.clone());

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: Some(Arc::new(FakeDownloadUrlGenerator)),
            audit_sink: None,
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}/download", get(get_export_download))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}/download", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::GONE);
        let json = response_json(response).await;
        assert_problem_detail(&json, 410);
        assert_eq!(json["code"], "ARTIFACT_EXPIRED");
    }

    #[tokio::test]
    async fn download_export_unauthenticated_returns_401() {
        let app = test_app();
        let random_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{random_id}/download"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json = response_json(response).await;
        assert_problem_detail(&json, 401);
        assert_eq!(json["code"], "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn get_export_status_without_auth_returns_401() {
        let app = test_app();
        let random_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{random_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json = response_json(response).await;
        assert_problem_detail(&json, 401);
        assert_eq!(json["code"], "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn download_export_not_found_returns_404() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let random_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{random_id}/download"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_problem_detail(&json, 404);
        assert_eq!(json["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn download_export_response_does_not_contain_object_storage_key() {
        let repo = Arc::new(InMemoryExportRepository::new());
        let storage_key = "exports/private/secret-internal-key.gpx";
        let (export_job, owner_id) = create_ready_export(&repo, storage_key);

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: Some(Arc::new(FakeDownloadUrlGenerator)),
            audit_sink: None,
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}/download", get(get_export_download))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}/download", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;

        // The raw object_storage_key must NEVER appear in the response
        let json_str = serde_json::to_string(&json).unwrap();
        assert!(
            !json_str.contains(storage_key),
            "raw object_storage_key must not appear in the download response"
        );
        assert!(
            json.get("objectStorageKey").is_none(),
            "objectStorageKey field must not exist"
        );
        assert!(
            json.get("object_storage_key").is_none(),
            "object_storage_key field must not exist"
        );
    }

    #[tokio::test]
    async fn download_export_audit_does_not_log_url() {
        let repo = Arc::new(InMemoryExportRepository::new());
        let audit_sink = Arc::new(FakeAuditSink::new());
        let (export_job, owner_id) = create_ready_export(&repo, "exports/user/audit-test.gpx");

        let state = ExportAppState {
            repo,
            route_version_gateway: Arc::new(StubRouteVersionGateway),
            job_queue: None,
            download_url_generator: Some(Arc::new(FakeDownloadUrlGenerator)),
            audit_sink: Some(audit_sink.clone()),
        };

        let app = Router::new()
            .route("/v1/exports/{exportId}/download", get(get_export_download))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/exports/{}/download", export_job.id.0))
                    .header("Authorization", format!("Bearer {}", owner_id.0))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify audit record exists and does NOT contain the presigned URL
        let records = audit_sink.records();
        assert_eq!(records.len(), 1);
        let (actor, action, resource_type, resource_id) = &records[0];
        assert_eq!(*actor, owner_id.0);
        assert_eq!(action, "export.download");
        assert_eq!(resource_type, "export_job");
        assert_eq!(*resource_id, export_job.id.0.to_string());
        // The audit record fields should not contain a URL
        assert!(!resource_id.contains("https://"));
        assert!(!action.contains("https://"));
    }

    // ===== sanitize_filename unit tests =====

    #[test]
    fn sanitize_filename_basic() {
        assert_eq!(sanitize_filename("Morning-Hike", "gpx"), "Morning-Hike.gpx");
    }

    #[test]
    fn sanitize_filename_special_chars() {
        assert_eq!(
            sanitize_filename("My Hike! @#$% 2024", "gpx"),
            "My-Hike-2024.gpx"
        );
    }

    #[test]
    fn sanitize_filename_empty_defaults() {
        assert_eq!(sanitize_filename("", "gpx"), "export.gpx");
    }

    #[test]
    fn sanitize_filename_only_special_chars() {
        assert_eq!(sanitize_filename("!!!@@@", "gpx"), "export.gpx");
    }

    #[test]
    fn sanitize_filename_truncates_long_names() {
        let long_name = "a".repeat(200);
        let result = sanitize_filename(&long_name, "gpx");
        assert!(result.len() <= 100);
        assert!(result.ends_with(".gpx"));
    }

    #[test]
    fn sanitize_filename_collapses_hyphens() {
        assert_eq!(sanitize_filename("a   b   c", "gpx"), "a-b-c.gpx");
    }
}
