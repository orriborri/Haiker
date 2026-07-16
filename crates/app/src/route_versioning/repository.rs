//! Route version repository trait.
//!
//! Defines the persistence interface for route version aggregates. Implementations
//! live in the platform/persistence layer.

use async_trait::async_trait;

use crate::activity_catalog::ActivityId;

use super::{RouteVersion, RouteVersionId, RouteVersioningError};

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
}
