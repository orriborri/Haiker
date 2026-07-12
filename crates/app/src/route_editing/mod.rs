//! Route Editing bounded context.
//!
//! Owns drafts, operations, undo/redo, concurrency control, draft persistence,
//! and validation.

use thiserror::Error;

/// Errors that can occur in the route editing context.
#[derive(Debug, Error)]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = RouteEditingError::DraftNotFound;
        assert_eq!(err.to_string(), "draft not found");

        let err = RouteEditingError::RevisionConflict {
            expected: 1,
            actual: 2,
        };
        assert_eq!(err.to_string(), "revision conflict: expected 1, got 2");
    }
}
