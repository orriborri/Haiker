//! Recorded route query handlers.
//!
//! Contains query logic for reading recorded route geometry with ownership checks.

use uuid::Uuid;

use crate::activity_catalog::repository::ActivityRepository;
use crate::activity_catalog::{ActivityCatalogError, ActivityId, LifecycleState};
use crate::identity::UserId;

use super::repository::{RecordedRouteData, RecordedRoutePreview, RecordedRouteRepository};
use super::RecordedActivityError;

/// Get a recorded route for an activity, verifying ownership.
///
/// Returns `NotFound` if:
/// - The activity does not exist
/// - The activity is owned by a different user (non-disclosing)
/// - The activity is deleted
/// - No recorded route data exists
pub async fn get_recorded_route(
    activity_id: Uuid,
    owner_id: UserId,
    preview: bool,
    activity_repo: &dyn ActivityRepository,
    route_repo: &dyn RecordedRouteRepository,
) -> Result<RecordedRouteResult, RecordedActivityError> {
    // First verify ownership via the activity catalog
    let activity = activity_repo
        .find_by_id(ActivityId::new(activity_id))
        .await
        .map_err(|e| match e {
            ActivityCatalogError::PersistenceError { message } => {
                RecordedActivityError::Persistence { message }
            }
            _ => RecordedActivityError::NotFound,
        })?
        .filter(|a| a.owner_id == owner_id && a.lifecycle_state != LifecycleState::Deleted)
        .ok_or(RecordedActivityError::NotFound)?;

    if preview {
        let route_preview = route_repo
            .get_recorded_route_preview(activity.id.0)
            .await?
            .ok_or(RecordedActivityError::NotFound)?;
        Ok(RecordedRouteResult::Preview(route_preview))
    } else {
        let route_data = route_repo
            .get_recorded_route(activity.id.0)
            .await?
            .ok_or(RecordedActivityError::NotFound)?;
        Ok(RecordedRouteResult::Full(route_data))
    }
}

/// Result of a recorded route query, either full data or preview.
#[derive(Debug, Clone)]
pub enum RecordedRouteResult {
    /// Full route data with geometry.
    Full(RecordedRouteData),
    /// Preview with only bounding box and statistics.
    Preview(RecordedRoutePreview),
}
