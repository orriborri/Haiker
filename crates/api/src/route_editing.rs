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
    ActivityGateway, RouteDraft, RouteDraftId, RouteDraftRepository, RouteEditingError,
    RouteVersionGateway,
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
    pub activity_gateway: Arc<dyn ActivityGateway>,
    pub route_version_gateway: Arc<dyn RouteVersionGateway>,
}

/// Convert a RouteEditingError to an ApiError with Problem Details fields.
fn route_editing_error_to_api_error(err: RouteEditingError) -> ApiError {
    match err {
        RouteEditingError::DraftNotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "route draft not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::RevisionConflict { expected, actual } => ApiError {
            status: StatusCode::CONFLICT,
            code: "ROUTE_DRAFT_REVISION_CONFLICT".to_string(),
            message: format!("revision conflict: expected {expected}, got {actual}"),
            problem_type: Some("/problems/stale-route-draft".to_string()),
            title: Some("Route draft revision is stale".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::DraftNotActive => ApiError {
            status: StatusCode::CONFLICT,
            code: "DRAFT_NOT_ACTIVE".to_string(),
            message: "draft is not in active state".to_string(),
            problem_type: Some("/problems/draft-not-active".to_string()),
            title: Some("Draft Not Active".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::InvalidOperation { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_OPERATION".to_string(),
            message,
            problem_type: Some("/problems/invalid-operation".to_string()),
            title: Some("Invalid Operation".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::OperationFailed { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "OPERATION_FAILED".to_string(),
            message,
            problem_type: Some("/problems/operation-failed".to_string()),
            title: Some("Operation Failed".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::InsufficientPoints { minimum, actual } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INSUFFICIENT_POINTS".to_string(),
            message: format!("insufficient points: minimum {minimum}, got {actual}"),
            problem_type: Some("/problems/insufficient-points".to_string()),
            title: Some("Insufficient Points".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::InvalidSegmentIndex { index, count } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_SEGMENT_INDEX".to_string(),
            message: format!("invalid segment index: {index}, segment count: {count}"),
            problem_type: Some("/problems/invalid-segment-index".to_string()),
            title: Some("Invalid Segment Index".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::InvalidPointIndex { index, count } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_POINT_INDEX".to_string(),
            message: format!("invalid point index: {index}, point count: {count}"),
            problem_type: Some("/problems/invalid-point-index".to_string()),
            title: Some("Invalid Point Index".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::NothingToUndo => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "NOTHING_TO_UNDO".to_string(),
            message: "nothing to undo".to_string(),
            problem_type: Some("/problems/nothing-to-undo".to_string()),
            title: Some("Nothing To Undo".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::NothingToRedo => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "NOTHING_TO_REDO".to_string(),
            message: "nothing to redo".to_string(),
            problem_type: Some("/problems/nothing-to-redo".to_string()),
            title: Some("Nothing To Redo".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::InvalidCoordinate { message } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_COORDINATE".to_string(),
            message,
            problem_type: Some("/problems/invalid-coordinate".to_string()),
            title: Some("Invalid Coordinate".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::ActivityNotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "activity not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::ActivityDeleted => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "ACTIVITY_DELETED".to_string(),
            message: "activity is deleted".to_string(),
            problem_type: Some("/problems/activity-deleted".to_string()),
            title: Some("Activity Deleted".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::InvalidBaseRouteVersion => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "INVALID_BASE_ROUTE_VERSION".to_string(),
            message: "invalid base route version".to_string(),
            problem_type: Some("/problems/invalid-base-route-version".to_string()),
            title: Some("Invalid Base Route Version".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::ReplacementTooLarge { maximum, actual } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "REPLACEMENT_TOO_LARGE".to_string(),
            message: format!("replacement too large: maximum {maximum}, actual {actual}"),
            problem_type: Some("/problems/replacement-too-large".to_string()),
            title: Some("Replacement Too Large".to_string()),
            request_id: None,
            details: None,
        },
        RouteEditingError::EndpointContinuityViolation {
            position,
            expected_lat,
            expected_lon,
            actual_lat,
            actual_lon,
        } => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "ENDPOINT_CONTINUITY_VIOLATION".to_string(),
            message: format!(
                "endpoint continuity violation at {position}: expected ({expected_lat}, {expected_lon}), got ({actual_lat}, {actual_lon})"
            ),
            problem_type: Some("/problems/endpoint-continuity-violation".to_string()),
            title: Some("Endpoint Continuity Violation".to_string()),
            request_id: None,
            details: None,
        },
    }
}

/// Extract the Idempotency-Key header value.
#[allow(clippy::result_large_err)]
fn extract_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header is required".to_string(),
            problem_type: Some("/problems/missing-idempotency-key".to_string()),
            title: Some("Missing Idempotency Key".to_string()),
            request_id: None,
            details: None,
        })?;

    if key.trim().is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "MISSING_IDEMPOTENCY_KEY".to_string(),
            message: "Idempotency-Key header must not be empty".to_string(),
            problem_type: Some("/problems/missing-idempotency-key".to_string()),
            title: Some("Missing Idempotency Key".to_string()),
            request_id: None,
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
        problem_type: Some("/problems/validation-failed".to_string()),
        title: Some("Validation Failed".to_string()),
        request_id: None,
        details: None,
    })?;

    // Check if there is already an active draft for this activity (idempotency).
    // This runs before gateway validation so that retries succeed even if the
    // activity is later soft-deleted.
    let existing = state
        .repo
        .find_active_by_activity(ActivityId(activity_id), actor.0.user_id)
        .await
        .map_err(route_editing_error_to_api_error)?;

    if let Some(existing_draft) = existing {
        // Same key + different payload -> return error (idempotency contract)
        if let Some(requested_base) = body.base_route_version_id {
            if Some(requested_base) != existing_draft.base_route_version_id {
                return Err(ApiError {
                    status: StatusCode::CONFLICT,
                    code: "IDEMPOTENCY_CONFLICT".to_string(),
                    message: "existing draft has a different baseRouteVersionId".to_string(),
                    problem_type: Some("/problems/idempotency-conflict".to_string()),
                    title: Some("Idempotency Conflict".to_string()),
                    request_id: None,
                    details: None,
                });
            }
        }

        // Idempotent: return existing draft
        let response = draft_to_response(&existing_draft);
        return Ok((StatusCode::CREATED, Json(response)));
    }

    // Validate activity existence, ownership, and lifecycle
    state
        .activity_gateway
        .validate_activity_for_draft(ActivityId(activity_id), actor.0.user_id)
        .await
        .map_err(route_editing_error_to_api_error)?;

    // Validate base route version if provided
    if let Some(base_version_id) = body.base_route_version_id {
        state
            .route_version_gateway
            .validate_route_version_exists(base_version_id, ActivityId(activity_id))
            .await
            .map_err(route_editing_error_to_api_error)?;
    }

    let draft = RouteDraft::create_from_geometry(
        actor.0.user_id,
        ActivityId(activity_id),
        body.base_route_version_id,
        geometry,
    )
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
            problem_type: Some("/problems/forbidden".to_string()),
            title: Some("Forbidden".to_string()),
            request_id: None,
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
        problem_type: Some("/problems/invalid-idempotency-key".to_string()),
        title: Some("Invalid Idempotency Key".to_string()),
        request_id: None,
        details: None,
    })?;

    // Check if this operation was already applied (idempotency)
    if let Some(existing_draft_id) = state
        .repo
        .find_by_operation_id(operation_id)
        .await
        .map_err(route_editing_error_to_api_error)?
    {
        // Load the draft to compare the stored operation with the incoming one
        let draft = state
            .repo
            .find_by_id(existing_draft_id)
            .await
            .map_err(route_editing_error_to_api_error)?
            .ok_or_else(|| route_editing_error_to_api_error(RouteEditingError::DraftNotFound))?;

        // Find the stored operation entry for this operation_id
        let stored_entry = draft
            .applied_operations
            .iter()
            .find(|e| e.id == operation_id);

        if let Some(entry) = stored_entry {
            // Convert incoming operation to domain form for comparison
            let incoming_operation = body.operation.to_domain().map_err(|msg| ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "INVALID_OPERATION".to_string(),
                message: msg,
                problem_type: None,
                title: None,
                request_id: None,
                details: None,
            })?;

            // Payload mismatch detection: same key but different operation
            if entry.operation != incoming_operation {
                return Err(ApiError {
                    status: StatusCode::CONFLICT,
                    code: "IDEMPOTENCY_PAYLOAD_MISMATCH".to_string(),
                    message: "idempotency key reused with a different operation payload"
                        .to_string(),
                    problem_type: Some("/problems/idempotency-conflict".to_string()),
                    title: Some("Idempotency conflict".to_string()),
                    request_id: None,
                    details: None,
                });
            }

            // Derive the revision this operation produced from its stack position.
            // Each operation increments revision by 1 starting from 0, so the
            // operation at index N produced revision N+1.
            let entry_index = draft
                .applied_operations
                .iter()
                .position(|e| e.id == operation_id)
                .unwrap_or(0);
            let snapshot_revision = (entry_index as u64) + 1;

            // Replay: return the state as it was when this operation was originally applied.
            // can_undo is true (at least this operation exists), can_redo is false
            // (new operations clear the redo stack at the time of application).
            return Ok((
                StatusCode::OK,
                Json(OperationResultResponse {
                    draft_id: draft.id.0,
                    revision: snapshot_revision,
                    can_undo: true,
                    can_redo: false,
                }),
            ));
        }

        // Operation ID was found in the repository but not in applied_operations
        // (should not normally happen). Fall through to return current state.
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
            problem_type: Some("/problems/forbidden".to_string()),
            title: Some("Forbidden".to_string()),
            request_id: None,
            details: None,
        });
    }

    let operation = body.operation.to_domain().map_err(|msg| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "INVALID_OPERATION".to_string(),
        message: msg,
        problem_type: Some("/problems/invalid-operation".to_string()),
        title: Some("Invalid Operation".to_string()),
        request_id: None,
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
            problem_type: Some("/problems/forbidden".to_string()),
            title: Some("Forbidden".to_string()),
            request_id: None,
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
            problem_type: Some("/problems/forbidden".to_string()),
            title: Some("Forbidden".to_string()),
            request_id: None,
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
        problem_type: Some("/problems/validation-failed".to_string()),
        title: Some("Validation Failed".to_string()),
        request_id: None,
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
            problem_type: Some("/problems/forbidden".to_string()),
            title: Some("Forbidden".to_string()),
            request_id: None,
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
            problem_type: Some("/problems/forbidden".to_string()),
            title: Some("Forbidden".to_string()),
            request_id: None,
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
