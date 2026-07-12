//! Recorded Activity bounded context.
//!
//! Owns source artifacts, source revisions, recorded tracks, segments,
//! point streams, and sensor samples.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::identity::UserId;

/// A strongly-typed source artifact identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceArtifactId(pub Uuid);

impl SourceArtifactId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for SourceArtifactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A strongly-typed source revision identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceRevisionId(pub Uuid);

impl SourceRevisionId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for SourceRevisionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A strongly-typed recorded track identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordedTrackId(pub Uuid);

impl RecordedTrackId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for RecordedTrackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A geographic coordinate with latitude and longitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    pub latitude: f64,
    pub longitude: f64,
}

impl Coordinate {
    /// Create a new Coordinate, validating that latitude is in [-90, 90]
    /// and longitude is in [-180, 180].
    pub fn new(latitude: f64, longitude: f64) -> Result<Self, RecordedActivityError> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(RecordedActivityError::InvalidCoordinate {
                message: format!("latitude must be between -90 and 90, got {latitude}"),
            });
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(RecordedActivityError::InvalidCoordinate {
                message: format!("longitude must be between -180 and 180, got {longitude}"),
            });
        }
        Ok(Self {
            latitude,
            longitude,
        })
    }
}

/// An elevation measurement in meters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Elevation(pub f64);

impl Elevation {
    pub fn new(meters: f64) -> Self {
        Self(meters)
    }

    pub fn meters(self) -> f64 {
        self.0
    }
}

/// A single point in a recorded GPS track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackPoint {
    pub coordinate: Coordinate,
    pub elevation: Option<Elevation>,
    pub timestamp: Option<DateTime<Utc>>,
}

impl TrackPoint {
    pub fn new(
        coordinate: Coordinate,
        elevation: Option<Elevation>,
        timestamp: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            coordinate,
            elevation,
            timestamp,
        }
    }
}

/// A contiguous segment of track points (minimum 2 points).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackSegment {
    points: Vec<TrackPoint>,
}

impl TrackSegment {
    /// Create a new TrackSegment from a vector of points.
    /// Requires at least 2 points.
    pub fn new(points: Vec<TrackPoint>) -> Result<Self, RecordedActivityError> {
        if points.len() < 2 {
            return Err(RecordedActivityError::InsufficientPoints {
                minimum: 2,
                actual: points.len(),
            });
        }
        Ok(Self { points })
    }

    pub fn points(&self) -> &[TrackPoint] {
        &self.points
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }
}

/// A bounding box defined by southwest and northeast corners.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub south_west: Coordinate,
    pub north_east: Coordinate,
}

impl BoundingBox {
    pub fn new(south_west: Coordinate, north_east: Coordinate) -> Self {
        Self {
            south_west,
            north_east,
        }
    }

    /// Compute a bounding box from a slice of coordinates.
    pub fn from_coordinates(coords: &[Coordinate]) -> Option<Self> {
        if coords.is_empty() {
            return None;
        }

        let mut min_lat = f64::MAX;
        let mut max_lat = f64::MIN;
        let mut min_lng = f64::MAX;
        let mut max_lng = f64::MIN;

        for coord in coords {
            min_lat = min_lat.min(coord.latitude);
            max_lat = max_lat.max(coord.latitude);
            min_lng = min_lng.min(coord.longitude);
            max_lng = max_lng.max(coord.longitude);
        }

        Some(Self {
            south_west: Coordinate {
                latitude: min_lat,
                longitude: min_lng,
            },
            north_east: Coordinate {
                latitude: max_lat,
                longitude: max_lng,
            },
        })
    }
}

/// Statistics computed from a recorded track.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RecordedStatistics {
    pub distance_meters: f64,
    pub duration_seconds: Option<f64>,
    pub elevation_gain_meters: Option<f64>,
    pub elevation_loss_meters: Option<f64>,
    pub point_count: u32,
    pub segment_count: u32,
}

/// A recorded track comprising one or more segments with computed statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedTrack {
    pub id: RecordedTrackId,
    pub segments: Vec<TrackSegment>,
    pub bounding_box: BoundingBox,
    pub statistics: RecordedStatistics,
}

