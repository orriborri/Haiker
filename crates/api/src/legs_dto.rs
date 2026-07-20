//! Request and response DTOs for leg endpoints.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Response body for a single leg summary in list responses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegSummaryResponse {
    pub id: Uuid,
    pub leg_number: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub date: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_summary: Option<LegSummaryStatsResponse>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Aggregated route statistics for a leg.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegSummaryStatsResponse {
    pub distance_meters: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_gain_meters: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_loss_meters: Option<f64>,
    pub point_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
}

/// Response body for a detailed leg view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegDetailResponse {
    pub id: Uuid,
    pub leg_number: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub date: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_track_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_summary: Option<LegSummaryStatsResponse>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response body for GET /v1/activities/{activityId}/legs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegListResponse {
    pub items: Vec<LegSummaryResponse>,
}

/// Request body for POST /v1/activities/{activityId}/legs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateLegRequest {
    /// Optional title for the leg.
    pub title: Option<String>,
    /// The date of this leg.
    pub date: NaiveDate,
}

/// Request body for PATCH /v1/activities/{activityId}/legs/{legId}.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UpdateLegRequest {
    /// New title for the leg. Set to null to clear.
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub title: Option<Option<String>>,
    /// New date for the leg.
    pub date: Option<NaiveDate>,
    /// New leg position (1-based) for reordering.
    pub leg_number: Option<u32>,
}

/// Aggregated statistics response for the activity detail.
#[allow(dead_code)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedStatsResponse {
    pub distance_meters: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_gain_meters: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_loss_meters: Option<f64>,
    pub point_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
}

/// Custom deserializer for doubly-optional fields.
/// - Absent field: `None` (do not update)
/// - `"title": null`: `Some(None)` (clear the title)
/// - `"title": "value"`: `Some(Some("value"))` (set the title)
fn deserialize_optional_field<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<Option<String>>::deserialize(deserializer)?;
    // If the field is present, opt is Some(inner) where inner is the JSON value.
    // serde gives us Some(None) for null, Some(Some(s)) for a string.
    Ok(Some(opt.unwrap_or(None)))
}
