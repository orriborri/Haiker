//! Route Editing API handlers.
//!
//! Implements POST /v1/activities/{activityId}/route-drafts,
//! GET /v1/route-drafts/{draftId}, POST /v1/route-drafts/{draftId}/operations,
//! POST /v1/route-drafts/{draftId}/undo, POST /v1/route-drafts/{draftId}/redo,
//! POST /v1/route-drafts/{draftId}/reset, DELETE /v1/route-drafts/{draftId}.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::route_editing::{
    RouteDraft, RouteDraftId, RouteDraftRepository, RouteEditingError,
};

use crate::auth::AuthenticatedActor;
use crate::error::ApiError;
use crate::route_editing_dto::{
    draft_to_response, geometry_to_domain, parse_idempotency_key, ApplyOperationRequest,
    CreateRouteDraftRequest, OperationResultResponse, ResetRequest, UndoRedoRequest,
};

/// Shared application state for route editing handlers.
#[derive(Clone)]
pub struct RouteEditingAppState {
    pub repo: Arc<dyn RouteDraftRepository>,
}

/// Convert a RouteEditingError to an ApiError.
fn route_editing_error_to_api_error(err: RouteEditingError) -> ApiError {
    match err {
        RouteEditingError::DraftNotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "route draft not found".to_string(),
            details: None,
        },
        RouteEditingError::RevisionConflict { expected, actual } => ApiError {
            status: StatusCode::CONFLICT,
            code: "REVISION_CONFLICT".to_string(),
            message: format!("revision conflict: expected {expected}, got {actual}"),
            details: None,
        },
        RouteEditingError::DraftNotActive => ApiError {
            status: StatusCode::CONFLICT,
            code: "DRAFT_NOT_ACTIVE".to_string(),
            message: "draft is not in active state".to_string(),
            details: None,
        },
        RouteEditingError::InvalidOperation { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_OPERATION".to_string(),
            message,
            details: None,
        },
        RouteEditingError::OperationFailed { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "OPERATION_FAILED".to_string(),
            message,
            details: None,
        },
        RouteEditingError::InsufficientPoints { minimum, actual } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INSUFFICIENT_POINTS".to_string(),
            message: format!("insufficient points: minimum {minimum}, got {actual}"),
            details: None,
        },
        RouteEditingError::InvalidSegmentIndex { index, count } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_SEGMENT_INDEX".to_string(),
            message: format!("invalid segment index: {index}, segment count: {count}"),
            details: None,
        },
        RouteEditingError::InvalidPointIndex { index, count } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_POINT_INDEX".to_string(),
            message: format!("invalid point index: {index}, point count: {count}"),
            details: None,
        },
        RouteEditingError::NothingToUndo => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "NOTHING_TO_UNDO".to_string(),
            message: "nothing to undo".to_string(),
            details: None,
        },
        RouteEditingError::NothingToRedo => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "NOTHING_TO_REDO".to_string(),
            message: "nothing to redo".to_string(),
            details: None,
        },
        RouteEditingError::InvalidCoordinate { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_COORDINATE".to_string(),
            message,
            details: None,
        },
    }
}

/// Extract the Idempotency-Key header value.
fn extract_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header is required".to_string(),
            details: None,
        })?;

    if key.trim().is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header must not be empty".to_string(),
            details: None,
        });
    }

    Ok(key.to_string())
}

/// POST /v1/activities/{activityId}/route-drafts
///
/// Create a new route draft for the given activity.
pub async fn post_create_draft(
    State(state): State<RouteEditingAppState>,
    actor: AuthenticatedActor,
    Path(activity_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<CreateRouteDraftRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let _idempotency_key = extract_idempotency_key(&headers)?;

    let geometry = geometry_to_domain(&body.geometry).map_err(|msg| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "VALIDATION_FAILED".to_string(),
        message: msg,
        details: None,
    })?;

    // Check if there is already an active draft for this activity
    let existing = state
        .repo
        .find_active_by_activity(ActivityId(activity_id), actor.0.user_id)
        .await
        .map_err(route_editing_error_to_api_error)?;

    if let Some(existing_draft) = existing {
        // Idempotent: return existing draft
        let response = draft_to_response(&existing_draft);
        return Ok((StatusCode::CREATED, Json(response)));
    }

    let draft =
        RouteDraft::create_from_geometry(actor.0.user_id, ActivityId(activity_id), None, geometry)
            .map_err(route_editing_error_to_api_error)?;

    state
        .repo
        .save(&draft)
        .await
        .map_err(route_editing_error_to_api_error)?;

    let response = draft_to_response(&draft);
    Ok((StatusCode::CREATED, Json(response)))
}

