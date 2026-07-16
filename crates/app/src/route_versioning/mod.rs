//! Route Versioning bounded context.
//!
//! Owns immutable published routes, version numbers, corrected statistics,
//! and publication. Once a route version is created, it cannot be modified
//! or deleted - this invariant is enforced both in the domain model and at
//! the database level via immutability triggers.

pub mod commit;
pub mod corrected_statistics;
pub mod gateway;
pub mod publish_command;
pub mod repository;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;
use crate::recorded_activity::{BoundingBox, Coordinate};

pub use corrected_statistics::CorrectedStatistics;

/// A strongly-typed route version identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouteVersionId(pub Uuid);

impl RouteVersionId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for RouteVersionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An immutable published route version.
///
/// Route versions form a linked list via `parent_version_id`. The first version
/// for an activity has `version_number = 1` and `parent_version_id = None`.
/// Subsequent versions reference their predecessor.
///
/// # Invariants
///
/// - `version_number` must be >= 1.
/// - `geometry` must contain at least 2 coordinates.
/// - Once created, a route version is immutable (no updates or deletions).
/// - `parent_version_id` is `None` only for the initial version (version_number = 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteVersion {
    /// Unique identifier for this route version.
    pub id: RouteVersionId,
    /// The activity this route version belongs to.
    pub activity_id: ActivityId,
    /// The previous version this was derived from, or None for the initial version.
    pub parent_version_id: Option<RouteVersionId>,
    /// Sequential version number starting at 1.
    pub version_number: i32,
    /// The route geometry as an ordered list of coordinates.
    pub geometry: Vec<Coordinate>,
    /// The geographic bounding box enclosing the geometry.
    pub bounding_box: BoundingBox,
    /// Statistics computed from the corrected geometry (e.g., distance, elevation).
    pub corrected_statistics: CorrectedStatistics,
    /// Identifies the algorithm version used to compute corrected_statistics.
    pub calculation_version: String,
    /// Optional human-readable summary of edits made in this version.
    pub edit_summary: Option<String>,
    /// The user who created this version.
    pub created_by: UserId,
    /// When this version was created.
    pub created_at: DateTime<Utc>,
}

impl RouteVersion {
    /// Create the initial route version (version_number = 1) from import data.
    ///
    /// This is the factory method used during the commit-import flow to produce
    /// the first published route version for an activity.
    ///
    /// # Arguments
    ///
    /// * `activity_id` - The activity this route version belongs to.
    /// * `geometry` - The route geometry as an ordered list of coordinates (minimum 2).
    /// * `bounding_box` - The geographic bounding box enclosing the geometry.
    /// * `corrected_statistics` - Statistics computed from the geometry.
    /// * `calculation_version` - Identifies the algorithm version used.
    /// * `created_by` - The user creating this version.
    pub fn new_initial(
        activity_id: ActivityId,
        geometry: Vec<Coordinate>,
        bounding_box: BoundingBox,
        corrected_statistics: CorrectedStatistics,
        calculation_version: String,
        created_by: UserId,
    ) -> Result<Self, RouteVersioningError> {
        if geometry.len() < 2 {
            return Err(RouteVersioningError::InsufficientGeometry {
                minimum: 2,
                actual: geometry.len(),
            });
        }

        Ok(Self {
            id: RouteVersionId::generate(),
            activity_id,
            parent_version_id: None,
            version_number: 1,
            geometry,
            bounding_box,
            corrected_statistics,
            calculation_version,
            edit_summary: None,
            created_by,
            created_at: Utc::now(),
        })
    }

    /// Create a new route version from publication (version N+1).
    ///
    /// This is the factory method used when publishing a route draft. It creates
    /// a new version that references its parent version.
    ///
    /// # Arguments
    ///
    /// * `parent_version_id` - The previous version this was derived from.
    /// * `version_number` - The sequential version number (must be >= 2).
    /// * `activity_id` - The activity this route version belongs to.
    /// * `geometry` - The route geometry as an ordered list of coordinates (minimum 2).
    /// * `bounding_box` - The geographic bounding box enclosing the geometry.
    /// * `corrected_statistics` - Statistics computed from the geometry.
    /// * `calculation_version` - Identifies the algorithm version used.
    /// * `edit_summary` - Optional human-readable summary of edits.
    /// * `created_by` - The user creating this version.
    #[allow(clippy::too_many_arguments)]
    pub fn new_from_publication(
        parent_version_id: RouteVersionId,
        version_number: i32,
        activity_id: ActivityId,
        geometry: Vec<Coordinate>,
        bounding_box: BoundingBox,
        corrected_statistics: CorrectedStatistics,
        calculation_version: String,
        edit_summary: Option<String>,
        created_by: UserId,
    ) -> Result<Self, RouteVersioningError> {
        if version_number < 2 {
            return Err(RouteVersioningError::InvalidVersionNumber {
                minimum: 2,
                actual: version_number,
            });
        }

        if geometry.len() < 2 {
            return Err(RouteVersioningError::InsufficientGeometry {
                minimum: 2,
                actual: geometry.len(),
            });
        }

        Ok(Self {
            id: RouteVersionId::generate(),
            activity_id,
            parent_version_id: Some(parent_version_id),
            version_number,
            geometry,
            bounding_box,
            corrected_statistics,
            calculation_version,
            edit_summary,
            created_by,
            created_at: Utc::now(),
        })
    }
}

