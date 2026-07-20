//! Request and response DTOs for route versioning endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Query parameters for GET /v1/activities/{activityId}/route-versions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RouteVersionListParams {
    /// Opaque cursor for the next page.
    pub cursor: Option<String>,
    /// Number of items per page (default 25, max 100).
    pub page_size: Option<u32>,
}

/// A single route version summary in the list response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteVersionSummaryResponse {
    pub id: Uuid,
    pub activity_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_version_id: Option<Uuid>,
    pub version_number: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_summary: Option<String>,
    pub corrected_statistics: serde_json::Value,
    pub calculation_version: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
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

/// Response body for GET /v1/activities/{activityId}/route-versions.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteVersionListResponse {
    pub items: Vec<RouteVersionSummaryResponse>,
    pub pagination: PaginationMeta,
}

/// Response body for GET /v1/route-versions/{routeVersionId}.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteVersionDetailResponse {
    pub id: Uuid,
    pub activity_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_version_id: Option<Uuid>,
    pub version_number: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_summary: Option<String>,
    pub corrected_statistics: serde_json::Value,
    pub calculation_version: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// GeoJSON FeatureCollection response for route version geometry.
///
/// Content-Type: application/geo+json
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteVersionGeometryResponse {
    /// Always "FeatureCollection".
    #[serde(rename = "type")]
    pub geojson_type: String,
    /// Bounding box as [west, south, east, north] (GeoJSON convention).
    pub bbox: [f64; 4],
    /// GeoJSON features (single LineString feature for the route).
    pub features: Vec<GeoJsonFeature>,
}

/// A single GeoJSON Feature representing the route version geometry.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoJsonFeature {
    /// Always "Feature".
    #[serde(rename = "type")]
    pub feature_type: String,
    /// The geometry of the feature.
    pub geometry: GeoJsonGeometry,
    /// Feature properties.
    pub properties: RouteVersionFeatureProperties,
}

/// GeoJSON geometry object (LineString for route versions).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoJsonGeometry {
    /// Always "LineString" for route geometry.
    #[serde(rename = "type")]
    pub geometry_type: String,
    /// Coordinates as [[longitude, latitude], ...].
    pub coordinates: Vec<[f64; 2]>,
}

/// Properties for the route version geometry feature.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteVersionFeatureProperties {
    /// Total number of points in the geometry.
    pub point_count: usize,
    /// Total distance in meters from corrected statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_meters: Option<f64>,
}

// --- Route Comparison DTOs ---

/// Query parameters for GET /v1/activities/{activityId}/route-comparison.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RouteComparisonParams {
    /// The route version ID to compare against the recorded route.
    pub route_version_id: Uuid,
}

/// Response body for GET /v1/activities/{activityId}/route-comparison.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteComparisonResponse {
    /// The recorded route section.
    pub recorded: RecordedRouteSection,
    /// The corrected route section.
    pub corrected: CorrectedRouteSection,
    /// Union bounding box encompassing both routes as [west, south, east, north].
    pub shared_bbox: [f64; 4],
}

/// The recorded route section of the comparison response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRouteSection {
    /// GeoJSON FeatureCollection for the recorded route (one Feature per segment).
    pub geometry: RecordedRouteGeometry,
    /// Statistics for the recorded route.
    pub statistics: RecordedRouteSectionStatistics,
}

/// GeoJSON FeatureCollection for the recorded route.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRouteGeometry {
    /// Always "FeatureCollection".
    #[serde(rename = "type")]
    pub geojson_type: String,
    /// Bounding box as [west, south, east, north].
    pub bbox: [f64; 4],
    /// GeoJSON features (one LineString per segment).
    pub features: Vec<RecordedRouteFeature>,
}

/// A single GeoJSON Feature for a recorded route segment.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRouteFeature {
    /// Always "Feature".
    #[serde(rename = "type")]
    pub feature_type: String,
    /// The geometry of the feature.
    pub geometry: GeoJsonGeometry,
    /// Feature properties.
    pub properties: RecordedRouteFeatureProperties,
}

/// Properties for a recorded route segment feature.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRouteFeatureProperties {
    /// Index of this segment (0-based).
    pub segment_index: usize,
    /// Number of points in this segment.
    pub point_count: usize,
}

/// Statistics for the recorded route.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRouteSectionStatistics {
    /// Total distance in meters.
    pub distance_meters: f64,
    /// Total elevation gain in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_gain_meters: Option<f64>,
    /// Total elevation loss in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_loss_meters: Option<f64>,
    /// Total number of points across all segments.
    pub point_count: u32,
    /// Number of segments.
    pub segment_count: u32,
}

/// The corrected route section of the comparison response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectedRouteSection {
    /// GeoJSON FeatureCollection for the corrected route (single LineString feature).
    pub geometry: CorrectedRouteGeometry,
    /// Statistics for the corrected route.
    pub statistics: CorrectedRouteSectionStatistics,
    /// Version number of the route version.
    pub version_number: i32,
    /// Optional edit summary from the route version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_summary: Option<String>,
}

/// GeoJSON FeatureCollection for the corrected route.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectedRouteGeometry {
    /// Always "FeatureCollection".
    #[serde(rename = "type")]
    pub geojson_type: String,
    /// Bounding box as [west, south, east, north].
    pub bbox: [f64; 4],
    /// GeoJSON features (single LineString feature).
    pub features: Vec<GeoJsonFeature>,
}

/// Statistics for the corrected route.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectedRouteSectionStatistics {
    /// Total distance in meters.
    pub distance_meters: f64,
    /// Number of coordinate points in the geometry.
    pub point_count: u32,
    /// Algorithm version used to compute these statistics.
    pub calculation_version: String,
}
