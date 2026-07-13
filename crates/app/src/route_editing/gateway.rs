//! Cross-context gateway traits for the route editing bounded context.
//!
//! These traits define what the route editing context needs from other contexts
//! (activity catalog, route versioning) without coupling to their implementations.

use async_trait::async_trait;
use uuid::Uuid;

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;

use super::RouteEditingError;

/// Gateway for validating activity existence, ownership, and lifecycle state.
///
/// The route editing context uses this to ensure that a draft can only be created
/// for an activity that exists, is owned by the requesting user, and is not deleted.
#[async_trait]
pub trait ActivityGateway: Send + Sync {
    /// Validate that the activity exists, is owned by the given user, and is active.
    ///
    /// Returns:
    /// - `Ok(())` if the activity is valid for draft creation
    /// - `Err(RouteEditingError::ActivityNotFound)` if the activity does not exist or is not owned by the user
    /// - `Err(RouteEditingError::ActivityDeleted)` if the activity exists and is owned but is deleted
    async fn validate_activity_for_draft(
        &self,
        activity_id: ActivityId,
        owner_id: UserId,
    ) -> Result<(), RouteEditingError>;
}

/// Gateway for validating base route version existence.
///
/// The route editing context uses this to ensure that a base route version ID
/// provided during draft creation actually refers to a valid route version
/// belonging to the specified activity.
#[async_trait]
pub trait RouteVersionGateway: Send + Sync {
    /// Validate that the given route version exists and belongs to the specified activity.
    ///
    /// Returns:
    /// - `Ok(())` if the route version is valid
    /// - `Err(RouteEditingError::InvalidBaseRouteVersion)` if the version does not exist or does not belong to the activity
    async fn validate_route_version_exists(
        &self,
        route_version_id: Uuid,
        activity_id: ActivityId,
    ) -> Result<(), RouteEditingError>;
}