/// Errors that can occur in the route versioning context.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteVersioningError {
    /// The requested route version was not found.
    #[error("route version not found")]
    NotFound,

    /// Attempted to modify an immutable route version.
    #[error("route version is immutable and cannot be modified")]
    Immutable,

    /// The geometry has fewer than the required minimum coordinates.
    #[error("insufficient geometry points: minimum {minimum}, got {actual}")]
    InsufficientGeometry { minimum: usize, actual: usize },

    /// The version number is below the required minimum.
    #[error("invalid version number: minimum {minimum}, got {actual}")]
    InvalidVersionNumber { minimum: i32, actual: i32 },

    /// The draft was not found.
    #[error("draft not found")]
    DraftNotFound,

    /// The user is not authorized to perform this action.
    #[error("not authorized")]
    NotAuthorized,

    /// The expected revision does not match the actual revision of the draft.
    #[error("revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict { expected: u64, actual: u64 },

    /// The draft is not in an active state.
    #[error("draft is not active")]
    DraftNotActive,

    /// Validation of the draft geometry failed with one or more errors.
    #[error("validation failed: {errors:?}")]
    ValidationFailed { errors: Vec<String> },

    /// An idempotency conflict occurred (same key, different parameters).
    #[error("idempotency conflict")]
    IdempotencyConflict,

    /// The referenced activity was not found.
    #[error("activity not found")]
    ActivityNotFound,

    /// A persistence error occurred.
    #[error("persistence error: {message}")]
    PersistenceError { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_coordinate(lat: f64, lng: f64) -> Coordinate {
        Coordinate::new(lat, lng).unwrap()
    }

    fn sample_geometry() -> Vec<Coordinate> {
        vec![
            sample_coordinate(47.0, 11.0),
            sample_coordinate(47.1, 11.1),
            sample_coordinate(47.2, 11.2),
        ]
    }

    fn sample_bounding_box() -> BoundingBox {
        BoundingBox::new(sample_coordinate(47.0, 11.0), sample_coordinate(47.2, 11.2))
    }

    #[test]
    fn route_version_id_display() {
        let id = Uuid::new_v4();
        let version_id = RouteVersionId::new(id);
        assert_eq!(version_id.to_string(), id.to_string());
    }

    #[test]
    fn route_version_id_generate_is_unique() {
        let id1 = RouteVersionId::generate();
        let id2 = RouteVersionId::generate();
        assert_ne!(id1, id2);
    }

    #[test]
    fn new_initial_creates_version_one() {
        let activity_id = ActivityId::generate();
        let user_id = UserId::new(Uuid::new_v4());
        let geometry = sample_geometry();
        let bbox = sample_bounding_box();
        let stats = CorrectedStatistics::new(1500.0, 3, "v1.0".to_string());

        let version = RouteVersion::new_initial(
            activity_id,
            geometry.clone(),
            bbox,
            stats.clone(),
            "v1.0".to_string(),
            user_id,
        )
        .unwrap();

        assert_eq!(version.activity_id, activity_id);
        assert_eq!(version.version_number, 1);
        assert!(version.parent_version_id.is_none());
        assert_eq!(version.geometry.len(), 3);
        assert_eq!(version.bounding_box, bbox);
        assert_eq!(version.corrected_statistics, stats);
        assert_eq!(version.calculation_version, "v1.0");
        assert!(version.edit_summary.is_none());
        assert_eq!(version.created_by, user_id);
    }

    #[test]
    fn new_initial_rejects_empty_geometry() {
        let activity_id = ActivityId::generate();
        let user_id = UserId::new(Uuid::new_v4());
        let bbox = sample_bounding_box();

        let result = RouteVersion::new_initial(
            activity_id,
            vec![],
            bbox,
            CorrectedStatistics::new(0.0, 0, "v1.0".to_string()),
            "v1.0".to_string(),
            user_id,
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RouteVersioningError::InsufficientGeometry {
                minimum: 2,
                actual: 0
            }
        );
    }

    #[test]
    fn new_initial_rejects_single_point_geometry() {
        let activity_id = ActivityId::generate();
        let user_id = UserId::new(Uuid::new_v4());
        let bbox = sample_bounding_box();

        let result = RouteVersion::new_initial(
            activity_id,
            vec![sample_coordinate(47.0, 11.0)],
            bbox,
            CorrectedStatistics::new(0.0, 1, "v1.0".to_string()),
            "v1.0".to_string(),
            user_id,
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            RouteVersioningError::InsufficientGeometry {
                minimum: 2,
                actual: 1
            }
        );
    }

    #[test]
    fn new_initial_accepts_two_point_geometry() {
        let activity_id = ActivityId::generate();
        let user_id = UserId::new(Uuid::new_v4());
        let geometry = vec![sample_coordinate(47.0, 11.0), sample_coordinate(47.1, 11.1)];
        let bbox = sample_bounding_box();

        let version = RouteVersion::new_initial(
            activity_id,
            geometry,
            bbox,
            CorrectedStatistics::new(0.0, 2, "v1.0".to_string()),
            "v1.0".to_string(),
            user_id,
        );

        assert!(version.is_ok());
        assert_eq!(version.unwrap().geometry.len(), 2);
    }

    #[test]
    fn error_display_not_found() {
        let err = RouteVersioningError::NotFound;
        assert_eq!(err.to_string(), "route version not found");
    }

    #[test]
    fn error_display_immutable() {
        let err = RouteVersioningError::Immutable;
        assert_eq!(
            err.to_string(),
            "route version is immutable and cannot be modified"
        );
    }

    #[test]
    fn error_display_insufficient_geometry() {
        let err = RouteVersioningError::InsufficientGeometry {
            minimum: 2,
            actual: 0,
        };
        assert_eq!(
            err.to_string(),
            "insufficient geometry points: minimum 2, got 0"
        );
    }

    #[test]
    fn error_display_persistence() {
        let err = RouteVersioningError::PersistenceError {
            message: "connection lost".to_string(),
        };
        assert_eq!(err.to_string(), "persistence error: connection lost");
    }
}
