//! Recorded route API handler.
//!
//! Implements GET /v1/activities/{activityId}/recorded-route.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::activity_catalog::repository::ActivityRepository;
use haiker_app::recorded_activity::queries::{get_recorded_route, RecordedRouteResult};
use haiker_app::recorded_activity::{RecordedActivityError, RecordedRouteRepository};

use crate::error::ApiError;
use crate::recorded_route_dto::{
    GeoJsonFeature, GeoJsonGeometry, RecordedRouteParams, RecordedRoutePreviewResponse,
    RecordedRouteResponse, RouteDetail, RouteProperties, SegmentProperties,
};
use haiker_infrastructure::auth_middleware::{AuthSession, HasSessionStore};
use haiker_infrastructure::session::SessionStore;

/// Shared application state for recorded route handlers.
#[derive(Clone)]
pub struct RecordedRouteAppState {
    pub activity_repo: Arc<dyn ActivityRepository>,
    pub route_repo: Arc<dyn RecordedRouteRepository>,
    pub session_store: SessionStore,
}

impl HasSessionStore for RecordedRouteAppState {
    fn session_store(&self) -> &SessionStore {
        &self.session_store
    }
}

/// Convert a RecordedActivityError to an ApiError.
fn recorded_route_error_to_api_error(err: RecordedActivityError) -> ApiError {
    match err {
        RecordedActivityError::NotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "recorded route not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        RecordedActivityError::Persistence { message } => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message,
            problem_type: Some("/problems/internal-error".to_string()),
            title: Some("Internal Server Error".to_string()),
            request_id: None,
            details: None,
        },
        _ => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: "unexpected error".to_string(),
            problem_type: Some("/problems/internal-error".to_string()),
            title: Some("Internal Server Error".to_string()),
            request_id: None,
            details: None,
        },
    }
}

