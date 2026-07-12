//! Route Versioning bounded context.
//!
//! Owns immutable published routes, version numbers, corrected statistics,
//! and publication.

use thiserror::Error;

/// Errors that can occur in the route versioning context.
#[derive(Debug, Error)]
pub enum RouteVersioningError {
    /// The requested route version was not found.
    #[error("route version not found")]
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = RouteVersioningError::NotFound;
        assert_eq!(err.to_string(), "route version not found");
    }
}
