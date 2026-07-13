//! Route Editing bounded context.
//!
//! Owns drafts, operations, undo/redo, concurrency control, draft persistence,
//! and validation.

mod draft;
mod operations;
mod value_objects;

pub use self::draft::{DraftState, OperationEntry, RouteDraft, RouteDraftId};
pub use self::operations::RouteOperation;
pub use self::value_objects::{
    Coordinate, Elevation, OperationId, PointIndex, RoutePoint, SegmentIndex,
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
}

#[cfg(test)]
mod tests;
