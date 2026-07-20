//! Route versioning query handlers.
//!
//! Contains query logic for listing and fetching route versions with ownership checks.

use crate::activity_catalog::repository::ActivityRepository;
use crate::activity_catalog::{ActivityCatalogError, ActivityId, LifecycleState};
use crate::identity::UserId;
use crate::recorded_activity::{BoundingBox, Coordinate};

use super::repository::{RouteVersionPage, RouteVersionRepository};
use super::{CorrectedStatistics, RouteVersion, RouteVersionId, RouteVersioningError};

/// Default page size for route version listings.
pub const DEFAULT_PAGE_SIZE: u32 = 25;

/// Maximum allowed page size.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Result of a route version geometry query.
#[derive(Debug, Clone)]
pub struct RouteVersionGeometry {
    /// The route geometry as coordinates.
    pub geometry: Vec<Coordinate>,
    /// The bounding box enclosing the geometry.
    pub bounding_box: BoundingBox,
    /// The corrected statistics.
    pub corrected_statistics: CorrectedStatistics,
}

/// List route versions for an activity, verifying ownership.
///
/// Returns `NotFound` if:
/// - The activity does not exist
/// - The activity is owned by a different user (non-disclosing)
/// - The activity is deleted
pub async fn list_route_versions(
    activity_id: ActivityId,
    owner_id: UserId,
    cursor: Option<&str>,
    page_size: Option<u32>,
    activity_repo: &dyn ActivityRepository,
    version_repo: &dyn RouteVersionRepository,
) -> Result<RouteVersionPage, RouteVersioningError> {
    // Verify ownership via the activity catalog
    verify_activity_ownership(activity_id, owner_id, activity_repo).await?;

    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    version_repo
        .list_by_activity(activity_id, cursor, page_size)
        .await
}

/// Get a single route version by ID, verifying ownership.
///
/// Returns `NotFound` if:
/// - The route version does not exist
/// - The associated activity is owned by a different user (non-disclosing)
/// - The associated activity is deleted
pub async fn get_route_version(
    route_version_id: RouteVersionId,
    owner_id: UserId,
    activity_repo: &dyn ActivityRepository,
    version_repo: &dyn RouteVersionRepository,
) -> Result<RouteVersion, RouteVersioningError> {
    let version = version_repo
        .find_by_id(route_version_id)
        .await?
        .ok_or(RouteVersioningError::NotFound)?;

    // Verify ownership via the activity catalog
    verify_activity_ownership(version.activity_id, owner_id, activity_repo).await?;

    Ok(version)
}

/// Get route version geometry by ID, verifying ownership.
///
/// Returns `NotFound` if:
/// - The route version does not exist
/// - The associated activity is owned by a different user (non-disclosing)
/// - The associated activity is deleted
pub async fn get_route_version_geometry(
    route_version_id: RouteVersionId,
    owner_id: UserId,
    activity_repo: &dyn ActivityRepository,
    version_repo: &dyn RouteVersionRepository,
) -> Result<RouteVersionGeometry, RouteVersioningError> {
    let version = version_repo
        .find_by_id(route_version_id)
        .await?
        .ok_or(RouteVersioningError::NotFound)?;

    // Verify ownership via the activity catalog
    verify_activity_ownership(version.activity_id, owner_id, activity_repo).await?;

    Ok(RouteVersionGeometry {
        geometry: version.geometry,
        bounding_box: version.bounding_box,
        corrected_statistics: version.corrected_statistics,
    })
}

