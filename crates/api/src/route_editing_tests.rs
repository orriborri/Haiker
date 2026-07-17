use super::*;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::Request;
use axum::routing::{get, post};
use axum::Router;
use haiker_app::activity_catalog::{ActivityId, LifecycleState};
use haiker_app::identity::UserId;
use haiker_app::route_editing::{
    ActivityGateway, DraftState, OperationId, RouteDraft, RouteDraftId, RouteDraftRepository,
    RouteEditingError, RoutePoint, RouteVersionGateway,
};
use std::collections::HashMap;
use std::sync::Mutex;
use tower::ServiceExt;

/// Ensure DEV_AUTH_ENABLED is set so AuthSession accepts Bearer UUID tokens.
fn ensure_dev_auth() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("DEV_AUTH_ENABLED", "true");
    });
}

/// Create a dummy SessionStore for tests.
fn dummy_session_store() -> SessionStore {
    let pool = sqlx::PgPool::connect_lazy("postgres://test:test@localhost/test").unwrap();
    SessionStore::new(pool)
}

/// In-memory route draft repository for testing.
pub struct InMemoryRouteDraftRepository {
    drafts: Mutex<HashMap<RouteDraftId, RouteDraft>>,
}

impl InMemoryRouteDraftRepository {
    pub fn new() -> Self {
        Self {
            drafts: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl RouteDraftRepository for InMemoryRouteDraftRepository {
    async fn save(&self, draft: &RouteDraft) -> Result<(), RouteEditingError> {
        self.drafts.lock().unwrap().insert(draft.id, draft.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: RouteDraftId) -> Result<Option<RouteDraft>, RouteEditingError> {
        Ok(self.drafts.lock().unwrap().get(&id).cloned())
    }

    async fn find_active_by_activity(
        &self,
        activity_id: ActivityId,
        owner_id: UserId,
    ) -> Result<Option<RouteDraft>, RouteEditingError> {
        Ok(self
            .drafts
            .lock()
            .unwrap()
            .values()
            .find(|d| {
                d.activity_id == activity_id
                    && d.owner_id == owner_id
                    && d.state == DraftState::Active
            })
            .cloned())
    }

    async fn update(&self, draft: &RouteDraft) -> Result<(), RouteEditingError> {
        self.drafts.lock().unwrap().insert(draft.id, draft.clone());
        Ok(())
    }

    async fn find_by_operation_id(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<RouteDraftId>, RouteEditingError> {
        Ok(self
            .drafts
            .lock()
            .unwrap()
            .values()
            .find(|d| d.applied_operations.iter().any(|e| e.id == operation_id))
            .map(|d| d.id))
    }
}

/// Represents a known activity in the in-memory gateway.
#[derive(Debug, Clone)]
struct KnownActivity {
    owner_id: UserId,
    lifecycle_state: LifecycleState,
}

/// In-memory activity gateway for testing.
///
/// By default (empty map), it permits all activity validations (always succeeds).
/// When activities are explicitly registered, it validates against them.
pub struct InMemoryActivityGateway {
    /// Map of activity_id -> KnownActivity. If empty, all validations pass.
    activities: Mutex<HashMap<ActivityId, KnownActivity>>,
    /// When true, an empty map means "permit all". When false, empty map means "deny all".
    permit_when_empty: bool,
}

impl InMemoryActivityGateway {
    /// Create a gateway that permits all validations (for backward-compatible tests).
    pub fn permissive() -> Self {
        Self {
            activities: Mutex::new(HashMap::new()),
            permit_when_empty: true,
        }
    }

    /// Create a gateway with explicit activity entries.
    pub fn with_activities(activities: Vec<(ActivityId, UserId, LifecycleState)>) -> Self {
        let map = activities
            .into_iter()
            .map(|(id, owner_id, state)| {
                (
                    id,
                    KnownActivity {
                        owner_id,
                        lifecycle_state: state,
                    },
                )
            })
            .collect();
        Self {
            activities: Mutex::new(map),
            permit_when_empty: false,
        }
    }
}

#[async_trait]
impl ActivityGateway for InMemoryActivityGateway {
    async fn validate_activity_for_draft(
        &self,
        activity_id: ActivityId,
        owner_id: UserId,
    ) -> Result<(), RouteEditingError> {
        let activities = self.activities.lock().unwrap();
        if activities.is_empty() && self.permit_when_empty {
            return Ok(());
        }
        match activities.get(&activity_id) {
            None => Err(RouteEditingError::ActivityNotFound),
            Some(activity) => {
                if activity.owner_id != owner_id {
                    // Non-disclosing: treat as not found
                    Err(RouteEditingError::ActivityNotFound)
                } else if activity.lifecycle_state == LifecycleState::Deleted {
                    Err(RouteEditingError::ActivityDeleted)
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// In-memory route version gateway for testing.
///
/// By default (empty set), it permits all validations (always succeeds).
/// When versions are explicitly registered, it validates against them.
pub struct InMemoryRouteVersionGateway {
    /// Set of valid (route_version_id, activity_id) pairs with optional geometry.
    #[allow(clippy::type_complexity)]
    valid_versions: Mutex<Vec<(Uuid, ActivityId, Option<Vec<Vec<RoutePoint>>>)>>,
    /// When true, an empty set means "permit all". When false, empty set means "deny all".
    permit_when_empty: bool,
}

impl InMemoryRouteVersionGateway {
    /// Create a gateway that permits all validations (for backward-compatible tests).
    pub fn permissive() -> Self {
        Self {
            valid_versions: Mutex::new(Vec::new()),
            permit_when_empty: true,
        }
    }

    /// Create a gateway with explicit valid version entries (no geometry stored).
    pub fn with_versions(versions: Vec<(Uuid, ActivityId)>) -> Self {
        Self {
            valid_versions: Mutex::new(
                versions
                    .into_iter()
                    .map(|(vid, aid)| (vid, aid, None))
                    .collect(),
            ),
            permit_when_empty: false,
        }
    }

    /// Create a gateway with explicit valid version entries including geometry.
    pub fn with_versions_and_geometry(
        versions: Vec<(Uuid, ActivityId, Vec<Vec<RoutePoint>>)>,
    ) -> Self {
        Self {
            valid_versions: Mutex::new(
                versions
                    .into_iter()
                    .map(|(vid, aid, geo)| (vid, aid, Some(geo)))
                    .collect(),
            ),
            permit_when_empty: false,
        }
    }
}

#[async_trait]
impl RouteVersionGateway for InMemoryRouteVersionGateway {
    async fn validate_route_version_exists(
        &self,
        route_version_id: Uuid,
        activity_id: ActivityId,
    ) -> Result<(), RouteEditingError> {
        let versions = self.valid_versions.lock().unwrap();
        if versions.is_empty() && self.permit_when_empty {
            return Ok(());
        }
        if versions
            .iter()
            .any(|(vid, aid, _)| *vid == route_version_id && *aid == activity_id)
        {
            Ok(())
        } else {
            Err(RouteEditingError::InvalidBaseRouteVersion)
        }
    }

    async fn get_route_version_geometry(
        &self,
        route_version_id: Uuid,
        activity_id: ActivityId,
    ) -> Result<Vec<Vec<RoutePoint>>, RouteEditingError> {
        let versions = self.valid_versions.lock().unwrap();
        if versions.is_empty() && self.permit_when_empty {
            // In permissive mode without stored geometry, return a default geometry
            return Ok(vec![vec![
                RoutePoint::new(
                    haiker_app::route_editing::Coordinate::new(47.0, 11.0).unwrap(),
                    None,
                ),
                RoutePoint::new(
                    haiker_app::route_editing::Coordinate::new(47.1, 11.1).unwrap(),
                    None,
                ),
                RoutePoint::new(
                    haiker_app::route_editing::Coordinate::new(47.2, 11.2).unwrap(),
                    None,
                ),
            ]]);
        }
        for (vid, aid, geo) in versions.iter() {
            if *vid == route_version_id && *aid == activity_id {
                if let Some(geometry) = geo {
                    return Ok(geometry.clone());
                }
                return Err(RouteEditingError::InvalidBaseRouteVersion);
            }
        }
        Err(RouteEditingError::InvalidBaseRouteVersion)
    }
}

fn test_app() -> Router {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    test_app_with_state(state)
}

fn test_app_with_state(state: RouteEditingAppState) -> Router {
    Router::new()
        .route(
            "/v1/activities/{activityId}/route-drafts",
            post(post_create_draft),
        )
        .route(
            "/v1/route-drafts/{draftId}",
            get(get_draft).delete(delete_draft),
        )
        .route(
            "/v1/route-drafts/{draftId}/operations",
            post(post_apply_operation),
        )
        .route("/v1/route-drafts/{draftId}/undo", post(post_undo))
        .route("/v1/route-drafts/{draftId}/redo", post(post_redo))
        .route("/v1/route-drafts/{draftId}/reset", post(post_reset))
        .route(
            "/v1/route-drafts/{draftId}/validation",
            post(post_validate_draft),
        )
        .route(
            "/v1/route-drafts/{draftId}/publication",
            post(post_publish_draft),
        )
        .with_state(state)
}

fn sample_geometry() -> serde_json::Value {
    serde_json::json!([
        [
            {"latitude": 47.0, "longitude": 11.0, "elevation": 500.0},
            {"latitude": 47.1, "longitude": 11.1, "elevation": 600.0},
            {"latitude": 47.2, "longitude": 11.2, "elevation": 700.0}
        ]
    ])
}

fn auth_header() -> (String, String) {
    let user_id = Uuid::new_v4();
    ("Authorization".to_string(), format!("Bearer {user_id}"))
}

async fn create_draft_for_user(
    app: &Router,
    user_id: Uuid,
    activity_id: Uuid,
) -> serde_json::Value {
    let body = serde_json::json!({
        "geometry": sample_geometry()
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{activity_id}/route-drafts"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&b).unwrap()
}

#[tokio::test]
async fn create_draft_returns_201() {
    let app = test_app();
    let (auth_key, auth_val) = auth_header();
    let activity_id = Uuid::new_v4();

    let body = serde_json::json!({
        "geometry": sample_geometry()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{activity_id}/route-drafts"))
                .header(&auth_key, &auth_val)
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert!(json["id"].is_string());
    assert_eq!(json["activityId"], activity_id.to_string());
    assert_eq!(json["revision"], 0);
    assert_eq!(json["state"], "active");
}

#[tokio::test]
async fn get_draft_returns_200() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["id"], draft_id);
    assert_eq!(json["revision"], 0);
}

#[tokio::test]
async fn apply_operation_returns_200_with_new_revision() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let op_id = Uuid::new_v4();
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", op_id.to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 1);
    assert_eq!(json["draftId"], draft_id);
}

#[tokio::test]
async fn apply_with_stale_revision_returns_409() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Apply first operation to bump revision to 1
    let body1 = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body1).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Try applying with stale revision 0
    let body2 = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 1,
            "newPosition": {"latitude": 48.5, "longitude": 12.5}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn apply_with_duplicate_idempotency_key_replays_response() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let op_id = Uuid::new_v4();
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    // First request
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", op_id.to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);
    let b1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    let json1: serde_json::Value = serde_json::from_slice(&b1).unwrap();

    // Second request with same idempotency key
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", op_id.to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);
    let b2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    let json2: serde_json::Value = serde_json::from_slice(&b2).unwrap();

    // Same revision returned (idempotent replay)
    assert_eq!(json1["revision"], json2["revision"]);
    assert_eq!(json1["draftId"], json2["draftId"]);
}

#[tokio::test]
async fn undo_redo_reset_work() {
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    // Base geometry that the reset will restore from the gateway
    let base_geometry = vec![vec![
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.0, 11.0).unwrap(),
            Some(haiker_app::route_editing::Elevation::new(500.0)),
        ),
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.1, 11.1).unwrap(),
            Some(haiker_app::route_editing::Elevation::new(600.0)),
        ),
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.2, 11.2).unwrap(),
            Some(haiker_app::route_editing::Elevation::new(700.0)),
        ),
    ]];

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions_and_geometry(
            vec![(version_id, activity_id, base_geometry)],
        )),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create draft with baseRouteVersionId
    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // Apply an operation (revision 0 -> 1)
    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Undo (revision 1 -> 2)
    let undo_body = serde_json::json!({"expectedRevision": 1});
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&undo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 2);

    // Redo (revision 2 -> 3)
    let redo_body = serde_json::json!({"expectedRevision": 2});
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/redo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&redo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 3);

