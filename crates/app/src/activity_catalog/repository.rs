//! Activity repository trait.
//!
//! Defines the persistence interface for activity aggregates. Implementations
//! live in the platform/persistence layer.

use async_trait::async_trait;

use crate::identity::UserId;

use super::{Activity, ActivityCatalogError, ActivityId};

/// A page of activities returned by cursor-based pagination.
#[derive(Debug, Clone)]
pub struct ActivityPage {
    /// The activities in this page.
    pub items: Vec<Activity>,
    /// Opaque cursor for fetching the next page. None if no more results.
    pub next_cursor: Option<String>,
    /// Whether there are more results beyond the current page.
    pub has_more: bool,
}

/// Repository trait for activity persistence.
///
/// Domain code programs against this trait; the actual persistence implementation
/// is provided by the infrastructure layer.
#[async_trait]
pub trait ActivityRepository: Send + Sync {
    /// List activities for a given owner with cursor-based pagination.
    ///
    /// Activities are ordered by started_at DESC, id DESC.
    /// Only active (non-deleted) activities are returned.
    async fn list_activities(
        &self,
        owner_id: UserId,
        cursor: Option<&str>,
        page_size: u32,
    ) -> Result<ActivityPage, ActivityCatalogError>;

    /// Find an activity by its ID.
    async fn find_by_id(&self, id: ActivityId) -> Result<Option<Activity>, ActivityCatalogError>;

    /// Save a new activity.
    async fn save(&self, activity: &Activity) -> Result<(), ActivityCatalogError>;

    /// Update an existing activity.
    async fn update(&self, activity: &Activity) -> Result<(), ActivityCatalogError>;

    /// Delete an activity by its ID (soft-delete by setting lifecycle_state).
    async fn delete(&self, id: ActivityId) -> Result<(), ActivityCatalogError>;
}
