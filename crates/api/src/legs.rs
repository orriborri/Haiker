//! Leg API handlers.
//!
//! Implements CRUD endpoints for activity legs:
//! - POST   /v1/activities/{activityId}/legs
//! - GET    /v1/activities/{activityId}/legs
//! - GET    /v1/activities/{activityId}/legs/{legId}
//! - PATCH  /v1/activities/{activityId}/legs/{legId}
//! - DELETE /v1/activities/{activityId}/legs/{legId}

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::activity_catalog::queries::get_activity;
use haiker_app::activity_catalog::repository::ActivityRepository;
use haiker_app::activity_catalog::ActivityId;
use haiker_app::recorded_activity::leg_commands::{
    add_leg, remove_leg, rename_leg, reorder_leg, update_leg_date,
};
use haiker_app::recorded_activity::leg_queries::{get_leg_detail, list_legs_for_activity};
use haiker_app::recorded_activity::leg_repository::LegRepository;
use haiker_app::recorded_activity::legs::LegId;
use haiker_app::recorded_activity::RecordedActivityError;

use crate::error::ApiError;
use crate::legs_dto::{
    CreateLegRequest, LegDetailResponse, LegListResponse, LegSummaryResponse,
    LegSummaryStatsResponse, UpdateLegRequest,
};
use haiker_infrastructure::auth_middleware::{AuthSession, HasSessionStore};
use haiker_infrastructure::session::SessionStore;

/// Shared application state for leg handlers.
#[derive(Clone)]
pub struct LegAppState {
    pub activity_repo: Arc<dyn ActivityRepository>,
    pub leg_repo: Arc<dyn LegRepository>,
    pub session_store: SessionStore,
}

impl HasSessionStore for LegAppState {
    fn session_store(&self) -> &SessionStore {
        &self.session_store
    }
}

/// Convert a RecordedActivityError to an ApiError.
fn leg_error_to_api_error(err: RecordedActivityError) -> ApiError {
    match err {
        RecordedActivityError::LegNotFound { .. } => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "leg not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        RecordedActivityError::InvalidLegTitle { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "VALIDATION_FAILED".to_string(),
            message,
            problem_type: Some("/problems/validation-failed".to_string()),
            title: Some("Validation Failed".to_string()),
            request_id: None,
            details: None,
        },
        RecordedActivityError::InvalidLegNumber { reason, .. } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "VALIDATION_FAILED".to_string(),
            message: reason,
            problem_type: Some("/problems/validation-failed".to_string()),
            title: Some("Validation Failed".to_string()),
            request_id: None,
            details: None,
        },
        RecordedActivityError::MaxLegsExceeded { max, .. } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "MAX_LEGS_EXCEEDED".to_string(),
            message: format!("maximum number of legs ({max}) exceeded"),
            problem_type: Some("/problems/max-legs-exceeded".to_string()),
            title: Some("Maximum Legs Exceeded".to_string()),
            request_id: None,
            details: None,
        },
        RecordedActivityError::DuplicateLegNumber { leg_number } => ApiError {
            status: StatusCode::CONFLICT,
            code: "DUPLICATE_LEG_NUMBER".to_string(),
            message: format!("duplicate leg number: {leg_number}"),
            problem_type: Some("/problems/duplicate-leg-number".to_string()),
            title: Some("Duplicate Leg Number".to_string()),
            request_id: None,
            details: None,
        },
        RecordedActivityError::Persistence { message } => {
            tracing::error!(error = %message, "persistence error during leg operation");
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL_ERROR".to_string(),
                message: "a persistence error occurred".to_string(),
                problem_type: Some("/problems/internal-error".to_string()),
                title: Some("Internal Server Error".to_string()),
                request_id: None,
                details: None,
            }
        }
        _ => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: "an unexpected error occurred".to_string(),
            problem_type: Some("/problems/internal-error".to_string()),
            title: Some("Internal Server Error".to_string()),
            request_id: None,
            details: None,
        },
    }
}

