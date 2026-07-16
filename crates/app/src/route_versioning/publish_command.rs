//! Publication command for creating route version N+1 from a draft.
//!
//! Implements the full publication workflow as pure domain logic:
//! load draft, authorize, validate, compute statistics, create version,
//! mark draft published, update activity pointer.

use crate::identity::UserId;
use crate::recorded_activity::{BoundingBox, Coordinate};
use crate::route_editing::validation::{validate_for_publication, PublicationValidationError};
use crate::route_editing::{RouteDraftId, RouteDraftRepository, RoutePoint};

use super::gateway::PublicationGateway;
use super::repository::RouteVersionRepository;
use super::{RouteVersion, RouteVersioningError};

/// Command to publish a route draft as a new route version.
#[derive(Debug, Clone)]
pub struct PublishRouteVersionCommand {
    /// The draft to publish.
    pub draft_id: RouteDraftId,
    /// Expected revision of the draft for optimistic concurrency.
    pub expected_revision: u64,
    /// The user performing the publication.
    pub actor_id: UserId,
    /// Idempotency key to prevent duplicate publications.
    pub idempotency_key: String,
    /// Optional human-readable summary of edits.
    pub edit_summary: Option<String>,
}

/// Execute the publish route version command.
///
/// Orchestrates the publication workflow:
/// 1. Check idempotency (return existing version if already published with this key)
/// 2. Load the draft
/// 3. Validate for publication (owner, state, revision, geometry)
/// 4. Compute corrected statistics from the geometry
/// 5. Find the latest version for the activity to determine version_number
/// 6. Create a new RouteVersion (N+1)
/// 7. Save the new version
/// 8. Mark draft as published
/// 9. Update activity current version pointer
/// 10. Return the new RouteVersion
pub async fn execute_publish(
    command: PublishRouteVersionCommand,
    draft_repo: &dyn RouteDraftRepository,
    version_repo: &dyn RouteVersionRepository,
    gateway: &dyn PublicationGateway,
) -> Result<RouteVersion, RouteVersioningError> {
    // 1. Idempotency check
    if let Some(existing) = version_repo
        .find_by_idempotency_key(&command.idempotency_key)
        .await?
    {
        return Ok(existing);
    }

    // 2. Load the draft
    let mut draft = draft_repo
        .find_by_id(command.draft_id)
        .await
        .map_err(|_| RouteVersioningError::DraftNotFound)?
        .ok_or(RouteVersioningError::DraftNotFound)?;

    // 3. Validate for publication (owner, state, revision, geometry)
    validate_for_publication(&draft, command.expected_revision, command.actor_id)
        .map_err(map_validation_errors)?;

    // 4. Compute corrected statistics from the draft geometry
    let flat_geometry = flatten_geometry(&draft.geometry);
    let bounding_box = BoundingBox::from_coordinates(&flat_geometry).ok_or(
        RouteVersioningError::ValidationFailed {
            errors: vec!["geometry produced no bounding box".to_string()],
        },
    )?;

    let total_distance = compute_total_distance(&flat_geometry);
    let corrected_statistics = serde_json::json!({
        "distance_meters": total_distance,
        "point_count": flat_geometry.len(),
    });

    // 5. Find the latest version for the activity
    let latest_version = version_repo
        .find_latest_by_activity(draft.activity_id)
        .await?
        .ok_or(RouteVersioningError::ActivityNotFound)?;

    let new_version_number = latest_version.version_number + 1;

    // 6. Create a new RouteVersion (N+1)
    let new_version = RouteVersion::new_from_publication(
        latest_version.id,
        new_version_number,
        draft.activity_id,
        flat_geometry,
        bounding_box,
        corrected_statistics,
        "v1.0".to_string(),
        command.edit_summary,
        command.actor_id,
    )?;

    // 7. Save the new version
    version_repo.save(&new_version).await?;

    // 8. Mark draft as published
    draft
        .publish()
        .map_err(|_| RouteVersioningError::DraftNotActive)?;
    draft_repo
        .update(&draft)
        .await
        .map_err(|_| RouteVersioningError::PersistenceError {
            message: "failed to update draft".to_string(),
        })?;

    // 9. Update activity current version pointer
    gateway
        .update_activity_current_version(draft.activity_id, new_version.id)
        .await?;

    // 10. Return the new RouteVersion
    Ok(new_version)
}