    // Reset (revision 3 -> 4) - now only needs expectedRevision
    let reset_body = serde_json::json!({
        "expectedRevision": 3
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/reset"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&reset_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 4);
    assert_eq!(json["canUndo"], false);
    assert_eq!(json["canRedo"], false);
}

#[tokio::test]
async fn missing_auth_returns_401() {
    let app = test_app();
    let activity_id = Uuid::new_v4();

    let body = serde_json::json!({
        "geometry": sample_geometry()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{activity_id}/route-drafts"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn draft_not_found_returns_404() {
    let app = test_app();
    let (auth_key, auth_val) = auth_header();
    let random_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{random_id}"))
                .header(&auth_key, &auth_val)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn wrong_owner_returns_404() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create draft as user1
    let created = create_draft_for_user(&app, user1, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Try to access as user2
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user2}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn idempotency_key_required_for_apply() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    // Missing Idempotency-Key header
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn idempotency_key_required_for_undo() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({"expectedRevision": 0});

    // Missing Idempotency-Key header
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_draft_returns_204() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn create_draft_missing_idempotency_key_returns_400() {
    let app = test_app();
    let (auth_key, auth_val) = auth_header();
    let activity_id = Uuid::new_v4();

    let body = serde_json::json!({
        "geometry": sample_geometry()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{activity_id}/route-drafts"))
                .header(&auth_key, &auth_val)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// --- Activity validation tests ---

#[tokio::test]
async fn create_draft_activity_not_found_returns_404() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Gateway has no activities registered (non-permissive mode)
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    let body = serde_json::json!({
        "geometry": sample_geometry()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{activity_id}/route-drafts"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "NOT_FOUND");
}

#[tokio::test]
async fn create_draft_deleted_activity_returns_422() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Deleted,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    let body = serde_json::json!({
        "geometry": sample_geometry()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "ACTIVITY_DELETED");
}

#[tokio::test]
async fn create_draft_cross_owner_activity_returns_404() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let other_user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());

    // Activity is owned by other_user_id, but user_id is making the request
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(other_user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    let body = serde_json::json!({
        "geometry": sample_geometry()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Non-disclosing: returns 404 instead of 403
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "NOT_FOUND");
}

// --- Base route version validation tests ---

#[tokio::test]
async fn create_draft_invalid_base_route_version_returns_422() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let invalid_version_id = Uuid::new_v4();

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions(vec![])),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": invalid_version_id
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INVALID_BASE_ROUTE_VERSION");
}

#[tokio::test]
async fn create_draft_valid_base_route_version_succeeds() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions(vec![(
            version_id,
            activity_id,
        )])),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["baseRouteVersionId"], version_id.to_string());
    assert_eq!(json["revision"], 0);
}

#[tokio::test]
async fn create_draft_idempotent_return_with_different_base_version_returns_409() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id_1 = Uuid::new_v4();
    let version_id_2 = Uuid::new_v4();

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions(vec![
            (version_id_1, activity_id),
            (version_id_2, activity_id),
        ])),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // First request: create draft with version_id_1
    let body1 = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id_1
    });

    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body1).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::CREATED);

    // Second request: same activity but different baseRouteVersionId
    let body2 = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id_2
    });

    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "IDEMPOTENCY_CONFLICT");
}

