//! Route comparison query handler.
//!
//! Combines recorded route geometry with a selected corrected route version
//! into a single response for the comparison view.

use crate::activity_catalog::repository::ActivityRepository;
use crate::activity_catalog::{ActivityCatalogError, ActivityId, LifecycleState};
use crate::identity::UserId;
use crate::recorded_activity::repository::{RecordedRouteRepository, RouteStatistics};
use crate::recorded_activity::{BoundingBox, Coordinate, RecordedActivityError};

use super::repository::RouteVersionRepository;
use super::{CorrectedStatistics, RouteVersionId, RouteVersioningError};

/// Result of a route comparison query containing both recorded and corrected data.
#[derive(Debug, Clone)]
pub struct RouteComparisonResult {
    /// Recorded route geometry as multiple segments (multi-segment route).
    pub recorded_geometry: Vec<Vec<Coordinate>>,
    /// Bounding box enclosing the recorded route.
    pub recorded_bounding_box: BoundingBox,
    /// Statistics from the recorded route.
    pub recorded_statistics: RecordedRouteStatistics,
    /// Corrected route version geometry (single line).
    pub corrected_geometry: Vec<Coordinate>,
    /// Bounding box enclosing the corrected route.
    pub corrected_bounding_box: BoundingBox,
    /// Statistics from the corrected route version.
    pub corrected_statistics: CorrectedStatistics,
    /// Union bounding box encompassing both recorded and corrected routes.
    pub shared_bounding_box: BoundingBox,
    /// The ID of the route version used for the corrected data.
    pub route_version_id: RouteVersionId,
    /// The version number of the corrected route version.
    pub version_number: i32,
    /// Optional edit summary from the route version.
    pub edit_summary: Option<String>,
}

/// Statistics for the recorded route in the comparison result.
#[derive(Debug, Clone, Copy)]
pub struct RecordedRouteStatistics {
    /// Total distance in meters.
    pub distance_meters: f64,
    /// Total elevation gain in meters.
    pub elevation_gain_meters: Option<f64>,
    /// Total elevation loss in meters.
    pub elevation_loss_meters: Option<f64>,
    /// Total number of points across all segments.
    pub point_count: u32,
    /// Number of segments.
    pub segment_count: u32,
}

impl From<RouteStatistics> for RecordedRouteStatistics {
    fn from(stats: RouteStatistics) -> Self {
        Self {
            distance_meters: stats.distance_meters,
            elevation_gain_meters: stats.elevation_gain_meters,
            elevation_loss_meters: stats.elevation_loss_meters,
            point_count: stats.point_count,
            segment_count: stats.segment_count,
        }
    }
}

/// Compute the union bounding box from two bounding boxes.
///
/// The union is the smallest bounding box that contains both input bounding boxes.
fn compute_shared_bounding_box(a: &BoundingBox, b: &BoundingBox) -> BoundingBox {
    let south_west = Coordinate {
        latitude: a.south_west.latitude.min(b.south_west.latitude),
        longitude: a.south_west.longitude.min(b.south_west.longitude),
    };
    let north_east = Coordinate {
        latitude: a.north_east.latitude.max(b.north_east.latitude),
        longitude: a.north_east.longitude.max(b.north_east.longitude),
    };
    BoundingBox::new(south_west, north_east)
}

