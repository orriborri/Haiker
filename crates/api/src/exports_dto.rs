//! Request and response DTOs for export endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request body for POST /v1/activities/{activityId}/exports.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RequestExportRequest {
    pub route_version_id: Uuid,
    pub format: String,
}

/// Response body for POST /v1/activities/{activityId}/exports (202 Accepted).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestExportResponse {
    pub export_id: Uuid,
    pub status: String,
}

/// Response body for GET /v1/exports/{exportId}.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportStatusResponse {
    pub id: Uuid,
    pub status: String,
    pub format: String,
    pub route_version_id: Uuid,
    pub activity_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response body for GET /v1/exports/{exportId}/download.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDownloadUrlResponse {
    pub download_url: String,
    pub filename: String,
    pub expires_at: DateTime<Utc>,
    pub checksum: String,
    pub content_type: String,
}