impl RecordedTrack {
    /// Create a new RecordedTrack from segments with pre-computed statistics.
    pub fn new(
        segments: Vec<TrackSegment>,
        bounding_box: BoundingBox,
        statistics: RecordedStatistics,
    ) -> Result<Self, RecordedActivityError> {
        if segments.is_empty() {
            return Err(RecordedActivityError::NoSegments);
        }
        Ok(Self {
            id: RecordedTrackId::generate(),
            segments,
            bounding_box,
            statistics,
        })
    }
}

/// A source artifact representing the raw uploaded file.
#[derive(Debug, Clone)]
pub struct SourceArtifact {
    pub id: SourceArtifactId,
    pub import_id: Uuid,
    pub owner_id: UserId,
    pub object_storage_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub checksum: String,
    pub created_at: DateTime<Utc>,
}

/// A source revision linking an artifact to an activity.
#[derive(Debug, Clone)]
pub struct SourceRevision {
    pub id: SourceRevisionId,
    pub activity_id: Uuid,
    pub source_artifact_id: SourceArtifactId,
    pub revision_number: i32,
    pub provider: String,
    pub parser_version: String,
    pub imported_at: DateTime<Utc>,
}

/// Errors that can occur in the recorded activity context.
#[derive(Debug, Error, PartialEq)]
pub enum RecordedActivityError {
    /// The requested recorded activity was not found.
    #[error("recorded activity not found")]
    NotFound,

    /// A coordinate value is out of valid range.
    #[error("invalid coordinate: {message}")]
    InvalidCoordinate { message: String },

    /// A track segment has fewer than the required minimum points.
    #[error("insufficient points: minimum {minimum}, got {actual}")]
    InsufficientPoints { minimum: usize, actual: usize },

    /// A recorded track must have at least one segment.
    #[error("recorded track must have at least one segment")]
    NoSegments,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_valid_center() {
        let coord = Coordinate::new(0.0, 0.0).unwrap();
        assert_eq!(coord.latitude, 0.0);
        assert_eq!(coord.longitude, 0.0);
    }

    #[test]
    fn coordinate_valid_extremes() {
        let coord = Coordinate::new(90.0, 180.0).unwrap();
        assert_eq!(coord.latitude, 90.0);
        assert_eq!(coord.longitude, 180.0);

        let coord = Coordinate::new(-90.0, -180.0).unwrap();
        assert_eq!(coord.latitude, -90.0);
        assert_eq!(coord.longitude, -180.0);
    }

