//! Request and response DTOs for route editing endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use haiker_app::route_editing::{
    Coordinate, Elevation, OperationId, PointIndex, RouteOperation, RoutePoint, SegmentIndex,
};

/// Request body for POST /v1/activities/{activityId}/route-drafts.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct CreateRouteDraftRequest {
    /// Initial geometry as array of segments, each segment is array of points.
    pub geometry: Vec<Vec<RoutePointDto>>,
    /// Optional base route version ID to anchor the draft to.
    #[serde(default)]
    pub base_route_version_id: Option<Uuid>,
}

/// A route point in DTO form.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RoutePointDto {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation: Option<f64>,
}

impl RoutePointDto {
    pub fn to_domain(&self) -> Result<RoutePoint, String> {
        let coordinate = Coordinate::new(self.latitude, self.longitude)
            .map_err(|e| format!("invalid coordinate: {e}"))?;
        let elevation = self.elevation.map(Elevation::new);
        Ok(RoutePoint::new(coordinate, elevation))
    }

    pub fn from_domain(point: &RoutePoint) -> Self {
        Self {
            latitude: point.coordinate.latitude,
            longitude: point.coordinate.longitude,
            elevation: point.elevation.map(|e| e.meters()),
        }
    }
}

/// Request body for POST /v1/route-drafts/{draftId}/operations.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOperationRequest {
    pub operation: OperationDto,
    pub expected_revision: u64,
}

/// DTO representing a route operation (tagged union).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OperationDto {
    #[serde(rename_all = "camelCase")]
    MovePoint {
        segment_index: usize,
        point_index: usize,
        new_position: PositionDto,
    },
    #[serde(rename_all = "camelCase")]
    AddPoint {
        segment_index: usize,
        after_point_index: usize,
        point: RoutePointDto,
    },
    #[serde(rename_all = "camelCase")]
    DeletePoint {
        segment_index: usize,
        point_index: usize,
    },
    #[serde(rename_all = "camelCase")]
    DeleteSection {
        segment_index: usize,
        start_index: usize,
        end_index: usize,
    },
    #[serde(rename_all = "camelCase")]
    ReplaceSection {
        segment_index: usize,
        start_index: usize,
        end_index: usize,
        replacement: Vec<RoutePointDto>,
    },
    #[serde(rename_all = "camelCase")]
    SplitSegment {
        segment_index: usize,
        at_point_index: usize,
    },
    #[serde(rename_all = "camelCase")]
    JoinSegments {
        first_segment_index: usize,
        second_segment_index: usize,
    },
}

/// A simple lat/lon position DTO.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PositionDto {
    pub latitude: f64,
    pub longitude: f64,
}

impl OperationDto {
    /// Convert this DTO to the domain RouteOperation.
    pub fn to_domain(&self) -> Result<RouteOperation, String> {
        match self {
            OperationDto::MovePoint {
                segment_index,
                point_index,
                new_position,
            } => {
                let coord = Coordinate::new(new_position.latitude, new_position.longitude)
                    .map_err(|e| e.to_string())?;
                Ok(RouteOperation::MovePoint {
                    segment_index: SegmentIndex::new(*segment_index),
                    point_index: PointIndex::new(*point_index),
                    new_position: coord,
                })
            }
            OperationDto::AddPoint {
                segment_index,
                after_point_index,
                point,
            } => {
                let domain_point = point.to_domain()?;
                Ok(RouteOperation::AddPoint {
                    segment_index: SegmentIndex::new(*segment_index),
                    after_point_index: PointIndex::new(*after_point_index),
                    point: domain_point,
                })
            }
            OperationDto::DeletePoint {
                segment_index,
                point_index,
            } => Ok(RouteOperation::DeletePoint {
                segment_index: SegmentIndex::new(*segment_index),
                point_index: PointIndex::new(*point_index),
            }),
            OperationDto::DeleteSection {
                segment_index,
                start_index,
                end_index,
            } => Ok(RouteOperation::DeleteSection {
                segment_index: SegmentIndex::new(*segment_index),
                start_index: PointIndex::new(*start_index),
                end_index: PointIndex::new(*end_index),
            }),
            OperationDto::ReplaceSection {
                segment_index,
                start_index,
                end_index,
                replacement,
            } => {
                let domain_points = replacement
                    .iter()
                    .map(|p| p.to_domain())
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(RouteOperation::ReplaceSection {
                    segment_index: SegmentIndex::new(*segment_index),
                    start_index: PointIndex::new(*start_index),
                    end_index: PointIndex::new(*end_index),
                    replacement: domain_points,
                })
            }
            OperationDto::SplitSegment {
                segment_index,
                at_point_index,
            } => Ok(RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(*segment_index),
                at_point_index: PointIndex::new(*at_point_index),
            }),
            OperationDto::JoinSegments {
                first_segment_index,
                second_segment_index,
            } => Ok(RouteOperation::JoinSegments {
                first_segment_index: SegmentIndex::new(*first_segment_index),
                second_segment_index: SegmentIndex::new(*second_segment_index),
            }),
        }
    }
}