/// GET /v1/activities/{activityId}/recorded-route
///
/// Returns the recorded route geometry for an activity.
/// Responds with application/geo+json for full geometry, application/json for preview.
#[tracing::instrument(skip(state, actor))]
pub async fn get_recorded_route_handler(
    State(state): State<RecordedRouteAppState>,
    actor: AuthSession,
    Path(activity_id): Path<Uuid>,
    Query(params): Query<RecordedRouteParams>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = actor.0.user_id;
    let preview = params.detail == RouteDetail::Preview;

    let result = get_recorded_route(
        activity_id,
        owner_id,
        preview,
        state.activity_repo.as_ref(),
        state.route_repo.as_ref(),
    )
    .await
    .map_err(recorded_route_error_to_api_error)?;

    match result {
        RecordedRouteResult::Full(data) => {
            // Build GeoJSON FeatureCollection
            let bbox = [
                data.bounding_box.south_west.longitude, // west
                data.bounding_box.south_west.latitude,  // south
                data.bounding_box.north_east.longitude, // east
                data.bounding_box.north_east.latitude,  // north
            ];

            let features: Vec<GeoJsonFeature> = data
                .segments
                .iter()
                .enumerate()
                .map(|(i, segment)| GeoJsonFeature {
                    feature_type: "Feature".to_string(),
                    geometry: GeoJsonGeometry {
                        geometry_type: "LineString".to_string(),
                        coordinates: segment
                            .points
                            .iter()
                            .map(|c| [c.longitude, c.latitude]) // GeoJSON: [lng, lat]
                            .collect(),
                    },
                    properties: SegmentProperties {
                        segment_index: i,
                        point_count: segment.points.len(),
                    },
                })
                .collect();

            let response = RecordedRouteResponse {
                geojson_type: "FeatureCollection".to_string(),
                bbox,
                features,
                properties: RouteProperties {
                    distance_meters: data.statistics.distance_meters,
                    elevation_gain_meters: data.statistics.elevation_gain_meters,
                    elevation_loss_meters: data.statistics.elevation_loss_meters,
                    point_count: data.statistics.point_count,
                    segment_count: data.statistics.segment_count,
                },
            };

            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/geo+json".to_string())],
                Json(response),
            )
                .into_response())
        }
        RecordedRouteResult::Preview(preview_data) => {
            let bbox = [
                preview_data.bounding_box.south_west.longitude,
                preview_data.bounding_box.south_west.latitude,
                preview_data.bounding_box.north_east.longitude,
                preview_data.bounding_box.north_east.latitude,
            ];

            let response = RecordedRoutePreviewResponse {
                bbox,
                point_count: preview_data.statistics.point_count,
                segment_count: preview_data.statistics.segment_count,
                distance_meters: preview_data.statistics.distance_meters,
                elevation_gain_meters: preview_data.statistics.elevation_gain_meters,
                elevation_loss_meters: preview_data.statistics.elevation_loss_meters,
            };

            Ok((StatusCode::OK, Json(response)).into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    use haiker_app::activity_catalog::repository::{ActivityPage, ActivityRepository};
    use haiker_app::activity_catalog::{
        Activity, ActivityCatalogError, ActivityId, ActivityTitle, ActivityType,
    };
    use haiker_app::identity::UserId;
    use haiker_app::recorded_activity::repository::{
        RecordedRouteData, RecordedRoutePreview, RecordedRouteRepository, RouteSegment,
        RouteStatistics,
    };
    use haiker_app::recorded_activity::{BoundingBox, Coordinate, RecordedActivityError};

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

    // --- In-memory ActivityRepository for tests ---

    struct TestActivityRepository {
        activities: Mutex<HashMap<ActivityId, Activity>>,
    }

    impl TestActivityRepository {
        fn with_activities(activities: Vec<Activity>) -> Self {
            let map: HashMap<ActivityId, Activity> =
                activities.into_iter().map(|a| (a.id, a.clone())).collect();
            Self {
                activities: Mutex::new(map),
            }
        }
    }

    #[async_trait]
    impl ActivityRepository for TestActivityRepository {
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

    // --- In-memory RecordedRouteRepository for tests ---

    struct TestRecordedRouteRepository {
        routes: Mutex<HashMap<Uuid, RecordedRouteData>>,
    }

    impl TestRecordedRouteRepository {
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
    impl RecordedRouteRepository for TestRecordedRouteRepository {
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

    fn make_activity(owner_id: UserId, title: &str) -> Activity {
        let title = ActivityTitle::new(title).unwrap();
        Activity::new(owner_id, title, ActivityType::Hike, None, None)
    }

    fn make_route_data(activity_id: Uuid) -> RecordedRouteData {
        RecordedRouteData {
            activity_id,
            segments: vec![
                RouteSegment {
                    points: vec![
                        Coordinate {
                            latitude: 47.5,
                            longitude: 10.2,
                        },
                        Coordinate {
                            latitude: 47.6,
                            longitude: 10.3,
                        },
                        Coordinate {
                            latitude: 47.7,
                            longitude: 10.4,
                        },
                    ],
                },
                RouteSegment {
                    points: vec![
                        Coordinate {
                            latitude: 47.7,
                            longitude: 10.4,
                        },
                        Coordinate {
                            latitude: 47.8,
                            longitude: 10.5,
                        },
                    ],
                },
            ],
            bounding_box: BoundingBox::new(
                Coordinate {
                    latitude: 47.5,
                    longitude: 10.2,
                },
                Coordinate {
                    latitude: 47.8,
                    longitude: 10.5,
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

    fn test_app(activities: Vec<Activity>, route_repo: Arc<dyn RecordedRouteRepository>) -> Router {
        ensure_dev_auth();
        let state = RecordedRouteAppState {
            activity_repo: Arc::new(TestActivityRepository::with_activities(activities)),
            route_repo,
            session_store: dummy_session_store(),
        };

        Router::new()
            .route(
                "/v1/activities/{activityId}/recorded-route",
                get(get_recorded_route_handler),
            )
            .with_state(state)
    }

    fn auth_header_for(user_id: Uuid) -> (String, String) {
        ("Authorization".to_string(), format!("Bearer {user_id}"))
    }

    // --- Tests ---

    #[tokio::test]
    async fn full_geometry_returns_geojson_feature_collection() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner, "Trail Run");
        let activity_id = activity.id.0;
        let route_data = make_route_data(activity_id);

        let app = test_app(
            vec![activity],
            Arc::new(TestRecordedRouteRepository::with_route(
                activity_id,
                route_data,
            )),
        );
        let (auth_key, auth_val) = auth_header_for(owner.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}/recorded-route"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "application/geo+json"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["type"], "FeatureCollection");
        assert_eq!(json["features"].as_array().unwrap().len(), 2);
        assert_eq!(json["properties"]["pointCount"], 5);
        assert_eq!(json["properties"]["segmentCount"], 2);
        assert_eq!(json["properties"]["distanceMeters"], 5000.0);
        assert_eq!(json["properties"]["elevationGainMeters"], 200.0);
        assert_eq!(json["properties"]["elevationLossMeters"], 150.0);
    }

    #[tokio::test]
    async fn geojson_uses_longitude_latitude_ordering() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner, "Trail Run");
        let activity_id = activity.id.0;
        let route_data = make_route_data(activity_id);

        let app = test_app(
            vec![activity],
            Arc::new(TestRecordedRouteRepository::with_route(
                activity_id,
                route_data,
            )),
        );
        let (auth_key, auth_val) = auth_header_for(owner.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}/recorded-route"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // First point of first segment: lat=47.5, lng=10.2
        // GeoJSON ordering is [longitude, latitude]
        let first_coord = &json["features"][0]["geometry"]["coordinates"][0];
        assert_eq!(first_coord[0].as_f64().unwrap(), 10.2); // longitude first
        assert_eq!(first_coord[1].as_f64().unwrap(), 47.5); // latitude second
    }

    #[tokio::test]
    async fn full_geometry_preserves_segment_structure() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner, "Multi-segment Hike");
        let activity_id = activity.id.0;
        let route_data = make_route_data(activity_id);

        let app = test_app(
            vec![activity],
            Arc::new(TestRecordedRouteRepository::with_route(
                activity_id,
                route_data,
            )),
        );
        let (auth_key, auth_val) = auth_header_for(owner.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}/recorded-route"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let features = json["features"].as_array().unwrap();
        assert_eq!(features.len(), 2);

        // First segment has 3 points
        assert_eq!(features[0]["properties"]["segmentIndex"], 0);
        assert_eq!(features[0]["properties"]["pointCount"], 3);
        assert_eq!(
            features[0]["geometry"]["coordinates"]
                .as_array()
                .unwrap()
                .len(),
            3
        );

        // Second segment has 2 points
        assert_eq!(features[1]["properties"]["segmentIndex"], 1);
        assert_eq!(features[1]["properties"]["pointCount"], 2);
        assert_eq!(
            features[1]["geometry"]["coordinates"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn bounding_box_is_included_in_response() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner, "Bbox Test");
        let activity_id = activity.id.0;
        let route_data = make_route_data(activity_id);

        let app = test_app(
            vec![activity],
            Arc::new(TestRecordedRouteRepository::with_route(
                activity_id,
                route_data,
            )),
        );
        let (auth_key, auth_val) = auth_header_for(owner.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}/recorded-route"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // bbox: [west, south, east, north] = [sw.lng, sw.lat, ne.lng, ne.lat]
        let bbox = json["bbox"].as_array().unwrap();
        assert_eq!(bbox[0].as_f64().unwrap(), 10.2); // west (sw longitude)
        assert_eq!(bbox[1].as_f64().unwrap(), 47.5); // south (sw latitude)
        assert_eq!(bbox[2].as_f64().unwrap(), 10.5); // east (ne longitude)
        assert_eq!(bbox[3].as_f64().unwrap(), 47.8); // north (ne latitude)
    }

    #[tokio::test]
    async fn preview_returns_simplified_response() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner, "Preview Test");
        let activity_id = activity.id.0;
        let route_data = make_route_data(activity_id);

        let app = test_app(
            vec![activity],
            Arc::new(TestRecordedRouteRepository::with_route(
                activity_id,
                route_data,
            )),
        );
        let (auth_key, auth_val) = auth_header_for(owner.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/v1/activities/{activity_id}/recorded-route?detail=preview"
                    ))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Preview should have bbox and statistics but no features/geometry
        assert!(json.get("features").is_none());
        assert!(json.get("type").is_none());

        assert_eq!(json["pointCount"], 5);
        assert_eq!(json["segmentCount"], 2);
        assert_eq!(json["distanceMeters"], 5000.0);

        let bbox = json["bbox"].as_array().unwrap();
        assert_eq!(bbox[0].as_f64().unwrap(), 10.2);
        assert_eq!(bbox[1].as_f64().unwrap(), 47.5);
        assert_eq!(bbox[2].as_f64().unwrap(), 10.5);
        assert_eq!(bbox[3].as_f64().unwrap(), 47.8);
    }

    #[tokio::test]
    async fn not_found_returns_404() {
        let owner = UserId::new(Uuid::new_v4());
        let random_id = Uuid::new_v4();

        let app = test_app(vec![], Arc::new(TestRecordedRouteRepository::new()));
        let (auth_key, auth_val) = auth_header_for(owner.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{random_id}/recorded-route"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["code"], "NOT_FOUND");
        assert_eq!(json["detail"], "recorded route not found");
    }

    #[tokio::test]
    async fn cross_owner_returns_404() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner, "Owner Hike");
        let activity_id = activity.id.0;
        let route_data = make_route_data(activity_id);

        let app = test_app(
            vec![activity],
            Arc::new(TestRecordedRouteRepository::with_route(
                activity_id,
                route_data,
            )),
        );
        let (auth_key, auth_val) = auth_header_for(other_user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}/recorded-route"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Non-disclosing: cross-owner returns same 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["code"], "NOT_FOUND");
        // Should not reveal any geometry or storage details
        assert!(json.get("features").is_none());
        assert!(json.get("bbox").is_none());
    }

    #[tokio::test]
    async fn without_auth_returns_401() {
        let random_id = Uuid::new_v4();
        let app = test_app(vec![], Arc::new(TestRecordedRouteRepository::new()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{random_id}/recorded-route"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_detail_param_returns_400() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_activity(owner, "Detail Test");
        let activity_id = activity.id.0;
        let route_data = make_route_data(activity_id);

        let app = test_app(
            vec![activity],
            Arc::new(TestRecordedRouteRepository::with_route(
                activity_id,
                route_data,
            )),
        );
        let (auth_key, auth_val) = auth_header_for(owner.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/v1/activities/{activity_id}/recorded-route?detail=typo"
                    ))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