#[tokio::test]
async fn get_draft_returns_base_route_version_id() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions(vec![(
            version_id,
            activity_id,
        )])),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create draft with base version
    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });

    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // GET the draft and verify baseRouteVersionId is returned
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["baseRouteVersionId"], version_id.to_string());
    assert_eq!(json["revision"], 0);
    // Verify geometry is preserved
    assert_eq!(json["geometry"], sample_geometry());
}

#[tokio::test]
async fn apply_with_same_key_different_payload_returns_409() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let op_id = Uuid::new_v4();

    // First request: apply a movePoint operation
    let body1 = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", op_id.to_string())
                .body(Body::from(serde_json::to_vec(&body1).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);

    // Second request: same idempotency key but DIFFERENT operation payload
    let body2 = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 1,
            "newPosition": {"latitude": 49.0, "longitude": 13.0}
        },
        "expectedRevision": 0
    });

    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", op_id.to_string())
                .body(Body::from(serde_json::to_vec(&body2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "IDEMPOTENCY_PAYLOAD_MISMATCH");
    assert_eq!(json["type"], "/problems/idempotency-conflict");
    assert_eq!(json["status"], 409);
}

#[tokio::test]
async fn error_response_has_problem_details_shape() {
    let app = test_app();
    let (auth_key, auth_val) = auth_header();
    let random_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{random_id}"))
                .header(&auth_key, &auth_val)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();

    // Verify Problem Details envelope fields are present
    assert!(json["type"].is_string(), "type field must be present");
    assert!(json["title"].is_string(), "title field must be present");
    assert!(json["status"].is_number(), "status field must be present");
    assert!(json["code"].is_string(), "code field must be present");
    assert!(json["detail"].is_string(), "detail field must be present");
    // requestId is null when middleware is not present
    assert!(
        json.get("requestId").is_some(),
        "requestId field must be present"
    );

    assert_eq!(json["code"], "NOT_FOUND");
    assert_eq!(json["status"], 404);
}

#[tokio::test]
async fn idempotent_replay_returns_snapshot_revision_not_current() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Apply operation A (revision 0 -> 1)
    let op_a_id = Uuid::new_v4();
    let body_a = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", op_a_id.to_string())
                .body(Body::from(serde_json::to_vec(&body_a).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response_a.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response_a.into_body(), usize::MAX)
        .await
        .unwrap();
    let json_a: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json_a["revision"], 1);

    // Apply operation B (revision 1 -> 2)
    let body_b = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 1,
            "newPosition": {"latitude": 48.5, "longitude": 12.5}
        },
        "expectedRevision": 1
    });

    let response_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body_b).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response_b.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response_b.into_body(), usize::MAX)
        .await
        .unwrap();
    let json_b: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json_b["revision"], 2);

    // Replay operation A (should return revision 1, not 2)
    let replay_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", op_a_id.to_string())
                .body(Body::from(serde_json::to_vec(&body_a).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(replay_response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(replay_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let replay_json: serde_json::Value = serde_json::from_slice(&b).unwrap();

    // The replay must return the snapshot revision (1), not the current draft revision (2)
    assert_eq!(replay_json["revision"], 1);
    assert_eq!(replay_json["canUndo"], true);
    assert_eq!(replay_json["canRedo"], false);
    assert_eq!(replay_json["draftId"], draft_id);
}

// --- DeleteSection operation validation tests ---

#[tokio::test]
async fn delete_section_returns_200_with_incremented_revision() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // sample_geometry() has 3 points (indices 0,1,2). Deleting index 1 to 1 leaves 2 points.
    let body = serde_json::json!({
        "operation": {
            "type": "deleteSection",
            "segmentIndex": 0,
            "startIndex": 1,
            "endIndex": 1
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 1);
    assert_eq!(json["draftId"], draft_id);
}

#[tokio::test]
async fn delete_section_reversed_range_returns_422_invalid_operation() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // startIndex > endIndex is a reversed range
    let body = serde_json::json!({
        "operation": {
            "type": "deleteSection",
            "segmentIndex": 0,
            "startIndex": 2,
            "endIndex": 1
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INVALID_OPERATION");
}

#[tokio::test]
async fn delete_section_out_of_bounds_returns_422_invalid_point_index() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // sample_geometry() has 3 points (indices 0,1,2). endIndex=5 is out of bounds.
    let body = serde_json::json!({
        "operation": {
            "type": "deleteSection",
            "segmentIndex": 0,
            "startIndex": 0,
            "endIndex": 5
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INVALID_POINT_INDEX");
}

#[tokio::test]
async fn delete_section_topology_breaking_returns_422_insufficient_points() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // sample_geometry() has 3 points. Deleting indices 0..2 (all 3) would leave 0 points.
    let body = serde_json::json!({
        "operation": {
            "type": "deleteSection",
            "segmentIndex": 0,
            "startIndex": 0,
            "endIndex": 2
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INSUFFICIENT_POINTS");
}

#[tokio::test]
async fn stale_revision_returns_409_with_problem_details() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Apply first operation to bump revision to 1
    let body1 = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body1).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Try with stale revision 0
    let body2 = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 1,
            "newPosition": {"latitude": 48.5, "longitude": 12.5}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["type"], "/problems/stale-route-draft");
    assert_eq!(json["title"], "Route draft revision is stale");
    assert_eq!(json["code"], "ROUTE_DRAFT_REVISION_CONFLICT");
    assert_eq!(json["status"], 409);
}

#[tokio::test]
async fn apply_move_point_invalid_coordinates_returns_422() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 91.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// --- AddPoint and DeletePoint API tests ---

#[tokio::test]
async fn apply_add_point_returns_200_with_new_revision() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "operation": {
            "type": "addPoint",
            "segmentIndex": 0,
            "afterPointIndex": 0,
            "point": {"latitude": 47.05, "longitude": 11.05}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 1);
    assert_eq!(json["draftId"], draft_id);
}

#[tokio::test]
async fn apply_add_point_with_elevation_returns_200() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "operation": {
            "type": "addPoint",
            "segmentIndex": 0,
            "afterPointIndex": 1,
            "point": {"latitude": 47.15, "longitude": 11.15, "elevation": 850.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 1);
    assert_eq!(json["draftId"], draft_id);
}

#[tokio::test]
async fn apply_delete_point_returns_200_with_new_revision() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // sample_geometry() has 3 points, deleting index 1 is valid
    let body = serde_json::json!({
        "operation": {
            "type": "deletePoint",
            "segmentIndex": 0,
            "pointIndex": 1
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 1);
    assert_eq!(json["draftId"], draft_id);
}

#[tokio::test]
async fn apply_delete_point_minimum_violation_returns_422() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create a draft with only 2 points in the segment
    let body_create = serde_json::json!({
        "geometry": [[
            {"latitude": 47.0, "longitude": 11.0},
            {"latitude": 47.1, "longitude": 11.1}
        ]]
    });

    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{activity_id}/route-drafts"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body_create).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // Attempt to delete a point from a 2-point segment
    let body = serde_json::json!({
        "operation": {
            "type": "deletePoint",
            "segmentIndex": 0,
            "pointIndex": 0
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INSUFFICIENT_POINTS");
}

#[tokio::test]
async fn apply_add_point_invalid_segment_returns_422() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // segmentIndex 99 is out of bounds
    let body = serde_json::json!({
        "operation": {
            "type": "addPoint",
            "segmentIndex": 99,
            "afterPointIndex": 0,
            "point": {"latitude": 47.05, "longitude": 11.05}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INVALID_SEGMENT_INDEX");
}

#[tokio::test]
async fn apply_delete_point_invalid_index_returns_422() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // pointIndex 99 is out of bounds (sample_geometry has 3 points)
    let body = serde_json::json!({
        "operation": {
            "type": "deletePoint",
            "segmentIndex": 0,
            "pointIndex": 99
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INVALID_POINT_INDEX");
}

#[tokio::test]
async fn apply_move_point_another_owner_returns_404() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create draft as user1
    let created = create_draft_for_user(&app, user1, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Try to apply operation as user2
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user2}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// --- Reset endpoint tests (server-side base geometry fetch) ---

#[tokio::test]
async fn reset_fetches_base_geometry_from_gateway() {
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    // The exact base geometry the gateway will return
    let base_geometry = vec![vec![
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.0, 11.0).unwrap(),
            Some(haiker_app::route_editing::Elevation::new(500.0)),
        ),
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.1, 11.1).unwrap(),
            Some(haiker_app::route_editing::Elevation::new(600.0)),
        ),
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.2, 11.2).unwrap(),
            Some(haiker_app::route_editing::Elevation::new(700.0)),
        ),
    ]];

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions_and_geometry(
            vec![(version_id, activity_id, base_geometry.clone())],
        )),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create draft with baseRouteVersionId and some different initial geometry
    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // Apply an operation to modify geometry (revision 0 -> 1)
    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Reset (revision 1 -> 2) - only expectedRevision in body
    let reset_body = serde_json::json!({
        "expectedRevision": 1
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/reset"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&reset_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["revision"], 2);
    assert_eq!(json["canUndo"], false);
    assert_eq!(json["canRedo"], false);

    // Verify the draft geometry matches the base version byte-for-byte
    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let b = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let draft_json: serde_json::Value = serde_json::from_slice(&b).unwrap();

    // Verify geometry matches base exactly
    let expected_geometry = serde_json::json!([[
        {"latitude": 47.0, "longitude": 11.0, "elevation": 500.0},
        {"latitude": 47.1, "longitude": 11.1, "elevation": 600.0},
        {"latitude": 47.2, "longitude": 11.2, "elevation": 700.0}
    ]]);
    assert_eq!(draft_json["geometry"], expected_geometry);
}

#[tokio::test]
async fn reset_without_base_version_returns_422() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create draft WITHOUT baseRouteVersionId
    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Try to reset - should fail because there is no base version
    let reset_body = serde_json::json!({
        "expectedRevision": 0
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/reset"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&reset_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "NO_BASE_ROUTE_VERSION");
}

#[tokio::test]
async fn reset_with_stale_revision_returns_409() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    let base_geometry = vec![vec![
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.0, 11.0).unwrap(),
            None,
        ),
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.1, 11.1).unwrap(),
            None,
        ),
    ]];

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions_and_geometry(
            vec![(version_id, activity_id, base_geometry)],
        )),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create draft with baseRouteVersionId
    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // Apply an operation (revision 0 -> 1)
    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Reset with stale revision 0 (actual is 1)
    let reset_body = serde_json::json!({
        "expectedRevision": 0
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/reset"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&reset_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn reset_cross_owner_returns_404() {
    ensure_dev_auth();
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    let base_geometry = vec![vec![
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.0, 11.0).unwrap(),
            None,
        ),
        RoutePoint::new(
            haiker_app::route_editing::Coordinate::new(47.1, 11.1).unwrap(),
            None,
        ),
    ]];

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user1),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions_and_geometry(
            vec![(version_id, activity_id, base_geometry)],
        )),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create draft as user1 with baseRouteVersionId
    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user1}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // user2 tries to reset user1's draft
    let reset_body = serde_json::json!({
        "expectedRevision": 0
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/reset"))
                .header("Authorization", format!("Bearer {user2}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&reset_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reset_gateway_rejects_geometry_returns_422() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    // Register the version for validation (so draft creation succeeds)
    // but WITHOUT geometry, so get_route_version_geometry returns Err.
    // This simulates a deleted base version or storage failure.
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions(vec![(
            version_id,
            activity_id,
        )])),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create draft with baseRouteVersionId (validation passes)
    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // Reset should fail because the gateway cannot produce geometry
    let reset_body = serde_json::json!({
        "expectedRevision": 0
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/reset"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&reset_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INVALID_BASE_ROUTE_VERSION");
    assert_eq!(json["type"], "/problems/invalid-base-route-version");
    assert_eq!(json["status"], 422);
}

// --- Immutability guard API tests: published/discarded drafts return 409 ---

/// Helper to publish a draft via the DELETE endpoint (discard) or by directly
/// manipulating the in-memory repo. Since there is no publish endpoint in the API,
/// we directly update the draft state in the repository.
async fn publish_draft_in_repo(repo: &InMemoryRouteDraftRepository, draft_id: &str) {
    let id = RouteDraftId::new(Uuid::parse_str(draft_id).unwrap());
    let mut draft = repo.find_by_id(id).await.unwrap().unwrap();
    draft.publish().unwrap();
    repo.update(&draft).await.unwrap();
}

#[tokio::test]
async fn apply_operation_to_published_draft_returns_409() {
    ensure_dev_auth();
    let repo = Arc::new(InMemoryRouteDraftRepository::new());
    let state = RouteEditingAppState {
        repo: repo.clone(),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Publish the draft directly in the repo
    publish_draft_in_repo(&repo, draft_id).await;

    // Attempt to apply an operation
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "DRAFT_NOT_ACTIVE");
}

#[tokio::test]
async fn apply_operation_to_discarded_draft_returns_409() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Discard the draft via the DELETE endpoint
    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    // Attempt to apply an operation to the discarded draft
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "DRAFT_NOT_ACTIVE");
}

#[tokio::test]
async fn undo_on_published_draft_returns_409() {
    ensure_dev_auth();
    let repo = Arc::new(InMemoryRouteDraftRepository::new());
    let state = RouteEditingAppState {
        repo: repo.clone(),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Apply an operation first so undo would be valid on an active draft
    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Publish the draft
    publish_draft_in_repo(&repo, draft_id).await;

    // Attempt undo
    let undo_body = serde_json::json!({"expectedRevision": 1});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&undo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "DRAFT_NOT_ACTIVE");
}

#[tokio::test]
async fn redo_on_discarded_draft_returns_409() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Apply an operation, then undo it so redo would be valid on an active draft
    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Undo
    let undo_body = serde_json::json!({"expectedRevision": 1});
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&undo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Discard the draft
    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    // Attempt redo
    let redo_body = serde_json::json!({"expectedRevision": 2});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/redo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&redo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "DRAFT_NOT_ACTIVE");
}

