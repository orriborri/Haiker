//! Route Editing bounded context.
//!
//! Owns drafts, operations, undo/redo, concurrency control, draft persistence,
//! and validation.

mod draft;
pub mod gateway;
mod operations;
pub mod repository;
pub mod validation;
mod value_objects;

pub use self::draft::{DraftState, OperationEntry, RouteDraft, RouteDraftId};
pub use self::gateway::{ActivityGateway, RouteVersionGateway};
pub use self::operations::RouteOperation;
pub use self::repository::RouteDraftRepository;
pub use self::validation::{validate_for_publication, PublicationValidationError};
pub use self::value_objects::{
    Coordinate, Elevation, OperationId, PointIndex, RoutePoint, SegmentIndex,
    MAX_REPLACEMENT_POINTS,
};

use thiserror::Error;

/// Errors that can occur in the route editing context.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteEditingError {
    /// The draft was not found.
    #[error("draft not found")]
    DraftNotFound,

    /// A concurrent modification conflict occurred.
    #[error("revision conflict: expected {expected}, got {actual}")]
    RevisionConflict {
        /// The expected revision number.
        expected: u64,
        /// The actual revision number.
        actual: u64,
    },

    /// The draft is not in an active state and cannot be edited.
    #[error("draft is not active")]
    DraftNotActive,

    /// An invalid operation was attempted.
    #[error("invalid operation: {message}")]
    InvalidOperation { message: String },

    /// An operation failed during application.
    #[error("operation failed: {message}")]
    OperationFailed { message: String },

    /// A segment has fewer than the required minimum points.
    #[error("insufficient points: minimum {minimum}, got {actual}")]
    InsufficientPoints { minimum: usize, actual: usize },

    /// An invalid segment index was referenced.
    #[error("invalid segment index: {index}, segment count: {count}")]
    InvalidSegmentIndex { index: usize, count: usize },

    /// An invalid point index was referenced.
    #[error("invalid point index: {index}, point count: {count}")]
    InvalidPointIndex { index: usize, count: usize },

    /// No operations available to undo.
    #[error("nothing to undo")]
    NothingToUndo,

    /// No operations available to redo.
    #[error("nothing to redo")]
    NothingToRedo,

    /// A coordinate value is out of valid range.
    #[error("invalid coordinate: {message}")]
    InvalidCoordinate { message: String },

    /// The referenced activity does not exist or is not accessible to the caller.
    #[error("activity not found")]
    ActivityNotFound,

    /// The activity exists but is in deleted lifecycle state.
    #[error("activity is deleted")]
    ActivityDeleted,

    /// The provided base route version ID does not exist or does not belong to the activity.
    #[error("invalid base route version")]
    InvalidBaseRouteVersion,

    /// The replacement exceeds the maximum allowed number of points.
    #[error("replacement too large: maximum {maximum}, actual {actual}")]
    ReplacementTooLarge { maximum: usize, actual: usize },

    /// The replacement endpoints do not match the geometry at the specified indices.
    #[error("endpoint continuity violation at {position}: expected ({expected}), got ({actual})")]
    EndpointContinuityViolation {
        position: String,
        expected: Coordinate,
        actual: Coordinate,
    },

    /// Publication validation failed with one or more geometry errors.
    #[error("publication validation failed")]
    PublicationValidationFailed {
        errors: Vec<validation::PublicationValidationError>,
    },
}

#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;
