//! Route draft repository trait.
//!
//! Defines the persistence interface for route draft aggregates. Implementations
//! live in the platform/persistence layer.

use async_trait::async_trait;

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;

use super::value_objects::OperationId;
use super::{RouteDraft, RouteDraftId, RouteEditingError};

/// Repository trait for route draft persistence.
///
/// Domain code programs against this trait; the actual persistence implementation
/// is provided by the infrastructure layer.
#[async_trait]
pub trait RouteDraftRepository: Send + Sync {
    /// Save a new route draft to the store.
    async fn save(&self, draft: &RouteDraft) -> Result<(), RouteEditingError>;

    /// Find a route draft by its ID.
    async fn find_by_id(&self, id: RouteDraftId) -> Result<Option<RouteDraft>, RouteEditingError>;

    /// Find the active draft for a given activity and owner.
    async fn find_active_by_activity(
        &self,
        activity_id: ActivityId,
        owner_id: UserId,
    ) -> Result<Option<RouteDraft>, RouteEditingError>;

    /// Update an existing route draft (persists all fields).
    async fn update(&self, draft: &RouteDraft) -> Result<(), RouteEditingError>;

    /// Find the draft ID that contains a specific operation (for idempotency lookups).
    async fn find_by_operation_id(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<RouteDraftId>, RouteEditingError>;
}