// --- OpenAPI contract tests: response shape validation ---

/// Validates that a JSON value conforms to the RouteDraftResponse schema.
fn assert_route_draft_response_schema(json: &serde_json::Value) {
    assert!(json["id"].is_string(), "id must be a string (uuid)");
    assert!(
        Uuid::parse_str(json["id"].as_str().unwrap()).is_ok(),
        "id must be a valid UUID"
    );
    assert!(
        json["activityId"].is_string(),
        "activityId must be a string (uuid)"
    );
    assert!(
        Uuid::parse_str(json["activityId"].as_str().unwrap()).is_ok(),
        "activityId must be a valid UUID"
    );
    assert!(
        json["revision"].is_i64() || json["revision"].is_u64(),
        "revision must be an integer"
    );
    assert!(json["state"].is_string(), "state must be a string");
    let state = json["state"].as_str().unwrap();
    assert!(
        ["active", "published", "discarded"].contains(&state),
        "state must be one of: active, published, discarded"
    );
    assert!(json["geometry"].is_array(), "geometry must be an array");
    let segments = json["geometry"].as_array().unwrap();
    assert!(
        !segments.is_empty(),
        "geometry must have at least 1 segment"
    );
    for segment in segments {
        assert!(segment.is_array(), "each segment must be an array");
        let points = segment.as_array().unwrap();
        assert!(
            points.len() >= 2,
            "each segment must have at least 2 points"
        );
        for point in points {
            assert_route_point_schema(point);
        }
    }
    assert!(json["canUndo"].is_boolean(), "canUndo must be a boolean");
    assert!(json["canRedo"].is_boolean(), "canRedo must be a boolean");
    assert!(
        json["createdAt"].is_string(),
        "createdAt must be a datetime string"
    );
    assert!(
        json["updatedAt"].is_string(),
        "updatedAt must be a datetime string"
    );
    if json.get("baseRouteVersionId").is_some() && !json["baseRouteVersionId"].is_null() {
        assert!(
            json["baseRouteVersionId"].is_string(),
            "baseRouteVersionId must be a string if present"
        );
    }
}

