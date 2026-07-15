//! Value objects for the route editing bounded context.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::RouteEditingError;

/// Maximum number of points allowed in a single ReplaceSection replacement.
pub const MAX_REPLACEMENT_POINTS: usize = 500;

/// A geographic coordinate with validated latitude and longitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    pub latitude: f64,
    pub longitude: f64,
}

impl Coordinate {
    /// Create a new Coordinate, validating that latitude is in [-90, 90]
    /// and longitude is in [-180, 180].
    pub fn new(latitude: f64, longitude: f64) -> Result<Self, RouteEditingError> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(RouteEditingError::InvalidCoordinate {
                message: format!("latitude must be between -90 and 90, got {latitude}"),
            });
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(RouteEditingError::InvalidCoordinate {
                message: format!("longitude must be between -180 and 180, got {longitude}"),
            });
        }
        Ok(Self {
            latitude,
            longitude,
        })
    }
}

impl Eq for Coordinate {}

impl std::fmt::Display for Coordinate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, {}", self.latitude, self.longitude)
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

impl Eq for Elevation {}

/// A point on a route, containing a coordinate and optional elevation.
/// Does NOT contain fabricated timestamp, heart rate, speed, temperature, or cadence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePoint {
    pub coordinate: Coordinate,
    pub elevation: Option<Elevation>,
}

impl RoutePoint {
    pub fn new(coordinate: Coordinate, elevation: Option<Elevation>) -> Self {
        Self {
            coordinate,
            elevation,
        }
    }
}

/// An index into a segment's point vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PointIndex(pub usize);

impl PointIndex {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn value(self) -> usize {
        self.0
    }
}

/// An index into the geometry's segment vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentIndex(pub usize);

impl SegmentIndex {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn value(self) -> usize {
        self.0
    }
}

/// A strongly-typed operation identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub Uuid);

impl OperationId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
