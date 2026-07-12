//! Import API handlers.
//!
//! Implements POST /v1/imports, POST /v1/imports/:id/completion, GET /v1/imports/:id.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::identity::UserId;
use haiker_app::imports::commands::{
    handle_complete_upload, handle_get_import, handle_start_import, CompleteUploadCommand,
    StartImportCommand, UploadUrlGenerator,
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
    pub job_queue: Option<Arc<dyn JobEnqueuer>>,
}

/// Convert an Import domain model to the API response DTO.
fn import_to_status_response(import: &Import) -> ImportStatusResponse {
    ImportStatusResponse {
        id: import.id.0,
        status: import.status.to_string(),
        failure_reason: import.failure_reason.clone(),
        activity_id: None, // Activity ID populated after parsing completes
        created_at: import.created_at,
        updated_at: import.updated_at,
    }
}

/// Convert an ImportError to an ApiError.
fn import_error_to_api_error(err: ImportError) -> ApiError {
    match err {
        ImportError::NotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "import not found".to_string(),
            details: None,
        },
        ImportError::Unauthorized => ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this import".to_string(),
            details: None,
        },
        ImportError::UploadTooLarge => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "UPLOAD_TOO_LARGE".to_string(),
            message: "file size exceeds the 50MB limit".to_string(),
            details: None,
        },
        ImportError::InvalidMediaType => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_MEDIA_TYPE".to_string(),
            message: "content type must be application/gpx+xml or application/xml".to_string(),
            details: None,
        },
        ImportError::InvalidTransition { from, to } => ApiError {
            status: StatusCode::CONFLICT,
            code: "INVALID_STATE_TRANSITION".to_string(),
            message: format!("cannot transition from {from} to {to}"),
            details: None,
        },
        ImportError::DuplicateIdempotencyKey => ApiError {
            status: StatusCode::CONFLICT,
            code: "DUPLICATE_IDEMPOTENCY_KEY".to_string(),
            message: "an import with this idempotency key already exists".to_string(),
            details: None,
        },
        ImportError::ValidationFailed { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "VALIDATION_FAILED".to_string(),
            message,
            details: None,
        },
        ImportError::StorageError { message } => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "STORAGE_ERROR".to_string(),
            message,
            details: None,
        },
        _ => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: err.to_string(),
            details: None,
        },
    }
}

/// Extract the Idempotency-Key header value.
fn extract_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header is required".to_string(),
            details: None,
        })?;

    if key.trim().is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header must not be empty".to_string(),
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

    let import = handle_complete_upload(cmd, state.repo.as_ref())
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

        let payload_json = serde_json::to_value(&job_payload).map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: format!("failed to serialize job payload: {e}"),
            details: None,
        })?;

        job_queue
            .enqueue(
                haiker_app::imports::job_types::PARSE_GPX_JOB_TYPE,
                payload_json,
                correlation_id,
            )
            .await
            .map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "JOB_ENQUEUE_FAILED".to_string(),
                message: format!("failed to enqueue parsing job: {e}"),
                details: None,
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

// -- In-memory implementations for use in main() stub and tests --

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory import repository (placeholder until persistence layer).
pub struct InMemoryImportRepository {
    imports: Mutex<HashMap<ImportId, Import>>,
}

impl InMemoryImportRepository {
    pub fn new() -> Self {
        Self {
            imports: Mutex::new(HashMap::new()),
        }
    }
}

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

/// Stub URL generator that returns a placeholder URL.
pub struct StubUrlGenerator;

#[async_trait]
impl UploadUrlGenerator for StubUrlGenerator {
    async fn generate_upload_url(&self, key: &str) -> Result<String, ImportError> {
        Ok(format!(
            "https://storage.placeholder.local/{key}?X-Amz-Signature=stub"
        ))
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
}