/// Validates that a JSON value conforms to the OperationResultResponse schema.
fn assert_operation_result_response_schema(json: &serde_json::Value) {
    assert!(
        json["draftId"].is_string(),
        "draftId must be a string (uuid)"
    );
    assert!(
        Uuid::parse_str(json["draftId"].as_str().unwrap()).is_ok(),
        "draftId must be a valid UUID"
    );
    assert!(
        json["revision"].is_i64() || json["revision"].is_u64(),
        "revision must be an integer"
    );
    assert!(json["canUndo"].is_boolean(), "canUndo must be a boolean");
    assert!(json["canRedo"].is_boolean(), "canRedo must be a boolean");
}

/// Validates that a JSON value conforms to the ProblemDetail schema.
fn assert_problem_detail_schema(json: &serde_json::Value) {
    assert!(
        json["type"].is_string(),
        "ProblemDetail: type must be a string"
    );
    assert!(
        json["title"].is_string(),
        "ProblemDetail: title must be a string"
    );
    assert!(
        json["status"].is_number(),
        "ProblemDetail: status must be a number"
    );
    assert!(
        json["code"].is_string(),
        "ProblemDetail: code must be a string"
    );
    assert!(
        json["detail"].is_string(),
        "ProblemDetail: detail must be a string"
    );
    assert!(
        json.get("requestId").is_some(),
        "ProblemDetail: requestId field must be present"
    );
}

