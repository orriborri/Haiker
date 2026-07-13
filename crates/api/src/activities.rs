//! Activity API handlers.
//!
//! Implements GET /v1/activities, GET /v1/activities/{activityId},
//! and PATCH /v1/activities/{activityId}/title.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::activity_catalog::commands::{self, AuditSink};
use haiker_app::activity_catalog::queries::{get_activity, list_activities};
use haiker_app::activity_catalog::repository::ActivityRepository;
use haiker_app::activity_catalog::{ActivityCatalogError, ActivityId};

use crate::activities_dto::{
    ActivityDetailResponse, ActivityListResponse, ActivitySummaryResponse, ListActivitiesParams,
    PaginationMeta, RenameActivityRequest,
};
use crate::auth::AuthenticatedActor;
use crate::error::ApiError;

/// Shared application state for activity handlers.
#[derive(Clone)]
pub struct ActivityAppState {
    pub repo: Arc<dyn ActivityRepository>,
    pub audit: Arc<dyn AuditSink>,
}

/// Convert an ActivityCatalogError to an ApiError.
fn activity_error_to_api_error(err: ActivityCatalogError) -> ApiError {
    match err {
        ActivityCatalogError::ActivityNotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "activity not found".to_string(),
            details: None,
        },
        // Non-disclosing: cross-owner access returns 404 identical to not-found.
        ActivityCatalogError::Unauthorized => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "activity not found".to_string(),
            details: None,
        },
        ActivityCatalogError::InvalidTitle { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "VALIDATION_FAILED".to_string(),
            message,
            details: None,
        },
        ActivityCatalogError::InvalidCursor { message } => ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "INVALID_CURSOR".to_string(),
            message,
            details: None,
        },
        ActivityCatalogError::PersistenceError { message } => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message,
            details: None,
        },
    }
}