/// Flatten draft geometry (Vec<Vec<RoutePoint>>) into a flat list of Coordinates.
///
/// Converts from route_editing::Coordinate to recorded_activity::Coordinate,
/// extracting just lat/lon from each RoutePoint.
fn flatten_geometry(segments: &[Vec<RoutePoint>]) -> Vec<Coordinate> {
    segments
        .iter()
        .flat_map(|segment| segment.iter())
        .map(|point| Coordinate {
            latitude: point.coordinate.latitude,
            longitude: point.coordinate.longitude,
        })
        .collect()
}

/// Compute total distance in meters using haversine formula.
/// This is a stub implementation for initial publication.
fn compute_total_distance(coords: &[Coordinate]) -> f64 {
    if coords.len() < 2 {
        return 0.0;
    }

    let mut total = 0.0;
    for window in coords.windows(2) {
        total += haversine_distance(&window[0], &window[1]);
    }
    total
}

/// Haversine distance between two coordinates in meters.
fn haversine_distance(a: &Coordinate, b: &Coordinate) -> f64 {
    let r = 6_371_000.0; // Earth radius in meters
    let lat1 = a.latitude.to_radians();
    let lat2 = b.latitude.to_radians();
    let dlat = (b.latitude - a.latitude).to_radians();
    let dlon = (b.longitude - a.longitude).to_radians();

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

/// Map publication validation errors to RouteVersioningError.
fn map_validation_errors(errors: Vec<PublicationValidationError>) -> RouteVersioningError {
    // Short-circuit errors map to specific error types
    if errors.len() == 1 {
        match &errors[0] {
            PublicationValidationError::NotOwner => return RouteVersioningError::NotAuthorized,
            PublicationValidationError::DraftNotActive => {
                return RouteVersioningError::DraftNotActive
            }
            PublicationValidationError::RevisionMismatch { expected, actual } => {
                return RouteVersioningError::RevisionConflict {
                    expected: *expected,
                    actual: *actual,
                }
            }
            _ => {}
        }
    }

    // Geometry errors are collected into ValidationFailed
    let error_messages: Vec<String> = errors
        .into_iter()
        .map(|e| match e {
            PublicationValidationError::NotOwner => "not the draft owner".to_string(),
            PublicationValidationError::DraftNotActive => "draft is not active".to_string(),
            PublicationValidationError::RevisionMismatch { expected, actual } => {
                format!("revision mismatch: expected {expected}, actual {actual}")
            }
            PublicationValidationError::NoSegments => "geometry has no segments".to_string(),
            PublicationValidationError::InsufficientPointsInSegment {
                segment_index,
                minimum,
                actual,
            } => {
                format!(
                    "segment {segment_index} has insufficient points: minimum {minimum}, got {actual}"
                )
            }
            PublicationValidationError::InvalidCoordinateInSegment {
                segment_index,
                point_index,
                message,
            } => {
                format!("segment {segment_index}, point {point_index}: {message}")
            }
            PublicationValidationError::NoBaseVersion => "no base version set".to_string(),
        })
        .collect();

    RouteVersioningError::ValidationFailed {
        errors: error_messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_catalog::ActivityId;
    use crate::identity::UserId;
    use crate::recorded_activity::{BoundingBox, Coordinate as RecCoordinate};
    use crate::route_editing::DraftState;
    use crate::route_editing::{
        Coordinate as EditCoordinate, RouteDraft, RouteDraftId, RoutePoint,
    };
    use crate::route_versioning::{RouteVersion, RouteVersionId, RouteVersioningError};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    use crate::route_editing::RouteEditingError;

    // --- Fake RouteDraftRepository ---

    struct FakeDraftRepo {
        drafts: Mutex<HashMap<RouteDraftId, RouteDraft>>,
    }

    impl FakeDraftRepo {
        fn with(draft: RouteDraft) -> Self {
            let mut map = HashMap::new();
            map.insert(draft.id, draft);
            Self {
                drafts: Mutex::new(map),
            }
        }

        fn empty() -> Self {
            Self {
                drafts: Mutex::new(HashMap::new()),
            }
        }

        fn get(&self, id: RouteDraftId) -> Option<RouteDraft> {
            self.drafts.lock().unwrap().get(&id).cloned()
        }
    }

    #[async_trait]
    impl RouteDraftRepository for FakeDraftRepo {
        async fn save(&self, draft: &RouteDraft) -> Result<(), RouteEditingError> {
            self.drafts.lock().unwrap().insert(draft.id, draft.clone());
            Ok(())
        }

        async fn find_by_id(
            &self,
            id: RouteDraftId,
        ) -> Result<Option<RouteDraft>, RouteEditingError> {
            Ok(self.drafts.lock().unwrap().get(&id).cloned())
        }

        async fn find_active_by_activity(
            &self,
            _activity_id: ActivityId,
            _owner_id: UserId,
        ) -> Result<Option<RouteDraft>, RouteEditingError> {
            unimplemented!()
        }

        async fn update(&self, draft: &RouteDraft) -> Result<(), RouteEditingError> {
            self.drafts.lock().unwrap().insert(draft.id, draft.clone());
            Ok(())
        }

        async fn find_by_operation_id(
            &self,
            _operation_id: crate::route_editing::OperationId,
        ) -> Result<Option<RouteDraftId>, RouteEditingError> {
            unimplemented!()
        }
    }

    // --- Fake RouteVersionRepository ---

    struct FakeVersionRepo {
        versions: Mutex<Vec<RouteVersion>>,
        idempotency_keys: Mutex<HashMap<String, RouteVersion>>,
    }

    impl FakeVersionRepo {
        fn new() -> Self {
            Self {
                versions: Mutex::new(Vec::new()),
                idempotency_keys: Mutex::new(HashMap::new()),
            }
        }

        fn with_version(version: RouteVersion) -> Self {
            let repo = Self::new();
            repo.versions.lock().unwrap().push(version);
            repo
        }

        fn with_idempotency(key: String, version: RouteVersion) -> Self {
            let repo = Self::new();
            repo.versions.lock().unwrap().push(version.clone());
            repo.idempotency_keys.lock().unwrap().insert(key, version);
            repo
        }

        fn saved_versions(&self) -> Vec<RouteVersion> {
            self.versions.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RouteVersionRepository for FakeVersionRepo {
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
            key: &str,
        ) -> Result<Option<RouteVersion>, RouteVersioningError> {
            Ok(self.idempotency_keys.lock().unwrap().get(key).cloned())
        }
    }

    // --- Fake PublicationGateway ---

    struct FakeGateway {
        calls: Mutex<Vec<(ActivityId, RouteVersionId)>>,
    }

    impl FakeGateway {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(ActivityId, RouteVersionId)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PublicationGateway for FakeGateway {
        async fn update_activity_current_version(
            &self,
            activity_id: ActivityId,
            route_version_id: RouteVersionId,
        ) -> Result<(), RouteVersioningError> {
            self.calls
                .lock()
                .unwrap()
                .push((activity_id, route_version_id));
            Ok(())
        }
    }

    // --- Test helpers ---

    fn make_valid_draft(owner: UserId, activity_id: ActivityId) -> RouteDraft {
        let geometry = vec![vec![
            RoutePoint::new(EditCoordinate::new(47.0, 11.0).unwrap(), None),
            RoutePoint::new(EditCoordinate::new(47.1, 11.1).unwrap(), None),
            RoutePoint::new(EditCoordinate::new(47.2, 11.2).unwrap(), None),
        ]];

        RouteDraft::create_from_geometry(owner, activity_id, Some(Uuid::new_v4()), geometry)
            .unwrap()
    }

    fn make_initial_version(activity_id: ActivityId, user_id: UserId) -> RouteVersion {
        RouteVersion::new_initial(
            activity_id,
            vec![
                RecCoordinate::new(47.0, 11.0).unwrap(),
                RecCoordinate::new(47.1, 11.1).unwrap(),
            ],
            BoundingBox::new(
                RecCoordinate::new(47.0, 11.0).unwrap(),
                RecCoordinate::new(47.1, 11.1).unwrap(),
            ),
            serde_json::json!({"distance_meters": 1000.0}),
            "v1.0".to_string(),
            user_id,
        )
        .unwrap()
    }

    // --- Tests ---

    #[tokio::test]
    async fn successful_publication_creates_version_n_plus_1() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::generate();
        let draft = make_valid_draft(owner, activity_id);
        let draft_id = draft.id;

        let initial_version = make_initial_version(activity_id, owner);
        let initial_version_id = initial_version.id;

        let draft_repo = FakeDraftRepo::with(draft);
        let version_repo = FakeVersionRepo::with_version(initial_version);
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id,
            expected_revision: 0,
            actor_id: owner,
            idempotency_key: "publish-1".to_string(),
            edit_summary: Some("Fixed trail section".to_string()),
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        assert!(result.is_ok());
        let new_version = result.unwrap();
        assert_eq!(new_version.version_number, 2);
        assert_eq!(new_version.parent_version_id, Some(initial_version_id));
        assert_eq!(new_version.activity_id, activity_id);
        assert_eq!(new_version.created_by, owner);
        assert_eq!(
            new_version.edit_summary,
            Some("Fixed trail section".to_string())
        );
        assert!(new_version.geometry.len() >= 2);

        // Verify draft is now published
        let updated_draft = draft_repo.get(draft_id).unwrap();
        assert_eq!(updated_draft.state, DraftState::Published);

        // Verify gateway was called
        let gateway_calls = gateway.calls();
        assert_eq!(gateway_calls.len(), 1);
        assert_eq!(gateway_calls[0].0, activity_id);
        assert_eq!(gateway_calls[0].1, new_version.id);

        // Verify version was saved
        let saved = version_repo.saved_versions();
        assert_eq!(saved.len(), 2); // initial + new
    }

    #[tokio::test]
    async fn cross_owner_fails_with_not_authorized() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::generate();
        let draft = make_valid_draft(owner, activity_id);
        let draft_id = draft.id;

        let initial_version = make_initial_version(activity_id, owner);

        let draft_repo = FakeDraftRepo::with(draft);
        let version_repo = FakeVersionRepo::with_version(initial_version);
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id,
            expected_revision: 0,
            actor_id: other_user,
            idempotency_key: "publish-2".to_string(),
            edit_summary: None,
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::NotAuthorized);
    }

    #[tokio::test]
    async fn revision_mismatch_returns_conflict() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::generate();
        let draft = make_valid_draft(owner, activity_id);
        let draft_id = draft.id;

        let initial_version = make_initial_version(activity_id, owner);

        let draft_repo = FakeDraftRepo::with(draft);
        let version_repo = FakeVersionRepo::with_version(initial_version);
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id,
            expected_revision: 99, // Wrong revision
            actor_id: owner,
            idempotency_key: "publish-3".to_string(),
            edit_summary: None,
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        assert_eq!(
            result.unwrap_err(),
            RouteVersioningError::RevisionConflict {
                expected: 99,
                actual: 0,
            }
        );
    }

    #[tokio::test]
    async fn already_published_draft_fails() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::generate();
        let mut draft = make_valid_draft(owner, activity_id);
        draft.publish().unwrap(); // Mark as published
        let draft_id = draft.id;

        let initial_version = make_initial_version(activity_id, owner);

        let draft_repo = FakeDraftRepo::with(draft);
        let version_repo = FakeVersionRepo::with_version(initial_version);
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id,
            expected_revision: 0,
            actor_id: owner,
            idempotency_key: "publish-4".to_string(),
            edit_summary: None,
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::DraftNotActive);
    }

    #[tokio::test]
    async fn idempotent_replay_returns_existing_version() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::generate();
        let draft = make_valid_draft(owner, activity_id);
        let draft_id = draft.id;

        let initial_version = make_initial_version(activity_id, owner);
        let existing_published = RouteVersion::new_from_publication(
            initial_version.id,
            2,
            activity_id,
            vec![
                RecCoordinate::new(47.0, 11.0).unwrap(),
                RecCoordinate::new(47.1, 11.1).unwrap(),
            ],
            BoundingBox::new(
                RecCoordinate::new(47.0, 11.0).unwrap(),
                RecCoordinate::new(47.1, 11.1).unwrap(),
            ),
            serde_json::json!({"distance_meters": 500.0}),
            "v1.0".to_string(),
            Some("Previous edit".to_string()),
            owner,
        )
        .unwrap();

        let existing_id = existing_published.id;

        let draft_repo = FakeDraftRepo::with(draft);
        let version_repo =
            FakeVersionRepo::with_idempotency("publish-5".to_string(), existing_published);
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id,
            expected_revision: 0,
            actor_id: owner,
            idempotency_key: "publish-5".to_string(),
            edit_summary: None,
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        assert!(result.is_ok());
        let version = result.unwrap();
        assert_eq!(version.id, existing_id);

        // Gateway should NOT be called for idempotent replay
        assert_eq!(gateway.calls().len(), 0);
    }

    #[tokio::test]
    async fn draft_not_found_returns_error() {
        let owner = UserId::new(Uuid::new_v4());
        let draft_repo = FakeDraftRepo::empty();
        let version_repo = FakeVersionRepo::new();
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id: RouteDraftId::generate(),
            expected_revision: 0,
            actor_id: owner,
            idempotency_key: "publish-6".to_string(),
            edit_summary: None,
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::DraftNotFound);
    }

    #[tokio::test]
    async fn geometry_validation_fails_with_errors() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::generate();

        // Create a draft with invalid geometry (empty segments)
        let geometry = vec![vec![
            RoutePoint::new(EditCoordinate::new(47.0, 11.0).unwrap(), None),
            RoutePoint::new(EditCoordinate::new(47.1, 11.1).unwrap(), None),
        ]];

        let mut draft =
            RouteDraft::create_from_geometry(owner, activity_id, Some(Uuid::new_v4()), geometry)
                .unwrap();

        // Manually set empty geometry to trigger validation failure
        draft.geometry = vec![];

        let draft_id = draft.id;
        let initial_version = make_initial_version(activity_id, owner);

        let draft_repo = FakeDraftRepo::with(draft);
        let version_repo = FakeVersionRepo::with_version(initial_version);
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id,
            expected_revision: 0,
            actor_id: owner,
            idempotency_key: "publish-7".to_string(),
            edit_summary: None,
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        match result.unwrap_err() {
            RouteVersioningError::ValidationFailed { errors } => {
                assert!(!errors.is_empty());
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_existing_version_returns_activity_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let activity_id = ActivityId::generate();
        let draft = make_valid_draft(owner, activity_id);
        let draft_id = draft.id;

        // No initial version exists for this activity
        let draft_repo = FakeDraftRepo::with(draft);
        let version_repo = FakeVersionRepo::new();
        let gateway = FakeGateway::new();

        let command = PublishRouteVersionCommand {
            draft_id,
            expected_revision: 0,
            actor_id: owner,
            idempotency_key: "publish-8".to_string(),
            edit_summary: None,
        };

        let result = execute_publish(command, &draft_repo, &version_repo, &gateway).await;

        assert_eq!(result.unwrap_err(), RouteVersioningError::ActivityNotFound);
    }
}
