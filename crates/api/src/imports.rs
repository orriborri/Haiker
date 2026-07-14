//! Import API handlers.
//!
//! Implements POST /v1/imports, POST /v1/imports/:id/completion, GET /v1/imports/:id.

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::identity::UserId;
use haiker_app::imports::commands::{
    handle_complete_upload, handle_get_import, handle_start_import, CompleteUploadCommand,
    StartImportCommand, UploadUrlGenerator, UploadVerifier,
};
use haiker_app::imports::job_types::ParseGpxJob;
use haiker_app::imports::repository::ImportRepository;
use haiker_app::imports::{Import, ImportError, ImportId};

use crate::auth::AuthenticatedActor;
use crate::error::ApiError;
use crate::imports_dto::{
    CompleteUploadRequest, ImportStatusResponse, StartImportRequest, StartImportResponse,
};

/// Trait for enqueueing background jobs from the API layer.
///
/// This abstracts the job queue infrastructure so the API handlers
/// do not depend directly on the platform crate's JobQueue.
#[async_trait]
pub trait JobEnqueuer: Send + Sync {
    /// Enqueue a job with the given type and JSON payload.
    async fn enqueue(
        &self,
        job_type: &str,
        payload: serde_json::Value,
        correlation_id: Uuid,
    ) -> Result<Uuid, String>;
}

/// Shared application state for import handlers.
#[derive(Clone)]
pub struct ImportAppState {
    pub repo: Arc<dyn ImportRepository>,
    pub url_generator: Arc<dyn UploadUrlGenerator>,
    pub upload_verifier: Arc<dyn UploadVerifier>,
    pub job_queue: Option<Arc<dyn JobEnqueuer>>,
}

/// Convert an Import domain model to the API response DTO.
fn import_to_status_response(import: &Import) -> ImportStatusResponse {
    ImportStatusResponse {
        id: import.id.0,
        status: import.status.to_string(),
        failure_reason: import
            .failure_reason
            .as_deref()
            .map(sanitize_failure_reason),
        activity_id: None, // Activity ID populated after parsing completes
        created_at: import.created_at,
        updated_at: import.updated_at,
    }
}

/// Known client-safe failure reason prefixes that can be exposed to users.
const SAFE_FAILURE_REASONS: &[&str] = &[
    "unsupported file format",
    "parsing failed",
    "file too large",
    "invalid media type",
    "checksum mismatch",
    "duplicate checksum",
    "upload too large",
    "validation failed",
];

/// Sanitize a failure reason before exposing it in API responses.
///
/// Only known safe failure reasons are passed through to the client.
/// Any failure reason that does not match a known safe prefix is replaced
/// with a generic message to prevent leaking internal details such as SQL
/// errors, file paths, or connection strings.
fn sanitize_failure_reason(reason: &str) -> String {
    let lower = reason.to_lowercase();
    for safe in SAFE_FAILURE_REASONS {
        if lower.starts_with(safe) {
            return reason.to_string();
        }
    }
    "an internal error occurred".to_string()
}