/// GET /v1/route-drafts/{draftId}
///
/// Get the current state of a route draft.
pub async fn get_draft(
    State(state): State<RouteEditingAppState>,
    actor: AuthenticatedActor,
    Path(draft_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let draft = state
        .repo
        .find_by_id(RouteDraftId::new(draft_id))
        .await
        .map_err(route_editing_error_to_api_error)?
        .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

    // Check ownership
    if draft.owner_id != actor.0.user_id {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this draft".to_string(),
            details: None,
        });
    }

    let response = draft_to_response(&draft);
    Ok((StatusCode::OK, Json(response)))
}

/// POST /v1/route-drafts/{draftId}/operations
///
/// Apply an operation to the draft. Requires Idempotency-Key header.
pub async fn post_apply_operation(
    State(state): State<RouteEditingAppState>,
    actor: AuthenticatedActor,
    Path(draft_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<ApplyOperationRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let idempotency_key = extract_idempotency_key(&headers)?;
    let operation_id = parse_idempotency_key(&idempotency_key).map_err(|msg| ApiError {
        status: StatusCode::BAD_REQUEST,
        code: "INVALID_IDEMPOTENCY_KEY".to_string(),
        message: msg,
        details: None,
    })?;

    // Check if this operation was already applied (idempotency)
    if let Some(existing_draft_id) = state
        .repo
        .find_by_operation_id(operation_id)
        .await
        .map_err(route_editing_error_to_api_error)?
    {
        // Replay: return the current state
        let draft = state
            .repo
            .find_by_id(existing_draft_id)
            .await
            .map_err(route_editing_error_to_api_error)?
            .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

        return Ok((
            StatusCode::OK,
            Json(OperationResultResponse {
                draft_id: draft.id.0,
                revision: draft.revision,
                can_undo: !draft.applied_operations.is_empty(),
                can_redo: !draft.undone_operations.is_empty(),
            }),
        ));
    }

    let mut draft = state
        .repo
        .find_by_id(RouteDraftId::new(draft_id))
        .await
        .map_err(route_editing_error_to_api_error)?
        .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

    // Check ownership
    if draft.owner_id != actor.0.user_id {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this draft".to_string(),
            details: None,
        });
    }

    let operation = body.operation.to_domain().map_err(|msg| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "INVALID_OPERATION".to_string(),
        message: msg,
        details: None,
    })?;

    draft
        .apply_operation(operation_id, operation, body.expected_revision)
        .map_err(route_editing_error_to_api_error)?;

    state
        .repo
        .update(&draft)
        .await
        .map_err(route_editing_error_to_api_error)?;

    Ok((
        StatusCode::OK,
        Json(OperationResultResponse {
            draft_id: draft.id.0,
            revision: draft.revision,
            can_undo: !draft.applied_operations.is_empty(),
            can_redo: !draft.undone_operations.is_empty(),
        }),
    ))
}

