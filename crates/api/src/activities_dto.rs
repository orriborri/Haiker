//! Request and response DTOs for activity endpoints.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Query parameters for GET /v1/activities.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ListActivitiesParams {
    /// Opaque cursor for the next page.
    pub cursor: Option<String>,
    /// Number of items per page (default 25, max 100).
    pub page_size: Option<u32>,
}

/// A single activity summary in the list response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySummaryResponse {
    pub id: Uuid,
    pub title: String,
    pub activity_type: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_summary: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrected_summary: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Pagination metadata in the response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginationMeta {
    /// Opaque cursor for the next page. Null if no more results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Whether there are more results beyond the current page.
    pub has_more: bool,
    /// The number of items returned in this page.
    pub page_size: u32,
}

/// Response body for GET /v1/activities.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityListResponse {
    pub items: Vec<ActivitySummaryResponse>,
    pub pagination: PaginationMeta,
}

/// Request body for PATCH /v1/activities/{activityId}/title.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RenameActivityRequest {
    /// The new title for the activity.
    pub title: String,
}

/// Response body for GET /v1/activities/{activityId}.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDetailResponse {
    pub id: Uuid,
    pub title: String,
    pub activity_type: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub lifecycle_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_summary: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrected_summary: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legs: Option<Vec<ActivityLegSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregated_stats: Option<ActivityAggregatedStats>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Summary of a leg within the activity detail response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLegSummary {
    pub id: Uuid,
    pub leg_number: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub date: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_summary: Option<serde_json::Value>,
}

/// Aggregated statistics across all legs in an activity.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityAggregatedStats {
    pub total_distance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_elevation_gain: Option<f64>,
    pub total_days: u32,
    pub total_legs: u32,
}