/// Validates a single RoutePoint conforms to the OpenAPI schema.
fn assert_route_point_schema(json: &serde_json::Value) {
    assert!(
        json["latitude"].is_f64() || json["latitude"].is_i64(),
        "RoutePoint: latitude must be a number"
    );
    assert!(
        json["longitude"].is_f64() || json["longitude"].is_i64(),
        "RoutePoint: longitude must be a number"
    );
    let lat = json["latitude"].as_f64().unwrap();
    let lon = json["longitude"].as_f64().unwrap();
    assert!(
        (-90.0..=90.0).contains(&lat),
        "RoutePoint: latitude must be in [-90, 90]"
    );
    assert!(
        (-180.0..=180.0).contains(&lon),
        "RoutePoint: longitude must be in [-180, 180]"
    );
    if json.get("elevation").is_some() && !json["elevation"].is_null() {
        assert!(
            json["elevation"].is_f64() || json["elevation"].is_i64(),
            "RoutePoint: elevation must be a number if present"
        );
    }
}

#[tokio::test]
async fn contract_create_draft_response_matches_route_draft_response_schema() {
    let app = test_app();
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let json = create_draft_for_user(&app, user_id, activity_id).await;
    assert_route_draft_response_schema(&json);
    assert_eq!(json["revision"], 0);
    assert_eq!(json["state"], "active");
    assert_eq!(json["canUndo"], false);
    assert_eq!(json["canRedo"], false);
}

#[tokio::test]
async fn contract_get_draft_response_matches_route_draft_response_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_route_draft_response_schema(&json);
}

#[tokio::test]
async fn contract_apply_operation_response_matches_operation_result_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_operation_result_response_schema(&json);
}

#[tokio::test]
async fn contract_undo_response_matches_operation_result_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let undo_body = serde_json::json!({"expectedRevision": 1});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&undo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_operation_result_response_schema(&json);
}

#[tokio::test]
async fn contract_redo_response_matches_operation_result_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let undo_body = serde_json::json!({"expectedRevision": 1});
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&undo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let redo_body = serde_json::json!({"expectedRevision": 2});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/redo"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&redo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_operation_result_response_schema(&json);
}

#[tokio::test]
async fn contract_all_error_codes_have_correct_problem_detail_structure() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Test INVALID_SEGMENT_INDEX (422)
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 99,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["code"], "INVALID_SEGMENT_INDEX");
    assert_eq!(json["status"], 422);

    // Test INVALID_POINT_INDEX (422)
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 99,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["code"], "INVALID_POINT_INDEX");
    assert_eq!(json["status"], 422);
}

#[tokio::test]
async fn contract_insufficient_points_error_matches_problem_detail_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // sample_geometry has 3 points; deleting 0..2 removes all
    let body = serde_json::json!({
        "operation": {
            "type": "deleteSection",
            "segmentIndex": 0,
            "startIndex": 0,
            "endIndex": 2
        },
        "expectedRevision": 0
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["code"], "INSUFFICIENT_POINTS");
    assert_eq!(json["status"], 422);
}

#[tokio::test]
async fn contract_invalid_operation_error_matches_problem_detail_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Reversed range: startIndex > endIndex
    let body = serde_json::json!({
        "operation": {
            "type": "deleteSection",
            "segmentIndex": 0,
            "startIndex": 2,
            "endIndex": 0
        },
        "expectedRevision": 0
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["code"], "INVALID_OPERATION");
    assert_eq!(json["status"], 422);
}

#[tokio::test]
async fn contract_revision_conflict_error_matches_problem_detail_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Bump revision
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Use stale revision
    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.5, "longitude": 12.5}
        },
        "expectedRevision": 0
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["code"], "ROUTE_DRAFT_REVISION_CONFLICT");
    assert_eq!(json["status"], 409);
}

#[tokio::test]
async fn contract_not_found_error_matches_problem_detail_schema() {
    let app = test_app();
    let (auth_key, auth_val) = auth_header();
    let random_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{random_id}"))
                .header(&auth_key, &auth_val)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["code"], "NOT_FOUND");
    assert_eq!(json["status"], 404);
}

#[tokio::test]
async fn contract_cross_owner_error_matches_problem_detail_schema() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user1, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user2}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["status"], 404);
}

