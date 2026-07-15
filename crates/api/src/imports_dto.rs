//! Request and response DTOs for import endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request body for POST /v1/imports.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct StartImportRequest {
    pub filename: String,
    pub content_type: String,
    pub file_size_bytes: u64,
}

/// Response body for POST /v1/imports (202 Accepted).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartImportResponse {
    pub import_id: Uuid,
    pub upload_url: String,
    pub status: String,
}

/// Request body for POST /v1/imports/:id/completion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct CompleteUploadRequest {
    pub checksum: String,
}

/// Response body for GET /v1/imports/:id and POST /v1/imports/:id/completion.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStatusResponse {
    pub id: Uuid,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