/// Get a route comparison combining the recorded route with a corrected route version.
///
/// Verifies activity ownership and then fetches both the recorded route data
/// and the specified route version geometry.
///
/// Returns `NotFound` if:
/// - The activity does not exist
/// - The activity is owned by a different user (non-disclosing)
/// - The activity is deleted
/// - No recorded route data exists for the activity
/// - The specified route version does not exist
/// - The route version does not belong to the specified activity
pub async fn get_route_comparison(
    activity_id: ActivityId,
    route_version_id: RouteVersionId,
    owner_id: UserId,
    activity_repo: &dyn ActivityRepository,
    version_repo: &dyn RouteVersionRepository,
    recorded_route_repo: &dyn RecordedRouteRepository,
) -> Result<RouteComparisonResult, RouteVersioningError> {
    // Verify activity ownership
    verify_activity_ownership(activity_id, owner_id, activity_repo).await?;

    // Fetch the route version
    let version = version_repo
        .find_by_id(route_version_id)
        .await?
        .ok_or(RouteVersioningError::NotFound)?;

    // Verify the route version belongs to this activity
    if version.activity_id != activity_id {
        return Err(RouteVersioningError::NotFound);
    }

    // Fetch the recorded route
    let recorded_route = recorded_route_repo
        .get_recorded_route(activity_id.0)
        .await
        .map_err(|e| match e {
            RecordedActivityError::Persistence { message } => {
                RouteVersioningError::PersistenceError { message }
            }
            _ => RouteVersioningError::NotFound,
        })?
        .ok_or(RouteVersioningError::NotFound)?;

    // Extract recorded geometry as multi-segment
    let recorded_geometry: Vec<Vec<Coordinate>> = recorded_route
        .segments
        .iter()
        .map(|seg| seg.points.clone())
        .collect();

    // Compute shared bounding box
    let shared_bounding_box =
        compute_shared_bounding_box(&recorded_route.bounding_box, &version.bounding_box);

    Ok(RouteComparisonResult {
        recorded_geometry,
        recorded_bounding_box: recorded_route.bounding_box,
        recorded_statistics: recorded_route.statistics.into(),
        corrected_geometry: version.geometry,
        corrected_bounding_box: version.bounding_box,
        corrected_statistics: version.corrected_statistics,
        shared_bounding_box,
        route_version_id: version.id,
        version_number: version.version_number,
        edit_summary: version.edit_summary,
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
    use crate::recorded_activity::repository::{
        RecordedRouteData, RecordedRoutePreview, RouteSegment, RouteStatistics,
    };
    use crate::route_versioning::{RouteVersion, RouteVersionId};

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
            _cursor: Option<&str>,
            _page_size: u32,
        ) -> Result<super::super::repository::RouteVersionPage, RouteVersioningError> {
            let versions = self.versions.lock().unwrap();
            let items: Vec<RouteVersion> = versions
                .iter()
                .filter(|v| v.activity_id == activity_id)
                .cloned()
                .collect();
            Ok(super::super::repository::RouteVersionPage {
                items,
                next_cursor: None,
                has_more: false,
            })
        }
    }

    // --- In-memory RecordedRouteRepository ---

    struct FakeRecordedRouteRepository {
        routes: Mutex<HashMap<Uuid, RecordedRouteData>>,
    }

    impl FakeRecordedRouteRepository {
        fn new() -> Self {
            Self {
                routes: Mutex::new(HashMap::new()),
            }
        }

        fn with_route(activity_id: Uuid, data: RecordedRouteData) -> Self {
            let mut map = HashMap::new();
            map.insert(activity_id, data);
            Self {
                routes: Mutex::new(map),
            }
        }
    }

    #[async_trait]
    impl RecordedRouteRepository for FakeRecordedRouteRepository {
        async fn get_recorded_route(
            &self,
            activity_id: Uuid,
        ) -> Result<Option<RecordedRouteData>, RecordedActivityError> {
            Ok(self.routes.lock().unwrap().get(&activity_id).cloned())
        }

        async fn get_recorded_route_preview(
            &self,
            activity_id: Uuid,
        ) -> Result<Option<RecordedRoutePreview>, RecordedActivityError> {
            Ok(self
                .routes
                .lock()
                .unwrap()
                .get(&activity_id)
                .map(|data| RecordedRoutePreview {
                    activity_id: data.activity_id,
                    bounding_box: data.bounding_box,
                    statistics: data.statistics,
                }))
        }
    }

    // --- Test helpers ---

    fn make_activity(owner_id: UserId) -> Activity {
        let title = ActivityTitle::new("Test Activity").unwrap();
        Activity::new(owner_id, title, ActivityType::Hike, None, None)
    }

    fn make_recorded_route(activity_id: Uuid) -> RecordedRouteData {
        RecordedRouteData {
            activity_id,
            segments: vec![
                RouteSegment {
                    points: vec![
                        Coordinate {
                            latitude: 47.0,
                            longitude: 11.0,
                        },
                        Coordinate {
                            latitude: 47.1,
                            longitude: 11.1,
                        },
                        Coordinate {
                            latitude: 47.2,
                            longitude: 11.2,
                        },
                    ],
                },
                RouteSegment {
                    points: vec![
                        Coordinate {
                            latitude: 47.2,
                            longitude: 11.2,
                        },
                        Coordinate {
                            latitude: 47.3,
                            longitude: 11.3,
                        },
                    ],
                },
            ],
            bounding_box: BoundingBox::new(
                Coordinate {
                    latitude: 47.0,
                    longitude: 11.0,
                },
                Coordinate {
                    latitude: 47.3,
                    longitude: 11.3,
                },
            ),
            statistics: RouteStatistics {
                distance_meters: 5000.0,
                elevation_gain_meters: Some(200.0),
                elevation_loss_meters: Some(150.0),
                point_count: 5,
                segment_count: 2,
            },
        }
    }

    fn make_route_version(activity_id: ActivityId, user_id: UserId) -> RouteVersion {
        RouteVersion {
            id: RouteVersionId::generate(),
            activity_id,
            parent_version_id: None,
            version_number: 1,
            geometry: vec![
                Coordinate::new(47.05, 11.05).unwrap(),
                Coordinate::new(47.15, 11.15).unwrap(),
                Coordinate::new(47.25, 11.25).unwrap(),
            ],
            bounding_box: BoundingBox::new(
                Coordinate::new(47.05, 11.05).unwrap(),
                Coordinate::new(47.25, 11.25).unwrap(),
            ),
            corrected_statistics: CorrectedStatistics::new(4800.0, 3, "v1.0".to_string()),
            calculation_version: "v1.0".to_string(),
            edit_summary: Some("Fixed trail section".to_string()),
            created_by: user_id,
            created_at: Utc::now(),
        }
    }

    // --- Tests ---

    #[tokio::test]
    async fn successful_comparison_returns_both_geometries() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let recorded_route = make_recorded_route(activity_id.0);
        let version = make_route_version(activity_id, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);
        let recorded_route_repo =
            FakeRecordedRouteRepository::with_route(activity_id.0, recorded_route);

        let result = get_route_comparison(
            activity_id,
            version_id,
            owner,
            &activity_repo,
            &version_repo,
            &recorded_route_repo,
        )
        .await
        .unwrap();

        // Verify recorded data
        assert_eq!(result.recorded_geometry.len(), 2); // 2 segments
        assert_eq!(result.recorded_geometry[0].len(), 3); // first segment has 3 points
        assert_eq!(result.recorded_geometry[1].len(), 2); // second segment has 2 points
        assert_eq!(result.recorded_statistics.distance_meters, 5000.0);
        assert_eq!(result.recorded_statistics.elevation_gain_meters, Some(200.0));
        assert_eq!(result.recorded_statistics.elevation_loss_meters, Some(150.0));
        assert_eq!(result.recorded_statistics.point_count, 5);
        assert_eq!(result.recorded_statistics.segment_count, 2);

        // Verify corrected data
        assert_eq!(result.corrected_geometry.len(), 3);
        assert_eq!(result.corrected_statistics.distance_meters, 4800.0);
        assert_eq!(result.corrected_statistics.point_count, 3);

        // Verify metadata
        assert_eq!(result.route_version_id, version_id);
        assert_eq!(result.version_number, 1);
        assert_eq!(result.edit_summary, Some("Fixed trail section".to_string()));
    }

    #[tokio::test]
    async fn cross_owner_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let recorded_route = make_recorded_route(activity_id.0);
        let version = make_route_version(activity_id, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);
        let recorded_route_repo =
            FakeRecordedRouteRepository::with_route(activity_id.0, recorded_route);

        let result = get_route_comparison(
            activity_id,
            version_id,
            other_user,
            &activity_repo,
            &version_repo,
            &recorded_route_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn nonexistent_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let fake_activity_id = ActivityId::generate();
        let fake_version_id = RouteVersionId::generate();

        let activity_repo = FakeActivityRepository::with_activities(vec![]);
        let version_repo = FakeRouteVersionRepository::new();
        let recorded_route_repo = FakeRecordedRouteRepository::new();

        let result = get_route_comparison(
            fake_activity_id,
            fake_version_id,
            owner,
            &activity_repo,
            &version_repo,
            &recorded_route_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn nonexistent_route_version_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let recorded_route = make_recorded_route(activity_id.0);
        let fake_version_id = RouteVersionId::generate();

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::new();
        let recorded_route_repo =
            FakeRecordedRouteRepository::with_route(activity_id.0, recorded_route);

        let result = get_route_comparison(
            activity_id,
            fake_version_id,
            owner,
            &activity_repo,
            &version_repo,
            &recorded_route_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn deleted_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let mut activity = make_activity(owner);
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id;

        let recorded_route = make_recorded_route(activity_id.0);
        let version = make_route_version(activity_id, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);
        let recorded_route_repo =
            FakeRecordedRouteRepository::with_route(activity_id.0, recorded_route);

        let result = get_route_comparison(
            activity_id,
            version_id,
            owner,
            &activity_repo,
            &version_repo,
            &recorded_route_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }

    #[tokio::test]
    async fn shared_bounding_box_is_union_of_recorded_and_corrected() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        let recorded_route = make_recorded_route(activity_id.0);
        let version = make_route_version(activity_id, owner);
        let version_id = version.id;

        let activity_repo = FakeActivityRepository::with_activities(vec![activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);
        let recorded_route_repo =
            FakeRecordedRouteRepository::with_route(activity_id.0, recorded_route);

        let result = get_route_comparison(
            activity_id,
            version_id,
            owner,
            &activity_repo,
            &version_repo,
            &recorded_route_repo,
        )
        .await
        .unwrap();

        // Recorded bbox: SW(47.0, 11.0) NE(47.3, 11.3)
        // Corrected bbox: SW(47.05, 11.05) NE(47.25, 11.25)
        // Shared bbox should be: SW(47.0, 11.0) NE(47.3, 11.3) (union = min of SWs, max of NEs)
        assert_eq!(result.shared_bounding_box.south_west.latitude, 47.0);
        assert_eq!(result.shared_bounding_box.south_west.longitude, 11.0);
        assert_eq!(result.shared_bounding_box.north_east.latitude, 47.3);
        assert_eq!(result.shared_bounding_box.north_east.longitude, 11.3);

        // Individual bounding boxes should be preserved
        assert_eq!(result.recorded_bounding_box.south_west.latitude, 47.0);
        assert_eq!(result.recorded_bounding_box.north_east.latitude, 47.3);
        assert_eq!(result.corrected_bounding_box.south_west.latitude, 47.05);
        assert_eq!(result.corrected_bounding_box.north_east.latitude, 47.25);
    }

    #[tokio::test]
    async fn route_version_for_different_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner);
        let activity_id = activity.id;

        // Create another activity with a route version
        let other_activity = make_activity(owner);
        let other_activity_id = other_activity.id;

        let recorded_route = make_recorded_route(activity_id.0);
        let version = make_route_version(other_activity_id, owner); // belongs to other activity
        let version_id = version.id;

        let activity_repo =
            FakeActivityRepository::with_activities(vec![activity, other_activity]);
        let version_repo = FakeRouteVersionRepository::with_versions(vec![version]);
        let recorded_route_repo =
            FakeRecordedRouteRepository::with_route(activity_id.0, recorded_route);

        let result = get_route_comparison(
            activity_id,
            version_id,
            owner,
            &activity_repo,
            &version_repo,
            &recorded_route_repo,
        )
        .await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotFound);
    }
}