/// Verify the authenticated user owns the specified activity.
///
/// Returns the activity ID on success, or an ApiError (404) if the activity
/// does not exist or belongs to another user.
async fn verify_activity_ownership(
    activity_id: Uuid,
    actor: &AuthSession,
    activity_repo: &dyn ActivityRepository,
) -> Result<ActivityId, ApiError> {
    let id = ActivityId::new(activity_id);
    get_activity(id, actor.0.user_id, activity_repo)
        .await
        .map_err(|_| ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "activity not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        })?;
    Ok(id)
}

/// POST /v1/activities/{activityId}/legs
///
/// Add a new leg to an activity.
#[tracing::instrument(skip(state, actor))]
pub async fn post_add_leg(
    State(state): State<LegAppState>,
    actor: AuthSession,
    Path(activity_id): Path<Uuid>,
    Json(body): Json<CreateLegRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let activity_id =
        verify_activity_ownership(activity_id, &actor, state.activity_repo.as_ref()).await?;

    let leg = add_leg(
        activity_id,
        body.title.as_deref(),
        body.date,
        None, // source_revision_id — set during import flow
        None, // recorded_track_id — set during import flow
        state.leg_repo.as_ref(),
    )
    .await
    .map_err(leg_error_to_api_error)?;

    let response = LegDetailResponse {
        id: leg.id.0,
        leg_number: leg.leg_number,
        title: leg.title.map(|t| t.as_str().to_string()),
        date: leg.date,
        source_revision_id: leg.source_revision_id.map(|id| id.0),
        recorded_track_id: leg.recorded_track_id.map(|id| id.0),
        recorded_summary: None,
        created_at: leg.created_at,
        updated_at: leg.updated_at,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

/// GET /v1/activities/{activityId}/legs
///
/// List all legs for an activity, ordered by leg number.
#[tracing::instrument(skip(state, actor))]
pub async fn get_legs(
    State(state): State<LegAppState>,
    actor: AuthSession,
    Path(activity_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let activity_id =
        verify_activity_ownership(activity_id, &actor, state.activity_repo.as_ref()).await?;

    let legs = list_legs_for_activity(activity_id, state.leg_repo.as_ref())
        .await
        .map_err(leg_error_to_api_error)?;

    // Gather summaries for each leg
    let mut items = Vec::with_capacity(legs.len());
    for leg in legs {
        let summary = state
            .leg_repo
            .get_leg_summary(leg.id)
            .await
            .map_err(leg_error_to_api_error)?;

        items.push(LegSummaryResponse {
            id: leg.id.0,
            leg_number: leg.leg_number,
            title: leg.title.map(|t| t.as_str().to_string()),
            date: leg.date,
            recorded_summary: summary.map(|s| LegSummaryStatsResponse {
                distance_meters: s.distance_meters,
                elevation_gain_meters: s.elevation_gain_meters,
                elevation_loss_meters: s.elevation_loss_meters,
                point_count: s.point_count,
                duration_seconds: s.duration_seconds,
            }),
            created_at: leg.created_at,
            updated_at: leg.updated_at,
        });
    }

    let response = LegListResponse { items };
    Ok((StatusCode::OK, Json(response)))
}

/// GET /v1/activities/{activityId}/legs/{legId}
///
/// Get detailed information about a specific leg.
#[tracing::instrument(skip(state, actor))]
pub async fn get_leg_detail_handler(
    State(state): State<LegAppState>,
    actor: AuthSession,
    Path((activity_id, leg_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify ownership
    verify_activity_ownership(activity_id, &actor, state.activity_repo.as_ref()).await?;

    let detail = get_leg_detail(LegId::new(leg_id), state.leg_repo.as_ref())
        .await
        .map_err(leg_error_to_api_error)?;

    // Verify the leg belongs to the requested activity
    if detail.leg.activity_id.0 != activity_id {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "leg not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        });
    }

    let response = LegDetailResponse {
        id: detail.leg.id.0,
        leg_number: detail.leg.leg_number,
        title: detail.leg.title.map(|t| t.as_str().to_string()),
        date: detail.leg.date,
        source_revision_id: detail.leg.source_revision_id.map(|id| id.0),
        recorded_track_id: detail.leg.recorded_track_id.map(|id| id.0),
        recorded_summary: detail.summary.map(|s| LegSummaryStatsResponse {
            distance_meters: s.distance_meters,
            elevation_gain_meters: s.elevation_gain_meters,
            elevation_loss_meters: s.elevation_loss_meters,
            point_count: s.point_count,
            duration_seconds: s.duration_seconds,
        }),
        created_at: detail.leg.created_at,
        updated_at: detail.leg.updated_at,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// PATCH /v1/activities/{activityId}/legs/{legId}
///
/// Update a leg's title, date, or position.
#[tracing::instrument(skip(state, actor))]
pub async fn patch_leg(
    State(state): State<LegAppState>,
    actor: AuthSession,
    Path((activity_id, leg_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateLegRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify ownership
    verify_activity_ownership(activity_id, &actor, state.activity_repo.as_ref()).await?;

    let leg_id = LegId::new(leg_id);

    // Verify the leg belongs to the requested activity
    let existing = state
        .leg_repo
        .find_leg(leg_id)
        .await
        .map_err(leg_error_to_api_error)?
        .ok_or_else(|| ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "leg not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        })?;

    if existing.activity_id.0 != activity_id {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "leg not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        });
    }

    // Apply title update if present
    if let Some(title_option) = &body.title {
        rename_leg(leg_id, title_option.as_deref(), state.leg_repo.as_ref())
            .await
            .map_err(leg_error_to_api_error)?;
    }

    // Apply date update if present
    if let Some(new_date) = body.date {
        update_leg_date(leg_id, new_date, state.leg_repo.as_ref())
            .await
            .map_err(leg_error_to_api_error)?;
    }

    // Apply reorder if present
    if let Some(new_position) = body.leg_number {
        reorder_leg(leg_id, new_position, state.leg_repo.as_ref())
            .await
            .map_err(leg_error_to_api_error)?;
    }

    // Re-fetch the updated leg for the response
    let updated_detail = get_leg_detail(leg_id, state.leg_repo.as_ref())
        .await
        .map_err(leg_error_to_api_error)?;

    let response = LegDetailResponse {
        id: updated_detail.leg.id.0,
        leg_number: updated_detail.leg.leg_number,
        title: updated_detail.leg.title.map(|t| t.as_str().to_string()),
        date: updated_detail.leg.date,
        source_revision_id: updated_detail.leg.source_revision_id.map(|id| id.0),
        recorded_track_id: updated_detail.leg.recorded_track_id.map(|id| id.0),
        recorded_summary: updated_detail.summary.map(|s| LegSummaryStatsResponse {
            distance_meters: s.distance_meters,
            elevation_gain_meters: s.elevation_gain_meters,
            elevation_loss_meters: s.elevation_loss_meters,
            point_count: s.point_count,
            duration_seconds: s.duration_seconds,
        }),
        created_at: updated_detail.leg.created_at,
        updated_at: updated_detail.leg.updated_at,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// DELETE /v1/activities/{activityId}/legs/{legId}
///
/// Remove a leg from an activity and renumber remaining legs.
#[tracing::instrument(skip(state, actor))]
pub async fn delete_leg_handler(
    State(state): State<LegAppState>,
    actor: AuthSession,
    Path((activity_id, leg_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify ownership
    verify_activity_ownership(activity_id, &actor, state.activity_repo.as_ref()).await?;

    let leg_id = LegId::new(leg_id);

    // Verify the leg belongs to the requested activity
    let existing = state
        .leg_repo
        .find_leg(leg_id)
        .await
        .map_err(leg_error_to_api_error)?
        .ok_or_else(|| ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "leg not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        })?;

    if existing.activity_id.0 != activity_id {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "leg not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        });
    }

    remove_leg(leg_id, state.leg_repo.as_ref())
        .await
        .map_err(leg_error_to_api_error)?;

    Ok(StatusCode::NO_CONTENT)
}
