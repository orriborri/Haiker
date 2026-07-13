//! Recorded route repository trait.
//!
//! Defines the persistence interface for reading recorded route geometry.
//! Implementations live in the platform/persistence layer.

use async_trait::async_trait;
use uuid::Uuid;

use super::{BoundingBox, Coordinate, RecordedActivityError};

/// Full recorded route data including geometry, bounding box, and statistics.
#[derive(Debug, Clone)]
pub struct RecordedRouteData {
    /// The activity this route belongs to.
    pub activity_id: Uuid,
    /// Segments containing coordinate points (each segment has ordered points).
    pub segments: Vec<RouteSegment>,
    /// Bounding box enclosing all route points.
    pub bounding_box: BoundingBox,
    /// Route statistics.
    pub statistics: RouteStatistics,
}

/// A single segment of a recorded route with its coordinate points.
#[derive(Debug, Clone)]
pub struct RouteSegment {
    /// Ordered coordinates forming this segment.
    pub points: Vec<Coordinate>,
}

/// Statistics associated with a recorded route.
#[derive(Debug, Clone, Copy)]
pub struct RouteStatistics {
    /// Total distance in meters.
    pub distance_meters: f64,
    /// Total elevation gain in meters.
    pub elevation_gain_meters: Option<f64>,
    /// Total elevation loss in meters.
    pub elevation_loss_meters: Option<f64>,
    /// Total number of points across all segments.
    pub point_count: u32,
    /// Number of segments.
    pub segment_count: u32,
}

/// Preview of a recorded route (simplified, no full geometry).
#[derive(Debug, Clone)]
pub struct RecordedRoutePreview {
    /// The activity this route belongs to.
    pub activity_id: Uuid,
    /// Bounding box enclosing all route points.
    pub bounding_box: BoundingBox,
    /// Route statistics.
    pub statistics: RouteStatistics,
}

/// Repository trait for reading recorded route data.
///
/// Domain code programs against this trait; the actual persistence implementation
/// is provided by the infrastructure layer.
#[async_trait]
pub trait RecordedRouteRepository: Send + Sync {
    /// Get the full recorded route data for an activity.
    ///
    /// Returns None if no recorded route exists for the given activity.
    async fn get_recorded_route(
        &self,
        activity_id: Uuid,
    ) -> Result<Option<RecordedRouteData>, RecordedActivityError>;

    /// Get a preview of the recorded route (bounding box + statistics, no geometry).
    ///
    /// Returns None if no recorded route exists for the given activity.
    async fn get_recorded_route_preview(
        &self,
        activity_id: Uuid,
    ) -> Result<Option<RecordedRoutePreview>, RecordedActivityError>;
}
