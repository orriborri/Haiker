//! Activity Catalog bounded context.
//!
//! Owns activity identity, title, type, timestamps, current route version,
//! summary statistics, and lifecycle management.

use thiserror::Error;

/// Errors that can occur in the activity catalog context.
#[derive(Debug, Error)]
pub enum ActivityCatalogError {
    /// The requested activity was not found.
    #[error("activity not found")]
    ActivityNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = ActivityCatalogError::ActivityNotFound;
        assert_eq!(err.to_string(), "activity not found");
    }
}
