//! Route version repository trait.
//!
//! Defines the persistence interface for route version aggregates. Implementations
//! live in the platform/persistence layer.

use async_trait::async_trait;

use crate::activity_catalog::ActivityId;

use super::{RouteVersion, RouteVersionId, RouteVersioningError};

/// A page of route versions returned by cursor-based pagination.
#[derive(Debug, Clone)]
pub struct RouteVersionPage {
    /// The route versions in this page.
    pub items: Vec<RouteVersion>,
    /// Opaque cursor for fetching the next page. None if no more results.
    pub next_cursor: Option<String>,
    /// Whether there are more results beyond the current page.
    pub has_more: bool,
}

/// Repository trait for route version persistence.
///
/// Domain code programs against this trait; the actual persistence implementation
/// is provided by the infrastructure layer.
#[async_trait]
pub trait RouteVersionRepository: Send + Sync {
    /// Save a new route version to the store.
    async fn save(&self, version: &RouteVersion) -> Result<(), RouteVersioningError>;

    /// Find a route version by its ID.
    async fn find_by_id(
        &self,
        id: RouteVersionId,
    ) -> Result<Option<RouteVersion>, RouteVersioningError>;

    /// Find the latest (highest version_number) route version for a given activity.
    async fn find_latest_by_activity(
        &self,
        activity_id: ActivityId,
    ) -> Result<Option<RouteVersion>, RouteVersioningError>;

    /// Find a route version by its idempotency key.
    async fn find_by_idempotency_key(
        &self,
        key: &str,
    ) -> Result<Option<RouteVersion>, RouteVersioningError>;

    /// List route versions for a given activity with cursor-based pagination.
    ///
    /// Results are ordered by version_number DESC (newest first).
    async fn list_by_activity(
        &self,
        activity_id: ActivityId,
        cursor: Option<&str>,
        page_size: u32,
    ) -> Result<RouteVersionPage, RouteVersioningError>;
}
