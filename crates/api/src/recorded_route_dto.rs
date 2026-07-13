//! Request and response DTOs for the recorded route endpoint.

use serde::{Deserialize, Serialize};

/// Query parameters for GET /v1/activities/{activityId}/recorded-route.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRouteParams {
    /// Level of detail: "preview" or "full" (default: "full").
    #[serde(default = "default_detail")]
    pub detail: RouteDetail,
}

/// Allowed values for the `detail` query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteDetail {
    Full,
    Preview,
}

fn default_detail() -> RouteDetail {
    RouteDetail::Full
}

/// GeoJSON FeatureCollection response for full recorded route geometry.
///
/// Content-Type: application/geo+json
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRouteResponse {
    /// Always "FeatureCollection".
    #[serde(rename = "type")]
    pub geojson_type: String,
    /// Bounding box as [west, south, east, north] (GeoJSON convention).
    pub bbox: [f64; 4],
    /// GeoJSON features (one feature per segment).
    pub features: Vec<GeoJsonFeature>,
    /// Additional properties (statistics).
    pub properties: RouteProperties,
}

/// A single GeoJSON Feature representing a route segment.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoJsonFeature {
    /// Always "Feature".
    #[serde(rename = "type")]
    pub feature_type: String,
    /// The geometry of the feature.
    pub geometry: GeoJsonGeometry,
    /// Feature properties.
    pub properties: SegmentProperties,
}

/// GeoJSON geometry object (LineString for route segments).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoJsonGeometry {
    /// Always "LineString" for route segments.
    #[serde(rename = "type")]
    pub geometry_type: String,
    /// Coordinates as [[longitude, latitude], ...].
    pub coordinates: Vec<[f64; 2]>,
}

/// Properties for a route segment feature.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentProperties {
    /// Zero-based index of this segment.
    pub segment_index: usize,
    /// Number of points in this segment.
    pub point_count: usize,
}

/// Statistics and metadata for the route.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteProperties {
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

/// Preview response for the recorded route (no full geometry).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRoutePreviewResponse {
    /// Bounding box as [west, south, east, north] (GeoJSON bbox convention).
    pub bbox: [f64; 4],
    /// Total number of points.
    pub point_count: u32,
    /// Number of segments.
    pub segment_count: u32,
    /// Total distance in meters.
    pub distance_meters: f64,
    /// Total elevation gain in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_gain_meters: Option<f64>,
    /// Total elevation loss in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_loss_meters: Option<f64>,
}