/// Request body for POST /v1/route-drafts/{draftId}/undo and /redo.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct UndoRedoRequest {
    pub expected_revision: u64,
}

/// Response body for GET /v1/route-drafts/{draftId}.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDraftResponse {
    pub id: Uuid,
    pub activity_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_route_version_id: Option<Uuid>,
    pub revision: u64,
    pub state: String,
    pub geometry: Vec<Vec<RoutePointDto>>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response body for mutation operations (apply, undo, redo, reset).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResultResponse {
    pub draft_id: Uuid,
    pub revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Convert geometry from DTO to domain.
pub fn geometry_to_domain(dto: &[Vec<RoutePointDto>]) -> Result<Vec<Vec<RoutePoint>>, String> {
    dto.iter()
        .map(|segment| {
            segment
                .iter()
                .map(|p| p.to_domain())
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Convert geometry from domain to DTO.
pub fn geometry_to_dto(geometry: &[Vec<RoutePoint>]) -> Vec<Vec<RoutePointDto>> {
    geometry
        .iter()
        .map(|segment| segment.iter().map(RoutePointDto::from_domain).collect())
        .collect()
}

/// Convert a RouteDraft to RouteDraftResponse.
pub fn draft_to_response(draft: &haiker_app::route_editing::RouteDraft) -> RouteDraftResponse {
    RouteDraftResponse {
        id: draft.id.0,
        activity_id: draft.activity_id.0,
        base_route_version_id: draft.base_route_version_id,
        revision: draft.revision,
        state: match draft.state {
            haiker_app::route_editing::DraftState::Active => "active".to_string(),
            haiker_app::route_editing::DraftState::Published => "published".to_string(),
            haiker_app::route_editing::DraftState::Discarded => "discarded".to_string(),
        },
        geometry: geometry_to_dto(&draft.geometry),
        can_undo: !draft.applied_operations.is_empty(),
        can_redo: !draft.undone_operations.is_empty(),
        created_at: draft.created_at,
        updated_at: draft.updated_at,
    }
}

/// Extract the Idempotency-Key as an OperationId.
pub fn parse_idempotency_key(key: &str) -> Result<OperationId, String> {
    let uuid = Uuid::parse_str(key).map_err(|e| format!("invalid idempotency key: {e}"))?;
    Ok(OperationId::new(uuid))
}

/// Request body for POST /v1/route-drafts/{draftId}/validation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ValidateForPublicationRequest {
    pub expected_revision: u64,
}

/// Response body for POST /v1/route-drafts/{draftId}/validation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResultResponse {
    pub valid: bool,
    pub errors: Vec<ValidationErrorDto>,
}

/// A single validation error in the response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationErrorDto {
    pub code: String,
    pub detail: String,
}

/// Request body for POST /v1/route-drafts/{draftId}/publication.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PublishRouteDraftRequest {
    pub expected_revision: u64,
    #[serde(default)]
    pub edit_summary: Option<String>,
}

/// Response body for POST /v1/route-drafts/{draftId}/publication.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationResponse {
    pub route_version_id: Uuid,
    pub version_number: i32,
    pub draft_id: Uuid,
}