    #[test]
    fn coordinate_rejects_latitude_too_high() {
        let result = Coordinate::new(90.1, 0.0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidCoordinate { .. }
        ));
    }

    #[test]
    fn coordinate_rejects_latitude_too_low() {
        let result = Coordinate::new(-90.1, 0.0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidCoordinate { .. }
        ));
    }

    #[test]
    fn coordinate_rejects_longitude_too_high() {
        let result = Coordinate::new(0.0, 180.1);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidCoordinate { .. }
        ));
    }

    #[test]
    fn coordinate_rejects_longitude_too_low() {
        let result = Coordinate::new(0.0, -180.1);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidCoordinate { .. }
        ));
    }

    #[test]
    fn elevation_construction() {
        let elev = Elevation::new(1234.5);
        assert_eq!(elev.meters(), 1234.5);
    }

    #[test]
    fn track_point_creation() {
        let coord = Coordinate::new(45.0, 10.0).unwrap();
        let elev = Elevation::new(500.0);
        let now = Utc::now();
        let point = TrackPoint::new(coord, Some(elev), Some(now));

        assert_eq!(point.coordinate, coord);
        assert_eq!(point.elevation, Some(elev));
        assert_eq!(point.timestamp, Some(now));
    }

    #[test]
    fn track_point_optional_fields() {
        let coord = Coordinate::new(45.0, 10.0).unwrap();
        let point = TrackPoint::new(coord, None, None);

        assert!(point.elevation.is_none());
        assert!(point.timestamp.is_none());
    }

    #[test]
    fn track_segment_valid_with_two_points() {
        let p1 = TrackPoint::new(Coordinate::new(45.0, 10.0).unwrap(), None, None);
        let p2 = TrackPoint::new(Coordinate::new(45.1, 10.1).unwrap(), None, None);

        let segment = TrackSegment::new(vec![p1, p2]).unwrap();
        assert_eq!(segment.point_count(), 2);
    }

    #[test]
    fn track_segment_rejects_single_point() {
        let p1 = TrackPoint::new(Coordinate::new(45.0, 10.0).unwrap(), None, None);

        let result = TrackSegment::new(vec![p1]);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InsufficientPoints {
                minimum: 2,
                actual: 1
            }
        ));
    }

    #[test]
    fn track_segment_rejects_empty() {
        let result = TrackSegment::new(vec![]);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InsufficientPoints {
                minimum: 2,
                actual: 0
            }
        ));
    }

    #[test]
    fn bounding_box_from_coordinates() {
        let coords = vec![
            Coordinate::new(10.0, 20.0).unwrap(),
            Coordinate::new(30.0, 40.0).unwrap(),
            Coordinate::new(20.0, 30.0).unwrap(),
        ];

        let bbox = BoundingBox::from_coordinates(&coords).unwrap();
        assert_eq!(bbox.south_west.latitude, 10.0);
        assert_eq!(bbox.south_west.longitude, 20.0);
        assert_eq!(bbox.north_east.latitude, 30.0);
        assert_eq!(bbox.north_east.longitude, 40.0);
    }

    #[test]
    fn bounding_box_from_empty_returns_none() {
        let result = BoundingBox::from_coordinates(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn recorded_track_valid() {
        let p1 = TrackPoint::new(Coordinate::new(45.0, 10.0).unwrap(), None, None);
        let p2 = TrackPoint::new(Coordinate::new(45.1, 10.1).unwrap(), None, None);
        let segment = TrackSegment::new(vec![p1, p2]).unwrap();

        let bbox = BoundingBox::new(
            Coordinate::new(45.0, 10.0).unwrap(),
            Coordinate::new(45.1, 10.1).unwrap(),
        );

        let stats = RecordedStatistics {
            distance_meters: 1000.0,
            duration_seconds: Some(600.0),
            elevation_gain_meters: Some(50.0),
            elevation_loss_meters: Some(30.0),
            point_count: 2,
            segment_count: 1,
        };

        let track = RecordedTrack::new(vec![segment], bbox, stats).unwrap();
        assert_eq!(track.segments.len(), 1);
        assert_eq!(track.statistics.distance_meters, 1000.0);
    }

    #[test]
    fn recorded_track_rejects_no_segments() {
        let bbox = BoundingBox::new(
            Coordinate::new(0.0, 0.0).unwrap(),
            Coordinate::new(1.0, 1.0).unwrap(),
        );

        let stats = RecordedStatistics {
            distance_meters: 0.0,
            duration_seconds: None,
            elevation_gain_meters: None,
            elevation_loss_meters: None,
            point_count: 0,
            segment_count: 0,
        };

        let result = RecordedTrack::new(vec![], bbox, stats);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::NoSegments
        ));
    }

    #[test]
    fn source_artifact_id_display() {
        let id = Uuid::new_v4();
        let artifact_id = SourceArtifactId::new(id);
        assert_eq!(artifact_id.to_string(), id.to_string());
    }

    #[test]
    fn source_revision_id_display() {
        let id = Uuid::new_v4();
        let revision_id = SourceRevisionId::new(id);
        assert_eq!(revision_id.to_string(), id.to_string());
    }

    #[test]
    fn recorded_track_id_display() {
        let id = Uuid::new_v4();
        let track_id = RecordedTrackId::new(id);
        assert_eq!(track_id.to_string(), id.to_string());
    }

    #[test]
    fn error_display() {
        let err = RecordedActivityError::NotFound;
        assert_eq!(err.to_string(), "recorded activity not found");

        let err = RecordedActivityError::InvalidCoordinate {
            message: "latitude out of range".to_string(),
        };
        assert_eq!(err.to_string(), "invalid coordinate: latitude out of range");

        let err = RecordedActivityError::InsufficientPoints {
            minimum: 2,
            actual: 1,
        };
        assert_eq!(err.to_string(), "insufficient points: minimum 2, got 1");

        let err = RecordedActivityError::NoSegments;
        assert_eq!(
            err.to_string(),
            "recorded track must have at least one segment"
        );
    }
}