#[tokio::test]
async fn contract_draft_not_active_error_matches_problem_detail_schema() {
    ensure_dev_auth();
    let repo = Arc::new(InMemoryRouteDraftRepository::new());
    let state = RouteEditingAppState {
        repo: repo.clone(),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    publish_draft_in_repo(&repo, draft_id).await;

    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_problem_detail_schema(&json);
    assert_eq!(json["code"], "DRAFT_NOT_ACTIVE");
    assert_eq!(json["status"], 409);
}

// --- Validation endpoint tests ---

#[tokio::test]
async fn validate_valid_draft_returns_200_with_valid_true() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions(vec![(
            version_id,
            activity_id,
        )])),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create draft with base version
    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // Validate
    let validate_body = serde_json::json!({"expectedRevision": 0});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/validation"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["valid"], true);
    assert_eq!(json["errors"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn validate_draft_not_found_returns_404() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let random_id = Uuid::new_v4();

    let validate_body = serde_json::json!({"expectedRevision": 0});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_id}/validation"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn validate_wrong_owner_returns_404() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user1, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let validate_body = serde_json::json!({"expectedRevision": 0});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/validation"))
                .header("Authorization", format!("Bearer {user2}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn validate_published_draft_returns_409() {
    ensure_dev_auth();
    let repo = Arc::new(InMemoryRouteDraftRepository::new());
    let state = RouteEditingAppState {
        repo: repo.clone(),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    publish_draft_in_repo(&repo, draft_id).await;

    let validate_body = serde_json::json!({"expectedRevision": 0});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/validation"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "DRAFT_NOT_ACTIVE");
}

#[tokio::test]
async fn validate_revision_mismatch_returns_409() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Use wrong revision
    let validate_body = serde_json::json!({"expectedRevision": 99});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/validation"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "ROUTE_DRAFT_REVISION_CONFLICT");
}

#[tokio::test]
async fn validate_no_base_version_returns_200_with_valid_false() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create draft WITHOUT base version
    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let validate_body = serde_json::json!({"expectedRevision": 0});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/validation"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["valid"], false);
    let errors = json["errors"].as_array().unwrap();
    assert!(errors.iter().any(|e| e["code"] == "NO_BASE_VERSION"));
}

#[tokio::test]
async fn validate_does_not_modify_draft_state() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());
    let version_id = Uuid::new_v4();

    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::with_versions(vec![(
            version_id,
            activity_id,
        )])),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    let body = serde_json::json!({
        "geometry": sample_geometry(),
        "baseRouteVersionId": version_id
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/activities/{}/route-drafts", activity_id.0))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let draft_id = created["id"].as_str().unwrap();

    // Validate
    let validate_body = serde_json::json!({"expectedRevision": 0});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/validation"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify draft state is unchanged
    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let draft_json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(draft_json["state"], "active");
    assert_eq!(draft_json["revision"], 0);
}

#[tokio::test]
async fn validate_multiple_geometry_errors_returned_together_in_200() {
    ensure_dev_auth();
    let user_id = Uuid::new_v4();
    let activity_id = ActivityId::new(Uuid::new_v4());

    let repo = Arc::new(InMemoryRouteDraftRepository::new());
    let state = RouteEditingAppState {
        repo: repo.clone(),
        activity_gateway: Arc::new(InMemoryActivityGateway::with_activities(vec![(
            activity_id,
            UserId::new(user_id),
            LifecycleState::Active,
        )])),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);

    // Create a valid draft first (needs >= 2 points per segment to pass creation)
    let created = create_draft_for_user(&app, user_id, activity_id.0).await;
    let draft_id = created["id"].as_str().unwrap();

    // Directly manipulate the draft in the repo to introduce multiple geometry errors:
    // - Set a segment with only 1 point (insufficient)
    // - Remove the base_route_version_id
    let id = RouteDraftId::new(Uuid::parse_str(draft_id).unwrap());
    let mut draft = repo.find_by_id(id).await.unwrap().unwrap();
    draft.geometry = vec![vec![RoutePoint::new(
        haiker_app::route_editing::Coordinate::new(47.0, 11.0).unwrap(),
        None,
    )]];
    draft.base_route_version_id = None;
    repo.update(&draft).await.unwrap();

    // Validate - should return 200 with multiple errors
    let validate_body = serde_json::json!({"expectedRevision": 0});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/validation"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&validate_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["valid"], false);

    let errors = json["errors"].as_array().unwrap();
    // Must have at least 2 errors: insufficient points in segment AND no base version
    assert!(
        errors.len() >= 2,
        "expected at least 2 errors, got {}",
        errors.len()
    );

    // Verify INSUFFICIENT_POINTS_IN_SEGMENT is present
    assert!(
        errors
            .iter()
            .any(|e| e["code"] == "INSUFFICIENT_POINTS_IN_SEGMENT"),
        "expected INSUFFICIENT_POINTS_IN_SEGMENT error"
    );

    // Verify NO_BASE_VERSION is present
    assert!(
        errors.iter().any(|e| e["code"] == "NO_BASE_VERSION"),
        "expected NO_BASE_VERSION error"
    );

    // Verify each error has the correct structure
    for error in errors {
        assert!(error["code"].is_string(), "error code must be a string");
        assert!(error["detail"].is_string(), "error detail must be a string");
    }
}

// --- Publication endpoint tests ---

use haiker_app::route_versioning::commit::{
    CommitPublication, PublicationCommitData, PublicationResult,
};
use haiker_app::route_versioning::{RouteVersionId, RouteVersioningError};

/// A fake CommitPublication implementation for testing the API handler layer.
struct FakePublicationCommitter {
    /// Controls what the committer returns.
    behavior: Mutex<FakeCommitBehavior>,
}

enum FakeCommitBehavior {
    /// Return a successful result.
    Success { version_number: i32 },
    /// Return an error.
    Error(RouteVersioningError),
}

impl FakePublicationCommitter {
    fn succeeding(version_number: i32) -> Self {
        Self {
            behavior: Mutex::new(FakeCommitBehavior::Success { version_number }),
        }
    }

    fn failing(error: RouteVersioningError) -> Self {
        Self {
            behavior: Mutex::new(FakeCommitBehavior::Error(error)),
        }
    }
}

#[async_trait]
impl CommitPublication for FakePublicationCommitter {
    async fn commit(
        &self,
        data: &PublicationCommitData,
    ) -> Result<PublicationResult, RouteVersioningError> {
        let behavior = self.behavior.lock().unwrap();
        match &*behavior {
            FakeCommitBehavior::Success { version_number } => Ok(PublicationResult {
                route_version_id: RouteVersionId::generate(),
                version_number: *version_number,
                draft_id: data.draft_id,
                corrected_statistics_json: serde_json::json!({
                    "distance_meters": 12345.6,
                    "point_count": 3,
                    "calculation_version": "v1.0"
                }),
            }),
            FakeCommitBehavior::Error(err) => {
                // Clone the error for returning
                Err(match err {
                    RouteVersioningError::DraftNotFound => RouteVersioningError::DraftNotFound,
                    RouteVersioningError::NotAuthorized => RouteVersioningError::NotAuthorized,
                    RouteVersioningError::RevisionConflict { expected, actual } => {
                        RouteVersioningError::RevisionConflict {
                            expected: *expected,
                            actual: *actual,
                        }
                    }
                    RouteVersioningError::DraftNotActive => RouteVersioningError::DraftNotActive,
                    RouteVersioningError::IdempotencyConflict => {
                        RouteVersioningError::IdempotencyConflict
                    }
                    RouteVersioningError::ActivityNotFound => {
                        RouteVersioningError::ActivityNotFound
                    }
                    RouteVersioningError::ValidationFailed { errors } => {
                        RouteVersioningError::ValidationFailed {
                            errors: errors.clone(),
                        }
                    }
                    other => RouteVersioningError::PersistenceError {
                        message: other.to_string(),
                    },
                })
            }
        }
    }
}

fn publication_test_app(
    committer: FakePublicationCommitter,
) -> (Router, Arc<InMemoryRouteDraftRepository>) {
    let repo = Arc::new(InMemoryRouteDraftRepository::new());
    let state = RouteEditingAppState {
        repo: repo.clone(),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: Some(Arc::new(committer)),
        session_store: dummy_session_store(),
    };
    (test_app_with_state(state), repo)
}

#[tokio::test]
async fn publish_draft_missing_idempotency_key_returns_400() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::succeeding(2));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn publish_draft_non_uuid_idempotency_key_returns_400() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::succeeding(2));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    // Send a non-UUID idempotency key
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", "not-a-valid-uuid-at-all")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "INVALID_IDEMPOTENCY_KEY");
}