/// POST /v1/route-drafts/{draftId}/undo
///
/// Undo the last applied operation. Requires Idempotency-Key header.
/// Note: Idempotency for undo is enforced via expectedRevision. A retry with
/// the same expectedRevision will fail with RevisionConflict if the first
/// request already succeeded (which incremented the revision). This is safe
/// because clients must always send the current known revision.
pub async fn post_undo(
    State(state): State<RouteEditingAppState>,
    actor: AuthenticatedActor,
    Path(draft_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<UndoRedoRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let _idempotency_key = extract_idempotency_key(&headers)?;

    let mut draft = state
        .repo
        .find_by_id(RouteDraftId::new(draft_id))
        .await
        .map_err(route_editing_error_to_api_error)?
        .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

    // Check ownership
    if draft.owner_id != actor.0.user_id {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this draft".to_string(),
            details: None,
        });
    }

    draft
        .undo(body.expected_revision)
        .map_err(route_editing_error_to_api_error)?;

    state
        .repo
        .update(&draft)
        .await
        .map_err(route_editing_error_to_api_error)?;

    Ok((
        StatusCode::OK,
        Json(OperationResultResponse {
            draft_id: draft.id.0,
            revision: draft.revision,
            can_undo: !draft.applied_operations.is_empty(),
            can_redo: !draft.undone_operations.is_empty(),
        }),
    ))
}

/// POST /v1/route-drafts/{draftId}/redo
///
/// Redo the last undone operation. Requires Idempotency-Key header.
/// Note: Idempotency for redo is enforced via expectedRevision (same as undo).
pub async fn post_redo(
    State(state): State<RouteEditingAppState>,
    actor: AuthenticatedActor,
    Path(draft_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<UndoRedoRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let _idempotency_key = extract_idempotency_key(&headers)?;

    let mut draft = state
        .repo
        .find_by_id(RouteDraftId::new(draft_id))
        .await
        .map_err(route_editing_error_to_api_error)?
        .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

    // Check ownership
    if draft.owner_id != actor.0.user_id {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this draft".to_string(),
            details: None,
        });
    }

    draft
        .redo(body.expected_revision)
        .map_err(route_editing_error_to_api_error)?;

    state
        .repo
        .update(&draft)
        .await
        .map_err(route_editing_error_to_api_error)?;

    Ok((
        StatusCode::OK,
        Json(OperationResultResponse {
            draft_id: draft.id.0,
            revision: draft.revision,
            can_undo: !draft.applied_operations.is_empty(),
            can_redo: !draft.undone_operations.is_empty(),
        }),
    ))
}

/// POST /v1/route-drafts/{draftId}/reset
///
/// Reset the draft to new base geometry. Requires Idempotency-Key header.
/// Note: Idempotency for reset is enforced via expectedRevision (same as undo/redo).
pub async fn post_reset(
    State(state): State<RouteEditingAppState>,
    actor: AuthenticatedActor,
    Path(draft_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<ResetRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let _idempotency_key = extract_idempotency_key(&headers)?;

    let geometry = geometry_to_domain(&body.geometry).map_err(|msg| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "VALIDATION_FAILED".to_string(),
        message: msg,
        details: None,
    })?;

    let mut draft = state
        .repo
        .find_by_id(RouteDraftId::new(draft_id))
        .await
        .map_err(route_editing_error_to_api_error)?
        .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

    // Check ownership
    if draft.owner_id != actor.0.user_id {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this draft".to_string(),
            details: None,
        });
    }

    draft
        .reset(body.expected_revision, geometry)
        .map_err(route_editing_error_to_api_error)?;

    state
        .repo
        .update(&draft)
        .await
        .map_err(route_editing_error_to_api_error)?;

    Ok((
        StatusCode::OK,
        Json(OperationResultResponse {
            draft_id: draft.id.0,
            revision: draft.revision,
            can_undo: !draft.applied_operations.is_empty(),
            can_redo: !draft.undone_operations.is_empty(),
        }),
    ))
}

/// DELETE /v1/route-drafts/{draftId}
///
/// Discard the draft.
pub async fn delete_draft(
    State(state): State<RouteEditingAppState>,
    actor: AuthenticatedActor,
    Path(draft_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let mut draft = state
        .repo
        .find_by_id(RouteDraftId::new(draft_id))
        .await
        .map_err(route_editing_error_to_api_error)?
        .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

    // Check ownership
    if draft.owner_id != actor.0.user_id {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN".to_string(),
            message: "not authorized to access this draft".to_string(),
            details: None,
        });
    }

    draft.discard().map_err(route_editing_error_to_api_error)?;

    state
        .repo
        .update(&draft)
        .await
        .map_err(route_editing_error_to_api_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "route_editing_tests.rs"]
mod tests;