/// GET /v1/activities
///
/// List activities owned by the authenticated user with cursor-based pagination.
#[tracing::instrument(skip(state, actor))]
pub async fn get_activities(
    State(state): State<ActivityAppState>,
    actor: AuthenticatedActor,
    Query(params): Query<ListActivitiesParams>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = actor.0.user_id;

    let page = list_activities(
        owner_id,
        params.cursor.as_deref(),
        params.page_size,
        state.repo.as_ref(),
    )
    .await
    .map_err(activity_error_to_api_error)?;

    let items: Vec<ActivitySummaryResponse> = page
        .items
        .iter()
        .map(|a| ActivitySummaryResponse {
            id: a.id.0,
            title: a.title.as_str().to_string(),
            activity_type: a.activity_type.to_string(),
            started_at: a.started_at,
            ended_at: a.ended_at,
            recorded_summary: a.recorded_summary.clone(),
            corrected_summary: a.corrected_summary.clone(),
            created_at: a.created_at,
            updated_at: a.updated_at,
        })
        .collect();

    let response = ActivityListResponse {
        pagination: PaginationMeta {
            cursor: page.next_cursor,
            has_more: page.has_more,
            page_size: items.len() as u32,
        },
        items,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// GET /v1/activities/{activityId}
///
/// Get a single activity detail by ID, scoped to the authenticated owner.
#[tracing::instrument(skip(state, actor))]
pub async fn get_activity_detail(
    State(state): State<ActivityAppState>,
    actor: AuthenticatedActor,
    Path(activity_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = actor.0.user_id;

    let activity = get_activity(ActivityId::new(activity_id), owner_id, state.repo.as_ref())
        .await
        .map_err(activity_error_to_api_error)?;

    let response = ActivityDetailResponse {
        id: activity.id.0,
        title: activity.title.as_str().to_string(),
        activity_type: activity.activity_type.to_string(),
        started_at: activity.started_at,
        ended_at: activity.ended_at,
        lifecycle_state: activity.lifecycle_state.to_string(),
        recorded_summary: activity.recorded_summary,
        corrected_summary: activity.corrected_summary,
        current_route_version_id: None,
        created_at: activity.created_at,
        updated_at: activity.updated_at,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// PATCH /v1/activities/{activityId}/title
///
/// Rename an activity. Validates the title, enforces ownership, and records
/// an audit event on success.
#[tracing::instrument(skip(state, actor, body))]
pub async fn patch_activity_title(
    State(state): State<ActivityAppState>,
    actor: AuthenticatedActor,
    Path(activity_id): Path<Uuid>,
    Json(body): Json<RenameActivityRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = actor.0.user_id;

    let activity = commands::rename_activity(
        ActivityId::new(activity_id),
        owner_id,
        &body.title,
        state.repo.as_ref(),
        state.audit.as_ref(),
    )
    .await
    .map_err(activity_error_to_api_error)?;

    let response = ActivityDetailResponse {
        id: activity.id.0,
        title: activity.title.as_str().to_string(),
        activity_type: activity.activity_type.to_string(),
        started_at: activity.started_at,
        ended_at: activity.ended_at,
        lifecycle_state: activity.lifecycle_state.to_string(),
        recorded_summary: activity.recorded_summary,
        corrected_summary: activity.corrected_summary,
        current_route_version_id: None,
        created_at: activity.created_at,
        updated_at: activity.updated_at,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// DELETE /v1/activities/{activityId}
///
/// Soft-delete an activity. Enforces ownership. Returns 204 No Content on success.
/// Idempotent: repeated deletion returns 204 without error.
#[tracing::instrument(skip(state, actor))]
pub async fn delete_activity_handler(
    State(state): State<ActivityAppState>,
    actor: AuthenticatedActor,
    Path(activity_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = actor.0.user_id;

    commands::delete_activity(
        ActivityId::new(activity_id),
        owner_id,
        state.repo.as_ref(),
        state.audit.as_ref(),
    )
    .await
    .map_err(activity_error_to_api_error)?;

    Ok(StatusCode::NO_CONTENT)
}

// -- In-memory implementations for use in tests --

#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
use haiker_app::activity_catalog::commands::NoOpAuditSink;
#[cfg(test)]
use haiker_app::activity_catalog::queries::{decode_cursor, encode_cursor, CursorPayload};
#[cfg(test)]
use haiker_app::activity_catalog::repository::ActivityPage;
#[cfg(test)]
use haiker_app::activity_catalog::{Activity, LifecycleState};
#[cfg(test)]
use haiker_app::identity::UserId;

/// In-memory activity repository for testing (not used in production).
#[cfg(test)]
pub struct InMemoryActivityRepository {
    activities: Mutex<HashMap<ActivityId, Activity>>,
}

#[cfg(test)]
impl InMemoryActivityRepository {
    pub fn new() -> Self {
        Self {
            activities: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_activities(activities: Vec<Activity>) -> Self {
        let map: HashMap<ActivityId, Activity> =
            activities.into_iter().map(|a| (a.id, a.clone())).collect();
        Self {
            activities: Mutex::new(map),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl ActivityRepository for InMemoryActivityRepository {
    async fn list_activities(
        &self,
        owner_id: UserId,
        cursor: Option<&str>,
        page_size: u32,
    ) -> Result<ActivityPage, ActivityCatalogError> {
        let activities = self.activities.lock().unwrap();

        // Get all active activities for this owner, sorted by started_at DESC, id DESC
        let mut owner_activities: Vec<&Activity> = activities
            .values()
            .filter(|a| a.owner_id == owner_id && a.lifecycle_state == LifecycleState::Active)
            .collect();

        owner_activities.sort_by(|a, b| {
            let a_time = a.started_at;
            let b_time = b.started_at;
            // DESC order: b before a for started_at, then by id DESC
            match (b_time, a_time) {
                (Some(bt), Some(at)) => bt.cmp(&at).then_with(|| b.id.0.cmp(&a.id.0)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.id.0.cmp(&a.id.0),
            }
        });

        // Apply cursor filtering
        let filtered: Vec<&Activity> = if let Some(cursor_str) = cursor {
            let cursor_payload = decode_cursor(cursor_str)?;
            let cursor_id: uuid::Uuid =
                cursor_payload
                    .id
                    .parse()
                    .map_err(|_| ActivityCatalogError::InvalidCursor {
                        message: "invalid id in cursor".to_string(),
                    })?;
            let cursor_started_at: Option<chrono::DateTime<chrono::Utc>> = cursor_payload
                .started_at
                .as_deref()
                .map(|s| {
                    s.parse::<chrono::DateTime<chrono::Utc>>().map_err(|_| {
                        ActivityCatalogError::InvalidCursor {
                            message: "invalid started_at in cursor".to_string(),
                        }
                    })
                })
                .transpose()?;

            owner_activities
                .into_iter()
                .filter(|a| {
                    match (a.started_at, cursor_started_at) {
                        (Some(a_time), Some(c_time)) => (a_time, a.id.0) < (c_time, cursor_id),
                        (None, Some(_)) => true, // NULL started_at comes after any value in DESC
                        (Some(_), None) => false,
                        (None, None) => a.id.0 < cursor_id,
                    }
                })
                .collect()
        } else {
            owner_activities
        };

        let has_more = filtered.len() as u32 > page_size;
        let items: Vec<Activity> = filtered
            .into_iter()
            .take(page_size as usize)
            .cloned()
            .collect();

        let next_cursor = if has_more {
            items.last().map(|last| {
                encode_cursor(&CursorPayload {
                    started_at: last.started_at.map(|ts| ts.to_rfc3339()),
                    id: last.id.0.to_string(),
                })
            })
        } else {
            None
        };

        Ok(ActivityPage {
            items,
            next_cursor,
            has_more,
        })
    }

    async fn find_by_id(&self, id: ActivityId) -> Result<Option<Activity>, ActivityCatalogError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, patch};
    use axum::Router;
    use chrono::{Duration, Utc};
    use haiker_app::activity_catalog::{ActivityTitle, ActivityType};
    use tower::ServiceExt;
    use uuid::Uuid;

    fn test_app() -> Router {
        let state = ActivityAppState {
            repo: Arc::new(InMemoryActivityRepository::new()),
            audit: Arc::new(NoOpAuditSink),
        };

        Router::new()
            .route("/v1/activities", get(get_activities))
            .route(
                "/v1/activities/{activityId}",
                get(get_activity_detail).delete(delete_activity_handler),
            )
            .route(
                "/v1/activities/{activityId}/title",
                patch(patch_activity_title),
            )
            .with_state(state)
    }

    fn test_app_with_activities(activities: Vec<Activity>) -> Router {
        let state = ActivityAppState {
            repo: Arc::new(InMemoryActivityRepository::with_activities(activities)),
            audit: Arc::new(NoOpAuditSink),
        };

        Router::new()
            .route("/v1/activities", get(get_activities))
            .route(
                "/v1/activities/{activityId}",
                get(get_activity_detail).delete(delete_activity_handler),
            )
            .route(
                "/v1/activities/{activityId}/title",
                patch(patch_activity_title),
            )
            .with_state(state)
    }

    fn auth_header() -> (String, String) {
        let user_id = Uuid::new_v4();
        ("Authorization".to_string(), format!("Bearer {user_id}"))
    }

    fn auth_header_for(user_id: Uuid) -> (String, String) {
        ("Authorization".to_string(), format!("Bearer {user_id}"))
    }

    fn make_activity(
        owner_id: UserId,
        title: &str,
        started_at: Option<chrono::DateTime<Utc>>,
    ) -> Activity {
        let title = ActivityTitle::new(title).unwrap();
        let mut activity = Activity::new(owner_id, title, ActivityType::Hike, started_at, None);
        activity.started_at = started_at;
        activity
    }

    #[tokio::test]
    async fn list_activities_returns_200_empty() {
        let app = test_app();
        let (auth_key, auth_val) = auth_header();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/activities")
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

        assert_eq!(json["items"].as_array().unwrap().len(), 0);
        assert_eq!(json["pagination"]["hasMore"], false);
        assert_eq!(json["pagination"]["pageSize"], 0);
    }

    #[tokio::test]
    async fn list_activities_returns_owner_activities_only() {
        let user1 = UserId::new(Uuid::new_v4());
        let user2 = UserId::new(Uuid::new_v4());

        let now = Utc::now();
        let activities = vec![
            make_activity(user1, "User1 Hike", Some(now)),
            make_activity(user2, "User2 Hike", Some(now - Duration::hours(1))),
            make_activity(user1, "User1 Walk", Some(now - Duration::hours(2))),
        ];

        let app = test_app_with_activities(activities);
        let (auth_key, auth_val) = auth_header_for(user1.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/activities")
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

        let items = json["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        // Only user1's activities
        for item in items {
            let title = item["title"].as_str().unwrap();
            assert!(title.starts_with("User1"));
        }
    }

    #[tokio::test]
    async fn list_activities_pagination() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let mut activities = Vec::new();
        for i in 0..5 {
            activities.push(make_activity(
                user,
                &format!("Hike {i}"),
                Some(now - Duration::hours(i as i64)),
            ));
        }

        let state = ActivityAppState {
            repo: Arc::new(InMemoryActivityRepository::with_activities(activities)),
            audit: Arc::new(NoOpAuditSink),
        };

        let app = Router::new()
            .route("/v1/activities", get(get_activities))
            .with_state(state);

        let (auth_key, auth_val) = auth_header_for(user.0);

        // First page: pageSize=2
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/activities?pageSize=2")
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

        let items = json["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(json["pagination"]["hasMore"], true);
        assert!(json["pagination"]["cursor"].is_string());

        // Second page using cursor
        let cursor = json["pagination"]["cursor"].as_str().unwrap();
        let uri = format!("/v1/activities?pageSize=2&cursor={cursor}");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&uri)
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
        let json2: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let items2 = json2["items"].as_array().unwrap();
        assert_eq!(items2.len(), 2);
        assert_eq!(json2["pagination"]["hasMore"], true);

        // Third page - should have 1 item
        let cursor2 = json2["pagination"]["cursor"].as_str().unwrap();
        let uri2 = format!("/v1/activities?pageSize=2&cursor={cursor2}");

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&uri2)
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
        let json3: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let items3 = json3["items"].as_array().unwrap();
        assert_eq!(items3.len(), 1);
        assert_eq!(json3["pagination"]["hasMore"], false);
    }

    #[tokio::test]
    async fn list_activities_no_duplicates_across_pages() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let mut activities = Vec::new();
        for i in 0..7 {
            activities.push(make_activity(
                user,
                &format!("Hike {i}"),
                Some(now - Duration::hours(i as i64)),
            ));
        }

        let state = ActivityAppState {
            repo: Arc::new(InMemoryActivityRepository::with_activities(activities)),
            audit: Arc::new(NoOpAuditSink),
        };

        let app = Router::new()
            .route("/v1/activities", get(get_activities))
            .with_state(state);

        let (auth_key, auth_val) = auth_header_for(user.0);

        let mut all_ids: Vec<String> = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..10 {
            let uri = match &cursor {
                Some(c) => format!("/v1/activities?pageSize=3&cursor={c}"),
                None => "/v1/activities?pageSize=3".to_string(),
            };

            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(&uri)
                        .header(&auth_key, &auth_val)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

            let items = json["items"].as_array().unwrap();
            for item in items {
                all_ids.push(item["id"].as_str().unwrap().to_string());
            }

            if !json["pagination"]["hasMore"].as_bool().unwrap() {
                break;
            }
            cursor = json["pagination"]["cursor"].as_str().map(|s| s.to_string());
        }

        // Verify no duplicates
        let unique_count = {
            let mut sorted = all_ids.clone();
            sorted.sort();
            sorted.dedup();
            sorted.len()
        };
        assert_eq!(all_ids.len(), 7);
        assert_eq!(unique_count, 7);
    }

    #[tokio::test]
    async fn list_activities_without_auth_returns_401() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/activities")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_activities_response_includes_expected_fields() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let mut activity = make_activity(user, "Morning Hike", Some(now));
        activity.ended_at = Some(now + Duration::hours(2));
        activity.recorded_summary = Some(serde_json::json!({"distance_km": 5.2}));
        activity.corrected_summary = Some(serde_json::json!({"distance_km": 5.5}));

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/activities")
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

        let item = &json["items"][0];
        assert!(item["id"].is_string());
        assert_eq!(item["title"], "Morning Hike");
        assert_eq!(item["activityType"], "hike");
        assert!(item["startedAt"].is_string());
        assert!(item["endedAt"].is_string());
        assert_eq!(item["recordedSummary"]["distance_km"], 5.2);
        assert_eq!(item["correctedSummary"]["distance_km"], 5.5);
        assert!(item["createdAt"].is_string());
        assert!(item["updatedAt"].is_string());
    }

    #[tokio::test]
    async fn list_activities_deleted_not_returned() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let active = make_activity(user, "Active Hike", Some(now));
        let mut deleted = make_activity(user, "Deleted Hike", Some(now - Duration::hours(1)));
        deleted.lifecycle_state = LifecycleState::Deleted;

        let app = test_app_with_activities(vec![active, deleted]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/activities")
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

        let items = json["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Active Hike");
    }

    // --- Activity Detail (GET /v1/activities/{activityId}) tests ---

    #[tokio::test]
    async fn get_activity_detail_returns_200_for_owner() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let mut activity = make_activity(user, "Summit Hike", Some(now));
        activity.ended_at = Some(now + Duration::hours(3));
        activity.recorded_summary = Some(serde_json::json!({"distance_km": 8.0}));
        activity.corrected_summary = Some(serde_json::json!({"distance_km": 8.2}));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}"))
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

        assert_eq!(json["id"], activity_id.to_string());
        assert_eq!(json["title"], "Summit Hike");
        assert_eq!(json["activityType"], "hike");
        assert_eq!(json["lifecycleState"], "active");
        assert!(json["startedAt"].is_string());
        assert!(json["endedAt"].is_string());
        assert_eq!(json["recordedSummary"]["distance_km"], 8.0);
        assert_eq!(json["correctedSummary"]["distance_km"], 8.2);
        assert!(json["createdAt"].is_string());
        assert!(json["updatedAt"].is_string());
    }

    #[tokio::test]
    async fn get_activity_detail_not_found_returns_404() {
        let user = UserId::new(Uuid::new_v4());
        let random_id = Uuid::new_v4();

        let app = test_app();
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{random_id}"))
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

        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "activity not found");
    }

    #[tokio::test]
    async fn get_activity_detail_cross_owner_returns_404() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity = make_activity(owner, "Owner Hike", Some(now));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(other_user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Non-disclosing: same 404 as not found
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "activity not found");
    }

    #[tokio::test]
    async fn get_activity_detail_deleted_returns_404() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let mut activity = make_activity(user, "Deleted Hike", Some(now));
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity_id}"))
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

        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "activity not found");
    }

    #[tokio::test]
    async fn get_activity_detail_without_auth_returns_401() {
        let random_id = Uuid::new_v4();
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{random_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // --- Activity Rename (PATCH /v1/activities/{activityId}/title) tests ---

    #[tokio::test]
    async fn patch_activity_title_succeeds() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity = make_activity(user, "Old Title", Some(now));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/v1/activities/{activity_id}/title"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"New Title"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["id"], activity_id.to_string());
        assert_eq!(json["title"], "New Title");
        assert_eq!(json["lifecycleState"], "active");
    }

    #[tokio::test]
    async fn patch_activity_title_empty_returns_422() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity = make_activity(user, "Existing", Some(now));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/v1/activities/{activity_id}/title"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"]["code"], "VALIDATION_FAILED");
    }

    #[tokio::test]
    async fn patch_activity_title_unknown_fields_returns_422() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity = make_activity(user, "Existing", Some(now));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/v1/activities/{activity_id}/title"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Valid","unknownField":"value"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // deny_unknown_fields causes deserialization failure -> 422
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn patch_activity_title_cross_owner_returns_404() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity = make_activity(owner, "Owner's Activity", Some(now));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(other_user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/v1/activities/{activity_id}/title"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Hijacked"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "activity not found");
    }

    #[tokio::test]
    async fn patch_activity_title_deleted_returns_404() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let mut activity = make_activity(user, "Deleted Activity", Some(now));
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/v1/activities/{activity_id}/title"))
                    .header(&auth_key, &auth_val)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Rename Deleted"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn patch_activity_title_without_auth_returns_401() {
        let random_id = Uuid::new_v4();
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/v1/activities/{random_id}/title"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"New"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // --- Activity Delete (DELETE /v1/activities/{activityId}) tests ---

    #[tokio::test]
    async fn delete_activity_returns_204() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity = make_activity(user, "To Delete", Some(now));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/activities/{activity_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_activity_cross_owner_returns_404() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity = make_activity(owner, "Owner's Activity", Some(now));
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(other_user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/activities/{activity_id}"))
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

        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["message"], "activity not found");
    }

    #[tokio::test]
    async fn delete_activity_repeated_returns_204_idempotent() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let mut activity = make_activity(user, "Already Deleted", Some(now));
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id.0;

        let app = test_app_with_activities(vec![activity]);
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/activities/{activity_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_activity_then_not_in_list() {
        let user = UserId::new(Uuid::new_v4());
        let now = Utc::now();

        let activity1 = make_activity(user, "Keep This", Some(now));
        let activity2 = make_activity(user, "Delete This", Some(now - Duration::hours(1)));
        let activity2_id = activity2.id.0;

        let state = ActivityAppState {
            repo: Arc::new(InMemoryActivityRepository::with_activities(vec![
                activity1, activity2,
            ])),
            audit: Arc::new(NoOpAuditSink),
        };

        let app = Router::new()
            .route("/v1/activities", get(get_activities))
            .route(
                "/v1/activities/{activityId}",
                get(get_activity_detail).delete(delete_activity_handler),
            )
            .with_state(state);

        let (auth_key, auth_val) = auth_header_for(user.0);

        // Delete activity2
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/activities/{activity2_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // List should only contain activity1
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/activities")
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

        let items = json["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Keep This");

        // Detail of deleted activity should return 404
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/activities/{activity2_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_activity_without_auth_returns_401() {
        let random_id = Uuid::new_v4();
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/activities/{random_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_activity_not_found_returns_404() {
        let user = UserId::new(Uuid::new_v4());
        let random_id = Uuid::new_v4();

        let app = test_app();
        let (auth_key, auth_val) = auth_header_for(user.0);

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/activities/{random_id}"))
                    .header(&auth_key, &auth_val)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