#[tokio::test]
async fn publish_draft_success_returns_201() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::succeeding(2));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0,
        "editSummary": "Fixed trail section"
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert!(json["routeVersionId"].is_string());
    assert_eq!(json["versionNumber"], 2);
    assert_eq!(json["draftId"], draft_id);
    // Verify correctedStatistics is present in the response
    assert!(json["correctedStatistics"].is_object());
    assert_eq!(json["correctedStatistics"]["distanceMeters"], 12345.6);
    assert_eq!(json["correctedStatistics"]["pointCount"], 3);
    assert_eq!(json["correctedStatistics"]["calculationVersion"], "v1.0");
}

#[tokio::test]
async fn publish_draft_cross_owner_returns_404() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::failing(
        RouteVersioningError::NotAuthorized,
    ));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "NOT_FOUND");
}

#[tokio::test]
async fn publish_draft_revision_conflict_returns_409() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::failing(
        RouteVersioningError::RevisionConflict {
            expected: 0,
            actual: 3,
        },
    ));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "ROUTE_DRAFT_REVISION_CONFLICT");
}

#[tokio::test]
async fn publish_draft_not_active_returns_409() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::failing(
        RouteVersioningError::DraftNotActive,
    ));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "DRAFT_NOT_ACTIVE");
}

#[tokio::test]
async fn publish_draft_not_found_returns_404() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::failing(
        RouteVersioningError::DraftNotFound,
    ));
    let user_id = Uuid::new_v4();
    let random_draft_id = Uuid::new_v4();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn publish_draft_idempotency_conflict_returns_409() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::failing(
        RouteVersioningError::IdempotencyConflict,
    ));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "IDEMPOTENCY_CONFLICT");
}

#[tokio::test]
async fn publish_draft_validation_failed_returns_422() {
    let (app, _repo) = publication_test_app(FakePublicationCommitter::failing(
        RouteVersioningError::ValidationFailed {
            errors: vec!["geometry has no segments".to_string()],
        },
    ));
    let user_id = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    let created = create_draft_for_user(&app, user_id, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    let body = serde_json::json!({
        "expectedRevision": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/publication"))
                .header("Authorization", format!("Bearer {user_id}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let b = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["code"], "VALIDATION_FAILED");
}

// =============================================================================
// Security tests: unauthenticated access (401 UNAUTHORIZED)
// =============================================================================

#[tokio::test]
async fn get_draft_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/route-drafts/{random_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn apply_operation_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_id}/operations"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn undo_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let body = serde_json::json!({"expectedRevision": 0});

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_id}/undo"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn redo_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let body = serde_json::json!({"expectedRevision": 0});

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_id}/redo"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn reset_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let body = serde_json::json!({"expectedRevision": 0});

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_id}/reset"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn delete_draft_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/route-drafts/{random_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn validate_draft_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let body = serde_json::json!({"expectedRevision": 0});

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_id}/validation"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn publish_draft_without_auth_returns_401() {
    let app = test_app();
    let random_id = Uuid::new_v4();

    let body = serde_json::json!({"expectedRevision": 0});

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{random_id}/publication"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// =============================================================================
// Security tests: cross-owner access (403 FORBIDDEN)
// =============================================================================

#[tokio::test]
async fn undo_cross_owner_returns_404() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create draft as user1
    let created = create_draft_for_user(&app, user1, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Apply operation so undo is possible
    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user1}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // user2 tries to undo user1's draft
    let undo_body = serde_json::json!({"expectedRevision": 1});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user2}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&undo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn redo_cross_owner_returns_404() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create draft as user1
    let created = create_draft_for_user(&app, user1, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // Apply operation, then undo so redo is possible
    let apply_body = serde_json::json!({
        "operation": {
            "type": "movePoint",
            "segmentIndex": 0,
            "pointIndex": 0,
            "newPosition": {"latitude": 48.0, "longitude": 12.0}
        },
        "expectedRevision": 0
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/operations"))
                .header("Authorization", format!("Bearer {user1}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&apply_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let undo_body = serde_json::json!({"expectedRevision": 1});
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/undo"))
                .header("Authorization", format!("Bearer {user1}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&undo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // user2 tries to redo user1's draft
    let redo_body = serde_json::json!({"expectedRevision": 2});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/route-drafts/{draft_id}/redo"))
                .header("Authorization", format!("Bearer {user2}"))
                .header("content-type", "application/json")
                .header("idempotency-key", Uuid::new_v4().to_string())
                .body(Body::from(serde_json::to_vec(&redo_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_draft_cross_owner_returns_404() {
    ensure_dev_auth();
    let state = RouteEditingAppState {
        repo: Arc::new(InMemoryRouteDraftRepository::new()),
        activity_gateway: Arc::new(InMemoryActivityGateway::permissive()),
        route_version_gateway: Arc::new(InMemoryRouteVersionGateway::permissive()),
        publication_committer: None,
        session_store: dummy_session_store(),
    };
    let app = test_app_with_state(state);
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let activity_id = Uuid::new_v4();

    // Create draft as user1
    let created = create_draft_for_user(&app, user1, activity_id).await;
    let draft_id = created["id"].as_str().unwrap();

    // user2 tries to delete user1's draft
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/route-drafts/{draft_id}"))
                .header("Authorization", format!("Bearer {user2}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
