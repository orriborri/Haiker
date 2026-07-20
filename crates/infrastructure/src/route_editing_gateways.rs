//! Production gateway implementations for the route editing context.
//!
//! These implementations provide cross-context validation by querying
//! the activity catalog and route versioning data stores.
//!
//! Note: These are placeholder implementations that always succeed.
//! They will be replaced with actual database-backed implementations
//! when the activity catalog and route versioning persistence layers
//! are integrated with the route editing context.

use async_trait::async_trait;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::identity::UserId;
use haiker_app::route_editing::{
    ActivityGateway, RouteEditingError, RoutePoint, RouteVersionGateway,
};

/// Placeholder activity gateway that delegates to the activity repository.
///
/// TODO: Replace with actual PgActivityRepository-backed implementation
/// that queries the activities table for existence, ownership, and lifecycle.
#[derive(Default)]
pub struct PgActivityGateway;

impl PgActivityGateway {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ActivityGateway for PgActivityGateway {
    async fn validate_activity_for_draft(
        &self,
        _activity_id: ActivityId,
        _owner_id: UserId,
    ) -> Result<(), RouteEditingError> {
        // Placeholder: always succeeds until integrated with actual persistence
        Ok(())
    }
}

/// Placeholder route version gateway.
///
/// TODO: Replace with actual PgRouteVersionRepository-backed implementation
/// that queries the route_versions table for existence and activity membership.
#[derive(Default)]
pub struct PgRouteVersionGateway;

impl PgRouteVersionGateway {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RouteVersionGateway for PgRouteVersionGateway {
    async fn validate_route_version_exists(
        &self,
        _route_version_id: Uuid,
        _activity_id: ActivityId,
    ) -> Result<(), RouteEditingError> {
        // Placeholder: always succeeds until integrated with actual persistence
        Ok(())
    }

    async fn get_route_version_geometry(
        &self,
        _route_version_id: Uuid,
        _activity_id: ActivityId,
    ) -> Result<Vec<Vec<RoutePoint>>, RouteEditingError> {
        // Placeholder: always succeeds until integrated with actual persistence.
        // Returns an empty geometry (single empty segment) as a stand-in.
        // This keeps the placeholder consistent with validate_route_version_exists
        // which also returns Ok(()) unconditionally.
        Ok(vec![vec![]])
    }
}