/// Verify that the given activity exists, is not deleted, and is owned by the specified user.
///
/// Returns `RouteVersioningError::NotFound` for all failure cases (non-disclosing).
async fn verify_activity_ownership(
    activity_id: ActivityId,
    owner_id: UserId,
    activity_repo: &dyn ActivityRepository,
) -> Result<(), RouteVersioningError> {
    let activity = activity_repo
        .find_by_id(activity_id)
        .await
        .map_err(|e| match e {
            ActivityCatalogError::PersistenceError { message } => {
                RouteVersioningError::PersistenceError { message }
            }
            _ => RouteVersioningError::NotFound,
        })?
        .filter(|a| a.owner_id == owner_id && a.lifecycle_state != LifecycleState::Deleted)
        .ok_or(RouteVersioningError::NotFound)?;

    // Activity found and ownership verified
    let _ = activity;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    use crate::activity_catalog::repository::ActivityPage;
    use crate::activity_catalog::{Activity, ActivityTitle, ActivityType};
    use crate::recorded_activity::Coordinate;
    use crate::route_versioning::CorrectedStatistics;

    // --- In-memory ActivityRepository ---

    struct FakeActivityRepository {
        activities: Mutex<HashMap<ActivityId, Activity>>,
    }

    impl FakeActivityRepository {
        fn with_activities(activities: Vec<Activity>) -> Self {
            let map: HashMap<ActivityId, Activity> =
                activities.into_iter().map(|a| (a.id, a.clone())).collect();
            Self {
                activities: Mutex::new(map),
            }
        }
    }

    #[async_trait]
    impl ActivityRepository for FakeActivityRepository {
        async fn list_activities(
            &self,
            _owner_id: UserId,
            _cursor: Option<&str>,
            _page_size: u32,
        ) -> Result<ActivityPage, ActivityCatalogError> {
            Ok(ActivityPage {
                items: vec![],
                next_cursor: None,
                has_more: false,
            })
        }

        async fn find_by_id(
            &self,
            id: ActivityId,
        ) -> Result<Option<Activity>, ActivityCatalogError> {
            Ok(self.activities.lock().unwrap().get(&id).cloned())
        }

        async fn save(&self, activity: &Activity) -> Result<(), ActivityCatalogError> {
            self.activities
                .lock()
                .unwrap()
                .insert(activity.id, activity.clone());
            Ok(())
        }

        async fn update(&self, activity: &Activity) -> Result<(), ActivityCatalogError> {
            self.activities
                .lock()
                .unwrap()
                .insert(activity.id, activity.clone());
            Ok(())
        }

        async fn delete(&self, id: ActivityId) -> Result<(), ActivityCatalogError> {
            self.activities.lock().unwrap().remove(&id);
            Ok(())
        }
    }

    // --- In-memory RouteVersionRepository ---

    struct FakeRouteVersionRepository {
        versions: Mutex<Vec<RouteVersion>>,
    }

    impl FakeRouteVersionRepository {
        fn new() -> Self {
            Self {
                versions: Mutex::new(Vec::new()),
            }
        }

        fn with_versions(versions: Vec<RouteVersion>) -> Self {
            Self {
                versions: Mutex::new(versions),
            }
        }
    }

    #[async_trait]
    impl RouteVersionRepository for FakeRouteVersionRepository {
        async fn save(&self, version: &RouteVersion) -> Result<(), RouteVersioningError> {
            self.versions.lock().unwrap().push(version.clone());
            Ok(())
        }

        async fn find_by_id(
            &self,
            id: RouteVersionId,
        ) -> Result<Option<RouteVersion>, RouteVersioningError> {
            Ok(self
                .versions
                .lock()
                .unwrap()
                .iter()
                .find(|v| v.id == id)
                .cloned())
        }

        async fn find_latest_by_activity(
            &self,
            activity_id: ActivityId,
        ) -> Result<Option<RouteVersion>, RouteVersioningError> {
            Ok(self
                .versions
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.activity_id == activity_id)
                .max_by_key(|v| v.version_number)
                .cloned())
        }

        async fn find_by_idempotency_key(
            &self,
            _key: &str,
        ) -> Result<Option<RouteVersion>, RouteVersioningError> {
            Ok(None)
        }

        async fn list_by_activity(
            &self,
            activity_id: ActivityId,
            cursor: Option<&str>,
            page_size: u32,
        ) -> Result<RouteVersionPage, RouteVersioningError> {
            let versions = self.versions.lock().unwrap();
            let mut matching: Vec<&RouteVersion> = versions
                .iter()
                .filter(|v| v.activity_id == activity_id)
                .collect();

            // Sort by version_number DESC
            matching.sort_by(|a, b| b.version_number.cmp(&a.version_number));

            // Apply cursor (cursor is the version_number to start after)
            let filtered: Vec<&RouteVersion> = if let Some(cursor_str) = cursor {
                let cursor_version: i32 = cursor_str.parse().unwrap_or(0);
                matching
                    .into_iter()
                    .filter(|v| v.version_number < cursor_version)
                    .collect()
            } else {
                matching
            };

            let has_more = filtered.len() as u32 > page_size;
            let items: Vec<RouteVersion> = filtered
                .into_iter()
                .take(page_size as usize)
                .cloned()
                .collect();

            let next_cursor = if has_more {
                items.last().map(|v| v.version_number.to_string())
            } else {
                None
            };

            Ok(RouteVersionPage {
                items,
                next_cursor,
                has_more,
            })
        }
    }

    // --- Test helpers ---

    fn make_activity(owner_id: UserId) -> Activity {
        let title = ActivityTitle::new("Test Activity").unwrap();
        Activity::new(owner_id, title, ActivityType::Hike, None, None)
    }

    fn sample_geometry() -> Vec<Coordinate> {
        vec![
            Coordinate::new(47.0, 11.0).unwrap(),
            Coordinate::new(47.1, 11.1).unwrap(),
            Coordinate::new(47.2, 11.2).unwrap(),
        ]
    }

    fn sample_bounding_box() -> BoundingBox {
        BoundingBox::new(
            Coordinate::new(47.0, 11.0).unwrap(),
            Coordinate::new(47.2, 11.2).unwrap(),
        )
    }

    fn make_route_version(
        activity_id: ActivityId,
        version_number: i32,
        user_id: UserId,
    ) -> RouteVersion {
        RouteVersion {
            id: RouteVersionId::generate(),
            activity_id,
            parent_version_id: if version_number > 1 {
                Some(RouteVersionId::generate())
            } else {
                None
            },
            version_number,
            geometry: sample_geometry(),
            bounding_box: sample_bounding_box(),
            corrected_statistics: CorrectedStatistics::new(1500.0, 3, "v1.0".to_string()),
            calculation_version: "v1.0".to_string(),
            edit_summary: if version_number > 1 {
                Some(format!("Edit for version {version_number}"))
            } else {
                None
            },
            created_by: user_id,
            created_at: Utc::now(),
        }
    }

    // --- list_route_versions tests ---

    #[tokio::test]
    async fn list_returns_versions_ordered_by_version_number_desc() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let v1 = make_route_version(activity_id, 1, owner);
        let v2 = make_route_version(activity_id, 2, owner);
        let v3 = make_route_version(activity_id, 3, owner);

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo =
            FakeRouteVersionRepository::with_versions(vec![v1, v2.clone(), v3.clone()]);

        let page = list_route_versions(
            activity_id,
            owner,
            None,
            None,
            &activity_repo,
            &version_repo,
        )
        .await
        .unwrap();

        assert_eq!(page.items.len(), 3);
        assert_eq!(page.items[0].version_number, 3);
        assert_eq!(page.items[1].version_number, 2);
        assert_eq!(page.items[2].version_number, 1);
        assert!(!page.has_more);
    }

    #[tokio::test]
    async fn list_with_pagination_returns_page_size_items() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let versions: Vec<RouteVersion> = (1..=5)
            .map(|n| make_route_version(activity_id, n, owner))
            .collect();

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(versions);

        let page = list_route_versions(
            activity_id,
            owner,
            None,
            Some(2),
            &activity_repo,
            &version_repo,
        )
        .await
        .unwrap();

        assert_eq!(page.items.len(), 2);
        assert!(page.has_more);
        assert!(page.next_cursor.is_some());
        assert_eq!(page.items[0].version_number, 5);
        assert_eq!(page.items[1].version_number, 4);
    }

    #[tokio::test]
    async fn list_with_cursor_returns_next_page() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let versions: Vec<RouteVersion> = (1..=5)
            .map(|n| make_route_version(activity_id, n, owner))
            .collect();

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(versions);

        // Get first page
        let page1 = list_route_versions(
            activity_id,
            owner,
            None,
            Some(2),
            &activity_repo,
            &version_repo,
        )
        .await
        .unwrap();

        // Get second page using cursor
        let page2 = list_route_versions(
            activity_id,
            owner,
            page1.next_cursor.as_deref(),
            Some(2),
            &activity_repo,
            &version_repo,
        )
        .await
        .unwrap();

        assert_eq!(page2.items.len(), 2);
        assert_eq!(page2.items[0].version_number, 3);
        assert_eq!(page2.items[1].version_number, 2);
    }

    #[tokio::test]
    async fn list_cross_owner_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let v1 = make_route_version(activity_id, 1, owner);

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![v1]);

        let result = list_route_versions(
            activity_id,
            other_user,
            None,
            None,
            &activity_repo,
            &version_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn list_deleted_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let mut activity = make_activity(owner);
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id;

        let v1 = make_route_version(activity_id, 1, owner);

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![v1]);

        let result = list_route_versions(
            activity_id,
            owner,
            None,
            None,
            &activity_repo,
            &version_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn list_nonexistent_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let fake_activity_id = ActivityId::generate();

        let activity_repo = FakeActivityRepository::with_activities(vec![]);
        let version_repo = FakeRouteVersionRepository::new();

        let result = list_route_versions(
            fake_activity_id,
            owner,
            None,
            None,
            &activity_repo,
            &version_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn list_empty_returns_empty_page() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::new();

        let page = list_route_versions(
            activity_id,
            owner,
            None,
            None,
            &activity_repo,
            &version_repo,
        )
        .await
        .unwrap();

        assert_eq!(page.items.len(), 0);
        assert!(!page.has_more);
        assert!(page.next_cursor.is_none());
    }

    // --- get_route_version tests ---

    #[tokio::test]
    async fn get_route_version_returns_data_for_owner() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let version = make_route_version(activity_id, 1, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version.clone()]);

        let result = get_route_version(version_id, owner, &activity_repo, &version_repo)
            .await
            .unwrap();

        assert_eq!(result.id, version_id);
        assert_eq!(result.activity_id, activity_id);
        assert_eq!(result.version_number, 1);
    }

    #[tokio::test]
    async fn get_route_version_cross_owner_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let version = make_route_version(activity_id, 1, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);

        let result = get_route_version(version_id, other_user, &activity_repo, &version_repo).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn get_route_version_nonexistent_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let fake_id = RouteVersionId::generate();

        let activity_repo = FakeActivityRepository::with_activities(vec![]);
        let version_repo = FakeRouteVersionRepository::new();

        let result = get_route_version(fake_id, owner, &activity_repo, &version_repo).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn get_route_version_deleted_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let mut activity = make_activity(owner);
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id;

        let version = make_route_version(activity_id, 1, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);

        let result = get_route_version(version_id, owner, &activity_repo, &version_repo).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    // --- get_route_version_geometry tests ---

    #[tokio::test]
    async fn get_geometry_returns_coordinate_data_for_owner() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let version = make_route_version(activity_id, 1, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);

        let result = get_route_version_geometry(version_id, owner, &activity_repo, &version_repo)
            .await
            .unwrap();

        assert_eq!(result.geometry.len(), 3);
        assert_eq!(result.geometry[0].latitude, 47.0);
        assert_eq!(result.geometry[0].longitude, 11.0);
        assert_eq!(result.bounding_box.south_west.latitude, 47.0);
        assert_eq!(result.bounding_box.north_east.latitude, 47.2);
    }

    #[tokio::test]
    async fn get_geometry_cross_owner_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let version = make_route_version(activity_id, 1, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);

        let result =
            get_route_version_geometry(version_id, other_user, &activity_repo, &version_repo).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn get_geometry_nonexistent_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let fake_id = RouteVersionId::generate();

        let activity_repo = FakeActivityRepository::with_activities(vec![]);
        let version_repo = FakeRouteVersionRepository::new();

        let result =
            get_route_version_geometry(fake_id, owner, &activity_repo, &version_repo).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn get_geometry_includes_corrected_statistics() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let version = make_route_version(activity_id, 1, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);

        let result = get_route_version_geometry(version_id, owner, &activity_repo, &version_repo)
            .await
            .unwrap();

        assert_eq!(result.corrected_statistics.distance_meters, 1500.0);
        assert_eq!(result.corrected_statistics.point_count, 3);
    }
}
