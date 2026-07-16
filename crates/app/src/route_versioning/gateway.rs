//! Cross-context gateway traits for the route versioning bounded context.
//!
//! These traits define what the route versioning context needs from other contexts
//! (activity catalog) without coupling to their implementations.

use async_trait::async_trait;

use crate::activity_catalog::ActivityId;

use super::{RouteVersionId, RouteVersioningError};

/// Gateway for updating the activity's current route version pointer.
///
/// The route versioning context uses this to notify the activity catalog that
/// a new route version has been published and should become the current version.
#[async_trait]
pub trait PublicationGateway: Send + Sync {
    /// Update the activity's current route version to the newly published version.
    ///
    /// Also sets the corrected_summary on the activity for the catalog read model.
    ///
    /// Returns:
    /// - `Ok(())` if the activity pointer was updated successfully
    /// - `Err(RouteVersioningError::ActivityNotFound)` if the activity does not exist
    async fn update_activity_current_version(
        &self,
        activity_id: ActivityId,
        route_version_id: RouteVersionId,
        corrected_summary: serde_json::Value,
    ) -> Result<(), RouteVersioningError>;
}
