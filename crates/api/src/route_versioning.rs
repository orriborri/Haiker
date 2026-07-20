//! Route Versioning API handlers.
//!
//! Implements:
//! - GET /v1/activities/{activityId}/route-versions
//! - GET /v1/route-versions/{routeVersionId}
//! - GET /v1/route-versions/{routeVersionId}/geometry

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use haiker_app::activity_catalog::repository::ActivityRepository;
use haiker_app::activity_catalog::ActivityId;
use haiker_app::route_versioning::queries::{
    get_route_version, get_route_version_geometry, list_route_versions,
};
use haiker_app::route_versioning::repository::RouteVersionRepository;
use haiker_app::route_versioning::{RouteVersionId, RouteVersioningError};

use crate::error::ApiError;
use crate::route_versioning_dto::{
    GeoJsonFeature, GeoJsonGeometry, PaginationMeta, RouteVersionDetailResponse,
    RouteVersionFeatureProperties, RouteVersionGeometryResponse, RouteVersionListParams,
    RouteVersionListResponse, RouteVersionSummaryResponse,
};
use haiker_platform::auth_middleware::{AuthSession, HasSessionStore};
use haiker_platform::session::SessionStore;

/// Shared application state for route versioning handlers.
#[derive(Clone)]
pub struct RouteVersioningAppState {
    pub activity_repo: Arc<dyn ActivityRepository>,
    pub version_repo: Arc<dyn RouteVersionRepository>,
    pub session_store: SessionStore,
}

impl HasSessionStore for RouteVersioningAppState {
    fn session_store(&self) -> &SessionStore {
        &self.session_store
    }
}

/// Convert a RouteVersioningError to an ApiError with Problem Details fields.
fn versioning_error_to_api_error(err: RouteVersioningError) -> ApiError {
    match err {
        RouteVersioningError::NotFound | RouteVersioningError::NotAuthorized => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND".to_string(),
            message: "route version not found".to_string(),
            problem_type: Some("/problems/not-found".to_string()),
            title: Some("Not Found".to_string()),
            request_id: None,
            details: None,
        },
        RouteVersioningError::PersistenceError { message } => ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_string(),
            message: format!("internal error: {message}"),
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

/// GET /v1/activities/{activityId}/route-versions
///
/// List route versions for an activity, ordered by version_number descending.
/// Returns 404 for missing, cross-owner, or deleted activities (non-disclosing).
#[tracing::instrument(skip(state, actor))]
pub async fn get_route_versions_list(
    State(state): State<RouteVersioningAppState>,
    actor: AuthSession,
    Path(activity_id): Path<Uuid>,
    Query(params): Query<RouteVersionListParams>,
) -> Result<impl IntoResponse, ApiError> {
    let page = list_route_versions(
        ActivityId(activity_id),
        actor.0.user_id,
        params.cursor.as_deref(),
        params.page_size,
        state.activity_repo.as_ref(),
        state.version_repo.as_ref(),
    )
    .await
    .map_err(versioning_error_to_api_error)?;

    let page_size = params.page_size.unwrap_or(25).clamp(1, 100);

    let items: Vec<RouteVersionSummaryResponse> = page
        .items
        .iter()
        .map(|v| RouteVersionSummaryResponse {
            id: v.id.0,
            activity_id: v.activity_id.0,
            parent_version_id: v.parent_version_id.map(|pid| pid.0),
            version_number: v.version_number,
            edit_summary: v.edit_summary.clone(),
            corrected_statistics: serde_json::to_value(&v.corrected_statistics).unwrap_or_default(),
            calculation_version: v.calculation_version.clone(),
            created_by: v.created_by.0,
            created_at: v.created_at,
        })
        .collect();

    let response = RouteVersionListResponse {
        items,
        pagination: PaginationMeta {
            cursor: page.next_cursor,
            has_more: page.has_more,
            page_size,
        },
    };

    Ok((StatusCode::OK, Json(response)))
}

/// GET /v1/route-versions/{routeVersionId}
///
/// Get detail for a specific route version.
/// Returns 404 for missing, cross-owner, or deleted activity (non-disclosing).
#[tracing::instrument(skip(state, actor))]
pub async fn get_route_version_detail(
    State(state): State<RouteVersioningAppState>,
    actor: AuthSession,
    Path(route_version_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let version = get_route_version(
        RouteVersionId(route_version_id),
        actor.0.user_id,
        state.activity_repo.as_ref(),
        state.version_repo.as_ref(),
    )
    .await
    .map_err(versioning_error_to_api_error)?;

    let response = RouteVersionDetailResponse {
        id: version.id.0,
        activity_id: version.activity_id.0,
        parent_version_id: version.parent_version_id.map(|pid| pid.0),
        version_number: version.version_number,
        edit_summary: version.edit_summary,
        corrected_statistics: serde_json::to_value(&version.corrected_statistics)
            .unwrap_or_default(),
        calculation_version: version.calculation_version,
        created_by: version.created_by.0,
        created_at: version.created_at,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// GET /v1/route-versions/{routeVersionId}/geometry
///
/// Get the geometry for a route version as GeoJSON FeatureCollection.
/// Content-Type: application/geo+json.
/// Returns 404 for missing, cross-owner, or deleted activity (non-disclosing).
#[tracing::instrument(skip(state, actor))]
pub async fn get_route_version_geometry_handler(
    State(state): State<RouteVersioningAppState>,
    actor: AuthSession,
    Path(route_version_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let geometry_result = get_route_version_geometry(
        RouteVersionId(route_version_id),
        actor.0.user_id,
        state.activity_repo.as_ref(),
        state.version_repo.as_ref(),
    )
    .await
    .map_err(versioning_error_to_api_error)?;

    let bbox = [
        geometry_result.bounding_box.south_west.longitude, // west
        geometry_result.bounding_box.south_west.latitude,  // south
        geometry_result.bounding_box.north_east.longitude, // east
        geometry_result.bounding_box.north_east.latitude,  // north
    ];

    let coordinates: Vec<[f64; 2]> = geometry_result
        .geometry
        .iter()
        .map(|c| [c.longitude, c.latitude]) // GeoJSON: [longitude, latitude]
        .collect();

    let distance_meters = Some(geometry_result.corrected_statistics.distance_meters);
    let point_count = coordinates.len();

    let feature = GeoJsonFeature {
        feature_type: "Feature".to_string(),
        geometry: GeoJsonGeometry {
            geometry_type: "LineString".to_string(),
            coordinates,
        },
        properties: RouteVersionFeatureProperties {
            point_count,
            distance_meters,
        },
    };

    let response = RouteVersionGeometryResponse {
        geojson_type: "FeatureCollection".to_string(),
        bbox,
        features: vec![feature],
    };

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/geo+json")],
        Json(response),
    ))
}