/// Convert an ImportError to an ApiError.
fn import_error_to_api_error(err: ImportError) -> ApiError {
    match err {
        ImportError::NotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "import not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::Unauthorized => ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this import".to_string(),
            problem_type: Some("/problems/forbidden".to_string()),
            title: Some("Forbidden".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::UploadTooLarge => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "UPLOAD_TOO_LARGE".to_string(),
            message: "file size exceeds the 50MB limit".to_string(),
            problem_type: Some("/problems/upload-too-large".to_string()),
            title: Some("Upload Too Large".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::InvalidMediaType => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_MEDIA_TYPE".to_string(),
            message: "content type must be application/gpx+xml or application/xml".to_string(),
            problem_type: Some("/problems/invalid-media-type".to_string()),
            title: Some("Invalid Media Type".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::InvalidTransition { from, to } => ApiError {
            status: StatusCode::CONFLICT,
            code: "INVALID_STATE_TRANSITION".to_string(),
            message: format!("cannot transition from {from} to {to}"),
            problem_type: Some("/problems/invalid-state-transition".to_string()),
            title: Some("Invalid State Transition".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::DuplicateIdempotencyKey => ApiError {
            status: StatusCode::CONFLICT,
            code: "DUPLICATE_IDEMPOTENCY_KEY".to_string(),
            message: "an import with this idempotency key already exists".to_string(),
            problem_type: Some("/problems/duplicate-idempotency-key".to_string()),
            title: Some("Duplicate Idempotency Key".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::IdempotencyPayloadMismatch => ApiError {
            status: StatusCode::CONFLICT,
            code: "IDEMPOTENCY_PAYLOAD_MISMATCH".to_string(),
            message: "idempotency key reused with different payload".to_string(),
            problem_type: Some("/problems/idempotency-conflict".to_string()),
            title: Some("Idempotency Conflict".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::ValidationFailed { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "VALIDATION_FAILED".to_string(),
            message,
            problem_type: Some("/problems/validation-failed".to_string()),
            title: Some("Validation Failed".to_string()),
            request_id: None,
            details: None,
        },
        ImportError::StorageError { message } => {
            tracing::error!(error = %message, "storage error during import operation");
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "STORAGE_ERROR".to_string(),
                message: "a storage error occurred".to_string(),
                problem_type: Some("/problems/storage-error".to_string()),
                title: Some("Storage Error".to_string()),
                request_id: None,
                details: None,
            }
        }
        _ => {
            tracing::error!(error = %err, "unexpected error during import operation");
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

/// POST /v1/imports
///
/// Start a new import. Requires Idempotency-Key header.
/// Returns 202 Accepted with the import ID and a presigned upload URL.
pub async fn post_start_import(
    State(state): State<ImportAppState>,
    actor: AuthenticatedActor,
    headers: HeaderMap,
    Json(body): Json<StartImportRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let idempotency_key = extract_idempotency_key(&headers)?;

    let cmd = StartImportCommand {
        owner_id: actor.0.user_id,
        idempotency_key,
        filename: body.filename,
        content_type: body.content_type,
        file_size_bytes: body.file_size_bytes,
    };

    let result = handle_start_import(cmd, state.repo.as_ref(), state.url_generator.as_ref())
        .await
        .map_err(import_error_to_api_error)?;

    let response = StartImportResponse {
        import_id: result.import.id.0,
        upload_url: result.upload_url,
        status: result.import.status.to_string(),
    };

    Ok((StatusCode::ACCEPTED, Json(response)))
}

/// POST /v1/imports/:import_id/completion
///
/// Finalize the upload by providing the file checksum.
/// Transitions the import from Uploading to Uploaded, then enqueues
/// a background job for GPX parsing.
pub async fn post_complete_upload(
    State(state): State<ImportAppState>,
    actor: AuthenticatedActor,
    Path(import_id): Path<Uuid>,
    Json(body): Json<CompleteUploadRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let cmd = CompleteUploadCommand {
        import_id: ImportId::new(import_id),
        owner_id: actor.0.user_id,
        checksum: body.checksum,
    };

    let import = handle_complete_upload(cmd, state.repo.as_ref(), state.upload_verifier.as_ref())
        .await
        .map_err(import_error_to_api_error)?;

    // Enqueue parse_gpx job for async processing
    if let Some(ref job_queue) = state.job_queue {
        let storage_key = format!("imports/{}/{}", import.owner_id, import.id);
        let correlation_id = Uuid::new_v4();

        let job_payload = ParseGpxJob {
            import_id: import.id.0,
            owner_id: import.owner_id.0,
            object_storage_key: storage_key,
            correlation_id,
        };

        let payload_json = serde_json::to_value(&job_payload).map_err(|e| {
            tracing::error!("failed to serialize job payload: {e}");
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
            .enqueue(
                haiker_app::imports::job_types::PARSE_GPX_JOB_TYPE,
                payload_json,
                correlation_id,
            )
            .await
            .map_err(|e| {
                tracing::error!("failed to enqueue parsing job: {e}");
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

    Ok((StatusCode::OK, Json(import_to_status_response(&import))))
}

/// GET /v1/imports/:import_id
///
/// Get the current status of an import.
pub async fn get_import_status(
    State(state): State<ImportAppState>,
    actor: AuthenticatedActor,
    Path(import_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id: UserId = actor.0.user_id;

    let import = handle_get_import(ImportId::new(import_id), owner_id, state.repo.as_ref())
        .await
        .map_err(import_error_to_api_error)?;

    Ok((StatusCode::OK, Json(import_to_status_response(&import))))
}

// -- In-memory implementations for use in tests --

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

/// In-memory import repository for testing (not used in production).
#[cfg(test)]
pub struct InMemoryImportRepository {
    imports: Mutex<HashMap<ImportId, Import>>,
}

#[cfg(test)]
impl InMemoryImportRepository {
    pub fn new() -> Self {
        Self {
            imports: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl ImportRepository for InMemoryImportRepository {
    async fn save(&self, import: &Import) -> Result<(), ImportError> {
        self.imports
            .lock()
            .unwrap()
            .insert(import.id, import.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: ImportId) -> Result<Option<Import>, ImportError> {
        Ok(self.imports.lock().unwrap().get(&id).cloned())
    }

    async fn find_by_idempotency_key(
        &self,
        owner_id: UserId,
        key: &str,
    ) -> Result<Option<Import>, ImportError> {
        Ok(self
            .imports
            .lock()
            .unwrap()
            .values()
            .find(|i| i.owner_id == owner_id && i.idempotency_key == key)
            .cloned())
    }

    async fn find_by_checksum(
        &self,
        _owner_id: UserId,
        _checksum: &haiker_app::imports::checksum::Checksum,
    ) -> Result<Option<Import>, ImportError> {
        Ok(None)
    }

    async fn update(&self, import: &Import) -> Result<(), ImportError> {
        self.imports
            .lock()
            .unwrap()
            .insert(import.id, import.clone());
        Ok(())
    }
}

/// Stub URL generator for testing (not used in production).
#[cfg(test)]
pub struct StubUrlGenerator;

#[cfg(test)]
#[async_trait]
impl UploadUrlGenerator for StubUrlGenerator {
    async fn generate_upload_url(&self, key: &str) -> Result<String, ImportError> {
        Ok(format!(
            "https://storage.placeholder.local/{key}?X-Amz-Signature=stub"
        ))
    }
}

/// Stub upload verifier for testing that returns valid metadata by default.
#[cfg(test)]
pub struct StubUploadVerifier;

#[cfg(test)]
#[async_trait]
impl UploadVerifier for StubUploadVerifier {
    async fn verify_upload(
        &self,
        _key: &str,
    ) -> Result<haiker_app::imports::commands::UploadMetadata, ImportError> {
        Ok(haiker_app::imports::commands::UploadMetadata {
            content_length: 1024,
            content_type: Some("application/gpx+xml".to_string()),
        })
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
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        Router::new()
            .route("/v1/imports", post(post_start_import))
            .route(
                "/v1/imports/{import_id}/completion",
                post(post_complete_upload),
            )
            .route("/v1/imports/{import_id}", get(get_import_status))
            .with_state(state)
    }

    fn auth_header() -> (String, String) {
        let user_id = Uuid::new_v4();
        ("Authorization".to_string(), format!("Bearer {user_id}"))
    }

    /// Extract the response body as a JSON value.
    async fn response_json(response: axum::http::Response<Body>) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// Assert that a JSON value matches the ProblemDetail schema with the expected status code.
    ///
    /// Validates:
    /// - `type` is a string starting with `/problems/`
    /// - `title` is a non-empty string
    /// - `status` is an integer matching `expected_status`
    /// - `code` is a non-empty UPPER_SNAKE_CASE string
    /// - `detail` is a string
    /// - `requestId` is a valid UUID string
    fn assert_problem_detail(json: &serde_json::Value, expected_status: u16) {
        // type: string starting with /problems/
        let problem_type = json["type"]
            .as_str()
            .expect("ProblemDetail must have 'type' as string");
        assert!(
            problem_type.starts_with("/problems/"),
            "type must start with /problems/, got: {problem_type}"
        );

        // title: non-empty string
        let title = json["title"]
            .as_str()
            .expect("ProblemDetail must have 'title' as string");
        assert!(!title.is_empty(), "title must not be empty");

        // status: integer matching expected
        let status = json["status"]
            .as_u64()
            .expect("ProblemDetail must have 'status' as integer");
        assert_eq!(
            status, expected_status as u64,
            "status must match expected: {expected_status}"
        );

        // code: non-empty UPPER_SNAKE_CASE string
        let code = json["code"]
            .as_str()
            .expect("ProblemDetail must have 'code' as string");
        assert!(!code.is_empty(), "code must not be empty");
        assert!(
            code.chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
            "code must be UPPER_SNAKE_CASE, got: {code}"
        );

        // detail: string (may be empty)
        json["detail"]
            .as_str()
            .expect("ProblemDetail must have 'detail' as string");

        // requestId: valid UUID string
        let request_id = json["requestId"]
            .as_str()
            .expect("ProblemDetail must have 'requestId' as string");
        assert!(
            Uuid::parse_str(request_id).is_ok(),
            "requestId must be a valid UUID, got: {request_id}"
        );
    }

    #[tokio::test]
    async fn start_import_returns_202() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "test-key-1")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["importId"].is_string());
        assert!(json["uploadUrl"].is_string());
        assert_eq!(json["status"], "uploading");
    }

    #[tokio::test]
    async fn start_import_missing_idempotency_key_returns_400() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn start_import_file_too_large_returns_422() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "big.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 50 * 1024 * 1024 + 1
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "test-key-big")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn start_import_invalid_content_type_returns_422() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "data.json",
            "contentType": "application/json",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "test-key-ct")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn start_import_without_auth_returns_401() {
        let app = test_app();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("content-type", "application/json")
                    .header("idempotency-key", "test-key-noauth")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_import_not_found_returns_404() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let random_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/imports/{random_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn complete_upload_on_nonexistent_import_returns_404() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let random_id = Uuid::new_v4();

        let body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{random_id}/completion"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn idempotency_key_replay_returns_same_import() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .with_state(state);

        let user_id = Uuid::new_v4();
        let auth_val = format!("Bearer {user_id}");

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        // First request
        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "replay-key")
                    .body(Body::from(body_bytes.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::ACCEPTED);
        let b1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
            .await
            .unwrap();
        let json1: serde_json::Value = serde_json::from_slice(&b1).unwrap();

        // Second request with same key
        let response2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "replay-key")
                    .body(Body::from(body_bytes))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::ACCEPTED);
        let b2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
            .await
            .unwrap();
        let json2: serde_json::Value = serde_json::from_slice(&b2).unwrap();

        assert_eq!(json1["importId"], json2["importId"]);
    }

    #[tokio::test]
    async fn full_flow_start_then_complete_then_get() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .route(
                "/v1/imports/{import_id}/completion",
                post(post_complete_upload),
            )
            .route("/v1/imports/{import_id}", get(get_import_status))
            .with_state(state);

        let user_id = Uuid::new_v4();
        let auth_val = format!("Bearer {user_id}");

        // Start import
        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 2048
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "flow-key")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let b = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let start_json: serde_json::Value = serde_json::from_slice(&b).unwrap();
        let import_id = start_json["importId"].as_str().unwrap();

        // Complete upload
        let complete_body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{import_id}/completion"))
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&complete_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let b = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let complete_json: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(complete_json["status"], "uploaded");

        // Get status
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/imports/{import_id}"))
                    .header("Authorization", &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let b = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let get_json: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(get_json["status"], "uploaded");
        assert_eq!(get_json["id"], import_id);
    }

    #[tokio::test]
    async fn complete_upload_wrong_owner_returns_403() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .route(
                "/v1/imports/{import_id}/completion",
                post(post_complete_upload),
            )
            .with_state(state);

        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        // Start import as user1
        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", format!("Bearer {user1}"))
                    .header("content-type", "application/json")
                    .header("idempotency-key", "owner-key")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let b = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
        let import_id = json["importId"].as_str().unwrap();

        // Complete as user2
        let complete_body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{import_id}/completion"))
                    .header("Authorization", format!("Bearer {user2}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&complete_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn sanitize_failure_reason_passes_through_known_safe_reasons() {
        let safe_reasons = vec![
            "unsupported file format",
            "parsing failed: invalid GPX structure",
            "file too large",
            "invalid media type",
            "checksum mismatch",
            "duplicate checksum",
            "upload too large",
            "validation failed: missing required field",
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
    fn sanitize_failure_reason_replaces_sql_errors_with_generic_message() {
        let internal_reason = "sqlx error: connection refused at /var/lib/postgresql/data";
        assert_eq!(
            sanitize_failure_reason(internal_reason),
            "an internal error occurred"
        );
    }

    #[test]
    fn sanitize_failure_reason_replaces_file_path_errors_with_generic_message() {
        let internal_reason = "IO error: /tmp/imports/abc123/file.gpx: permission denied";
        assert_eq!(
            sanitize_failure_reason(internal_reason),
            "an internal error occurred"
        );
    }

    #[test]
    fn sanitize_failure_reason_replaces_connection_string_errors() {
        let internal_reason = "connection pool timeout: postgres://user:pass@host:5432/db";
        assert_eq!(
            sanitize_failure_reason(internal_reason),
            "an internal error occurred"
        );
    }

    #[test]
    fn sanitize_failure_reason_replaces_stack_traces() {
        let internal_reason = "thread 'main' panicked at src/imports/orchestrator.rs:42";
        assert_eq!(
            sanitize_failure_reason(internal_reason),
            "an internal error occurred"
        );
    }

    #[test]
    fn import_status_response_does_not_expose_internal_failure_reason() {
        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let mut import = haiker_app::imports::Import::new(
            owner_id,
            haiker_app::imports::ImportFormat::Gpx,
            "key-sanitize".to_string(),
            None,
        )
        .unwrap();
        import
            .fail("sqlx error: connection refused at /var/lib/db".to_string())
            .unwrap();

        let response = import_to_status_response(&import);
        assert_eq!(
            response.failure_reason.as_deref(),
            Some("an internal error occurred")
        );
    }

    #[test]
    fn import_status_response_preserves_safe_failure_reason() {
        let owner_id = haiker_app::identity::UserId::new(Uuid::new_v4());
        let mut import = haiker_app::imports::Import::new(
            owner_id,
            haiker_app::imports::ImportFormat::Gpx,
            "key-safe".to_string(),
            None,
        )
        .unwrap();
        import
            .fail("parsing failed: invalid GPX root element".to_string())
            .unwrap();

        let response = import_to_status_response(&import);
        assert_eq!(
            response.failure_reason.as_deref(),
            Some("parsing failed: invalid GPX root element")
        );
    }

    #[tokio::test]
    async fn idempotency_key_with_different_payload_returns_409() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .with_state(state);

        let user_id = Uuid::new_v4();
        let auth_val = format!("Bearer {user_id}");

        // First request
        let body1 = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "mismatch-key")
                    .body(Body::from(serde_json::to_vec(&body1).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::ACCEPTED);

        // Second request with different payload
        let body2 = serde_json::json!({
            "filename": "different.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "mismatch-key")
                    .body(Body::from(serde_json::to_vec(&body2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::CONFLICT);

        let b = axum::body::to_bytes(response2.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(json["code"], "IDEMPOTENCY_PAYLOAD_MISMATCH");
    }

    // ===== Contract Tests: POST /v1/imports =====

    #[tokio::test]
    async fn contract_post_imports_202_response_matches_schema() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-202")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let json = response_json(response).await;

        // importId must be a valid UUID
        let import_id = json["importId"]
            .as_str()
            .expect("importId must be a string");
        assert!(
            Uuid::parse_str(import_id).is_ok(),
            "importId must be a valid UUID"
        );

        // uploadUrl must be a non-empty string
        let upload_url = json["uploadUrl"]
            .as_str()
            .expect("uploadUrl must be a string");
        assert!(!upload_url.is_empty(), "uploadUrl must not be empty");

        // status must be "uploading"
        assert_eq!(json["status"], "uploading");
    }

    #[tokio::test]
    async fn contract_post_imports_400_missing_idempotency_key_returns_problem_detail() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
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
    async fn contract_post_imports_401_missing_auth_returns_problem_detail() {
        let app = test_app();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-401")
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
    async fn contract_post_imports_422_file_too_large_returns_problem_detail() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "big.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 50 * 1024 * 1024 + 1
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-422-size")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let json = response_json(response).await;
        assert_problem_detail(&json, 422);
        assert_eq!(json["code"], "UPLOAD_TOO_LARGE");
    }

    #[tokio::test]
    async fn contract_post_imports_422_invalid_content_type_returns_problem_detail() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let body = serde_json::json!({
            "filename": "data.json",
            "contentType": "application/json",
            "fileSizeBytes": 1024
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-422-ct")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let json = response_json(response).await;
        assert_problem_detail(&json, 422);
        assert_eq!(json["code"], "INVALID_MEDIA_TYPE");
    }

    #[tokio::test]
    async fn contract_post_imports_409_payload_mismatch_returns_problem_detail() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .with_state(state);

        let user_id = Uuid::new_v4();
        let auth_val = format!("Bearer {user_id}");

        let body1 = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-409-key")
                    .body(Body::from(serde_json::to_vec(&body1).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::ACCEPTED);

        let body2 = serde_json::json!({
            "filename": "other.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-409-key")
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

    // ===== Contract Tests: POST /v1/imports/:id/completion =====

    #[tokio::test]
    async fn contract_post_completion_200_response_matches_schema() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .route(
                "/v1/imports/{import_id}/completion",
                post(post_complete_upload),
            )
            .with_state(state);

        let user_id = Uuid::new_v4();
        let auth_val = format!("Bearer {user_id}");

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 2048
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-complete-200")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let start_json = response_json(response).await;
        let import_id = start_json["importId"].as_str().unwrap();

        let complete_body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{import_id}/completion"))
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&complete_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;

        // id: valid UUID matching the import
        let id = json["id"].as_str().expect("id must be a string");
        assert_eq!(id, import_id);
        assert!(Uuid::parse_str(id).is_ok(), "id must be a valid UUID");

        // status: string
        let status = json["status"].as_str().expect("status must be a string");
        assert_eq!(status, "uploaded");

        // createdAt: required string (ISO 8601)
        let created_at = json["createdAt"]
            .as_str()
            .expect("createdAt must be a string");
        assert!(!created_at.is_empty(), "createdAt must not be empty");

        // updatedAt: required string (ISO 8601)
        let updated_at = json["updatedAt"]
            .as_str()
            .expect("updatedAt must be a string");
        assert!(!updated_at.is_empty(), "updatedAt must not be empty");

        // failureReason: optional (should be absent for non-failed)
        assert!(
            json.get("failureReason").is_none() || json["failureReason"].is_null(),
            "failureReason should be absent or null for non-failed import"
        );

        // activityId: optional (should be absent before parsing)
        assert!(
            json.get("activityId").is_none() || json["activityId"].is_null(),
            "activityId should be absent or null before parsing"
        );
    }

    #[tokio::test]
    async fn contract_post_completion_401_missing_auth_returns_problem_detail() {
        let app = test_app();
        let random_id = Uuid::new_v4();

        let body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{random_id}/completion"))
                    .header("content-type", "application/json")
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
    async fn contract_post_completion_403_wrong_owner_returns_problem_detail() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .route(
                "/v1/imports/{import_id}/completion",
                post(post_complete_upload),
            )
            .with_state(state);

        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", format!("Bearer {user1}"))
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-403")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let start_json = response_json(response).await;
        let import_id = start_json["importId"].as_str().unwrap();

        let complete_body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{import_id}/completion"))
                    .header("Authorization", format!("Bearer {user2}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&complete_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let json = response_json(response).await;
        assert_problem_detail(&json, 403);
        assert_eq!(json["code"], "FORBIDDEN");
    }

    #[tokio::test]
    async fn contract_post_completion_404_nonexistent_import_returns_problem_detail() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let random_id = Uuid::new_v4();

        let body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{random_id}/completion"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
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
    async fn contract_post_completion_409_invalid_state_returns_problem_detail() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .route(
                "/v1/imports/{import_id}/completion",
                post(post_complete_upload),
            )
            .with_state(state);

        let user_id = Uuid::new_v4();
        let auth_val = format!("Bearer {user_id}");

        // Start and complete the import
        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-409-state")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let start_json = response_json(response).await;
        let import_id = start_json["importId"].as_str().unwrap();

        let complete_body = serde_json::json!({
            "checksum": "a".repeat(64)
        });

        // First completion succeeds
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{import_id}/completion"))
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&complete_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Second completion fails with 409 (already uploaded)
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/imports/{import_id}/completion"))
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&complete_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let json = response_json(response).await;
        assert_problem_detail(&json, 409);
        assert_eq!(json["code"], "INVALID_STATE_TRANSITION");
    }

    // ===== Contract Tests: GET /v1/imports/:id =====

    #[tokio::test]
    async fn contract_get_import_200_response_matches_schema() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .route("/v1/imports/{import_id}", get(get_import_status))
            .with_state(state);

        let user_id = Uuid::new_v4();
        let auth_val = format!("Bearer {user_id}");

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", &auth_val)
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-get-200")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let start_json = response_json(response).await;
        let import_id = start_json["importId"].as_str().unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/imports/{import_id}"))
                    .header("Authorization", &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;

        // id: valid UUID
        let id = json["id"].as_str().expect("id must be a string");
        assert_eq!(id, import_id);
        assert!(Uuid::parse_str(id).is_ok(), "id must be a valid UUID");

        // status: non-empty string
        let status = json["status"].as_str().expect("status must be a string");
        assert!(!status.is_empty(), "status must not be empty");

        // createdAt: required string
        let created_at = json["createdAt"]
            .as_str()
            .expect("createdAt must be a string");
        assert!(!created_at.is_empty());

        // updatedAt: required string
        let updated_at = json["updatedAt"]
            .as_str()
            .expect("updatedAt must be a string");
        assert!(!updated_at.is_empty());

        // failureReason: absent or null for non-failed
        assert!(
            json.get("failureReason").is_none() || json["failureReason"].is_null(),
            "failureReason should be absent for non-failed import"
        );

        // activityId: absent or null
        assert!(
            json.get("activityId").is_none() || json["activityId"].is_null(),
            "activityId should be absent before parsing"
        );
    }

    #[tokio::test]
    async fn contract_get_import_401_missing_auth_returns_problem_detail() {
        let app = test_app();
        let random_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/imports/{random_id}"))
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
    async fn contract_get_import_404_nonexistent_id_returns_problem_detail() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();
        let random_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/imports/{random_id}"))
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
    async fn contract_get_import_404_wrong_owner_returns_problem_detail() {
        let state = ImportAppState {
            repo: Arc::new(InMemoryImportRepository::new()),
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports", post(post_start_import))
            .route("/v1/imports/{import_id}", get(get_import_status))
            .with_state(state);

        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        let body = serde_json::json!({
            "filename": "hike.gpx",
            "contentType": "application/gpx+xml",
            "fileSizeBytes": 1024
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/imports")
                    .header("Authorization", format!("Bearer {user1}"))
                    .header("content-type", "application/json")
                    .header("idempotency-key", "contract-get-404-owner")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let start_json = response_json(response).await;
        let import_id = start_json["importId"].as_str().unwrap();

        // GET as different user returns 404 (not 403)
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/imports/{import_id}"))
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

    // ===== Contract Test: Failed import sanitized failureReason =====

    #[tokio::test]
    async fn contract_get_failed_import_has_sanitized_failure_reason() {
        let repo = Arc::new(InMemoryImportRepository::new());

        // Create an import and manually transition it to failed with internal error
        let owner_id = UserId::new(Uuid::new_v4());
        let mut import = Import::new(
            owner_id,
            haiker_app::imports::ImportFormat::Gpx,
            "contract-fail-key".to_string(),
            None,
        )
        .unwrap();
        import.start_upload().unwrap();
        import
            .fail("sqlx error: connection refused at /var/lib/postgresql/data".to_string())
            .unwrap();

        repo.imports
            .lock()
            .unwrap()
            .insert(import.id, import.clone());

        let state = ImportAppState {
            repo,
            url_generator: Arc::new(StubUrlGenerator),
            upload_verifier: Arc::new(StubUploadVerifier),
            job_queue: None,
        };

        let app = Router::new()
            .route("/v1/imports/{import_id}", get(get_import_status))
            .with_state(state);

        let auth_val = format!("Bearer {}", owner_id.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/imports/{}", import.id.0))
                    .header("Authorization", &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;

        // Verify the response matches the schema
        assert_eq!(json["id"], import.id.0.to_string());
        assert_eq!(json["status"], "failed");

        // The failure reason must be sanitized - no internal details
        let failure_reason = json["failureReason"]
            .as_str()
            .expect("failureReason must be present for failed imports");
        assert_eq!(
            failure_reason, "an internal error occurred",
            "Internal error details must be sanitized"
        );
        assert!(
            !failure_reason.contains("sqlx"),
            "Must not contain internal details"
        );
        assert!(
            !failure_reason.contains("postgresql"),
            "Must not contain infrastructure details"
        );
    }
}
