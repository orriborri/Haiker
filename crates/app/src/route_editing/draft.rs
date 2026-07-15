//! RouteDraft aggregate - the core domain entity for route editing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;

use super::operations::RouteOperation;
use super::value_objects::{
    OperationId, PointIndex, RoutePoint, SegmentIndex, MAX_REPLACEMENT_POINTS,
};
use super::RouteEditingError;

/// A strongly-typed route draft identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouteDraftId(pub Uuid);

impl RouteDraftId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for RouteDraftId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The lifecycle state of a route draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftState {
    Active,
    Published,
    Discarded,
}

/// An entry in the operation stack storing the operation and geometry snapshot for undo.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationEntry {
    pub id: OperationId,
    pub operation: RouteOperation,
    /// Geometry state before this operation was applied (enables deterministic undo).
    pub geometry_before: Vec<Vec<RoutePoint>>,
}

/// The RouteDraft aggregate root.
///
/// Represents a working copy of a route that can be edited through operations.
/// Enforces all domain invariants: revision-based optimistic concurrency,
/// operation idempotency, state guards, and geometry validity.
#[derive(Debug, Clone)]
pub struct RouteDraft {
    pub id: RouteDraftId,
    pub activity_id: ActivityId,
    pub owner_id: UserId,
    pub base_route_version_id: Option<Uuid>,
    pub revision: u64,
    pub geometry: Vec<Vec<RoutePoint>>,
    pub applied_operations: Vec<OperationEntry>,
    pub undone_operations: Vec<OperationEntry>,
    pub state: DraftState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RouteDraft {
    /// Create a new draft from base geometry.
    /// Requires at least 1 segment with at least 2 points each.
    pub fn create_from_geometry(
        owner_id: UserId,
        activity_id: ActivityId,
        base_route_version_id: Option<Uuid>,
        geometry: Vec<Vec<RoutePoint>>,
    ) -> Result<Self, RouteEditingError> {
        Self::validate_geometry(&geometry)?;
        let now = Utc::now();
        Ok(Self {
            id: RouteDraftId::generate(),
            activity_id,
            owner_id,
            base_route_version_id,
            revision: 0,
            geometry,
            applied_operations: Vec::new(),
            undone_operations: Vec::new(),
            state: DraftState::Active,
            created_at: now,
            updated_at: now,
        })
    }

    /// Apply an operation to the draft.
    ///
    /// Enforces: state guard, revision check, idempotency, geometry validation.
    pub fn apply_operation(
        &mut self,
        operation_id: OperationId,
        operation: RouteOperation,
        expected_revision: u64,
    ) -> Result<(), RouteEditingError> {
        self.check_active()?;
        self.check_revision(expected_revision)?;

        // Idempotency: if this operation_id was already applied, no-op
        if self.applied_operations.iter().any(|e| e.id == operation_id) {
            return Ok(());
        }

        // Snapshot geometry before mutation for deterministic undo
        let geometry_before = self.geometry.clone();

        // Validate and apply
        self.execute_operation(&operation)?;

        // Clear redo stack on new operation
        self.undone_operations.clear();
        self.applied_operations.push(OperationEntry {
            id: operation_id,
            operation,
            geometry_before,
        });
        self.revision += 1;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Undo the last applied operation.
    ///
    /// Restores geometry to the state before the last operation was applied.
    pub fn undo(&mut self, expected_revision: u64) -> Result<(), RouteEditingError> {
        self.check_active()?;
        self.check_revision(expected_revision)?;

        let mut entry = self
            .applied_operations
            .pop()
            .ok_or(RouteEditingError::NothingToUndo)?;

        // Swap: current geometry becomes the "geometry_before" for redo,
        // and we restore the snapshot.
        std::mem::swap(&mut self.geometry, &mut entry.geometry_before);
        self.undone_operations.push(entry);
        self.revision += 1;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Redo the last undone operation.
    ///
    /// Re-applies the undone operation by restoring the post-operation geometry.
    pub fn redo(&mut self, expected_revision: u64) -> Result<(), RouteEditingError> {
        self.check_active()?;
        self.check_revision(expected_revision)?;

        let mut entry = self
            .undone_operations
            .pop()
            .ok_or(RouteEditingError::NothingToRedo)?;

        // Swap: current geometry (pre-redo) becomes geometry_before,
        // and we restore the post-operation geometry stored in entry.
        std::mem::swap(&mut self.geometry, &mut entry.geometry_before);
        self.applied_operations.push(entry);
        self.revision += 1;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Reset the draft to a new base geometry, clearing all operation stacks.
    pub fn reset(
        &mut self,
        expected_revision: u64,
        base_geometry: Vec<Vec<RoutePoint>>,
    ) -> Result<(), RouteEditingError> {
        self.check_active()?;
        self.check_revision(expected_revision)?;
        Self::validate_geometry(&base_geometry)?;

        self.geometry = base_geometry;
        self.applied_operations.clear();
        self.undone_operations.clear();
        self.revision += 1;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Publish the draft, making it immutable.
    pub fn publish(&mut self) -> Result<(), RouteEditingError> {
        self.check_active()?;
        self.state = DraftState::Published;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Discard the draft, making it immutable.
    pub fn discard(&mut self) -> Result<(), RouteEditingError> {
        self.check_active()?;
        self.state = DraftState::Discarded;
        self.updated_at = Utc::now();
        Ok(())
    }

    // --- Private helpers ---

    fn validate_geometry(geometry: &[Vec<RoutePoint>]) -> Result<(), RouteEditingError> {
        if geometry.is_empty() {
            return Err(RouteEditingError::InvalidOperation {
                message: "geometry must contain at least 1 segment".to_string(),
            });
        }
        for segment in geometry {
            if segment.len() < 2 {
                return Err(RouteEditingError::InsufficientPoints {
                    minimum: 2,
                    actual: segment.len(),
                });
            }
        }
        Ok(())
    }

    fn check_active(&self) -> Result<(), RouteEditingError> {
        if self.state != DraftState::Active {
            return Err(RouteEditingError::DraftNotActive);
        }
        Ok(())
    }

    fn check_revision(&self, expected: u64) -> Result<(), RouteEditingError> {
        if expected != self.revision {
            return Err(RouteEditingError::RevisionConflict {
                expected,
                actual: self.revision,
            });
        }
        Ok(())
    }

    fn validate_segment_idx(&self, index: SegmentIndex) -> Result<(), RouteEditingError> {
        if index.value() >= self.geometry.len() {
            return Err(RouteEditingError::InvalidSegmentIndex {
                index: index.value(),
                count: self.geometry.len(),
            });
        }
        Ok(())
    }

    fn validate_point_idx(
        &self,
        seg: SegmentIndex,
        pt: PointIndex,
    ) -> Result<(), RouteEditingError> {
        self.validate_segment_idx(seg)?;
        let segment = &self.geometry[seg.value()];
        if pt.value() >= segment.len() {
            return Err(RouteEditingError::InvalidPointIndex {
                index: pt.value(),
                count: segment.len(),
            });
        }
        Ok(())
    }

    /// Execute an operation on the current geometry (mutates in place).
    fn execute_operation(&mut self, operation: &RouteOperation) -> Result<(), RouteEditingError> {
        match operation {
            RouteOperation::MovePoint {
                segment_index,
                point_index,
                new_position,
            } => {
                self.validate_point_idx(*segment_index, *point_index)?;
                self.geometry[segment_index.value()][point_index.value()].coordinate =
                    *new_position;
                Ok(())
            }

            RouteOperation::AddPoint {
                segment_index,
                after_point_index,
                point,
            } => {
                self.validate_point_idx(*segment_index, *after_point_index)?;
                let insert_pos = after_point_index.value() + 1;
                self.geometry[segment_index.value()].insert(insert_pos, point.clone());
                Ok(())
            }

            RouteOperation::DeletePoint {
                segment_index,
                point_index,
            } => {
                self.validate_point_idx(*segment_index, *point_index)?;
                let segment = &self.geometry[segment_index.value()];
                if segment.len() <= 2 {
                    return Err(RouteEditingError::InsufficientPoints {
                        minimum: 2,
                        actual: segment.len() - 1,
                    });
                }
                self.geometry[segment_index.value()].remove(point_index.value());
                Ok(())
            }

            RouteOperation::DeleteSection {
                segment_index,
                start_index,
                end_index,
            } => {
                self.validate_segment_idx(*segment_index)?;
                let segment = &self.geometry[segment_index.value()];
                if start_index.value() > end_index.value() {
                    return Err(RouteEditingError::InvalidOperation {
                        message: "start_index must be <= end_index".to_string(),
                    });
                }
                if end_index.value() >= segment.len() {
                    return Err(RouteEditingError::InvalidPointIndex {
                        index: end_index.value(),
                        count: segment.len(),
                    });
                }
                let delete_count = end_index.value() - start_index.value() + 1;
                let remaining = segment.len() - delete_count;
                if remaining < 2 {
                    return Err(RouteEditingError::InsufficientPoints {
                        minimum: 2,
                        actual: remaining,
                    });
                }
                self.geometry[segment_index.value()].drain(start_index.value()..=end_index.value());
                Ok(())
            }

            RouteOperation::ReplaceSection {
                segment_index,
                start_index,
                end_index,
                replacement,
            } => {
                self.validate_segment_idx(*segment_index)?;
                let segment = &self.geometry[segment_index.value()];
                if start_index.value() > end_index.value() {
                    return Err(RouteEditingError::InvalidOperation {
                        message: "start_index must be <= end_index".to_string(),
                    });
                }
                if end_index.value() >= segment.len() {
                    return Err(RouteEditingError::InvalidPointIndex {
                        index: end_index.value(),
                        count: segment.len(),
                    });
                }

                // Complexity limit: reject replacements exceeding the maximum point count
                if replacement.len() > MAX_REPLACEMENT_POINTS {
                    return Err(RouteEditingError::ReplacementTooLarge {
                        maximum: MAX_REPLACEMENT_POINTS,
                        actual: replacement.len(),
                    });
                }

                // Minimum replacement size: must have at least 2 points
                if replacement.len() < 2 {
                    return Err(RouteEditingError::InvalidOperation {
                        message: "replacement must contain at least 2 points".to_string(),
                    });
                }

                // Endpoint continuity: first replacement point must match geometry at start_index
                if let Some(first) = replacement.first() {
                    let expected = &segment[start_index.value()].coordinate;
                    if first.coordinate != *expected {
                        return Err(RouteEditingError::EndpointContinuityViolation {
                            position: "start".to_string(),
                            expected: *expected,
                            actual: first.coordinate,
                        });
                    }
                }

                // Endpoint continuity: last replacement point must match geometry at end_index
                if let Some(last) = replacement.last() {
                    let expected = &segment[end_index.value()].coordinate;
                    if last.coordinate != *expected {
                        return Err(RouteEditingError::EndpointContinuityViolation {
                            position: "end".to_string(),
                            expected: *expected,
                            actual: last.coordinate,
                        });
                    }
                }

                let delete_count = end_index.value() - start_index.value() + 1;
                let new_len = segment.len() - delete_count + replacement.len();
                if new_len < 2 {
                    return Err(RouteEditingError::InsufficientPoints {
                        minimum: 2,
                        actual: new_len,
                    });
                }

                tracing::debug!(
                    segment_index = segment_index.value(),
                    start_index = start_index.value(),
                    end_index = end_index.value(),
                    replacement_point_count = replacement.len(),
                    "Replacing section in route draft"
                );

                self.geometry[segment_index.value()].splice(
                    start_index.value()..=end_index.value(),
                    replacement.iter().cloned(),
                );
                Ok(())
            }

            RouteOperation::SplitSegment {
                segment_index,
                at_point_index,
            } => {
                self.validate_point_idx(*segment_index, *at_point_index)?;
                let segment = &self.geometry[segment_index.value()];
                if at_point_index.value() == 0 {
                    return Err(RouteEditingError::InvalidOperation {
                        message: "cannot split at the first point of a segment".to_string(),
                    });
                }
                if at_point_index.value() == segment.len() - 1 {
                    return Err(RouteEditingError::InvalidOperation {
                        message: "cannot split at the last point of a segment".to_string(),
                    });
                }
                // Split point appears in both resulting segments
                let first_part = segment[..=at_point_index.value()].to_vec();
                let second_part = segment[at_point_index.value()..].to_vec();
                if first_part.len() < 2 || second_part.len() < 2 {
                    return Err(RouteEditingError::InsufficientPoints {
                        minimum: 2,
                        actual: first_part.len().min(second_part.len()),
                    });
                }
                let idx = segment_index.value();
                self.geometry.remove(idx);
                self.geometry.insert(idx, second_part);
                self.geometry.insert(idx, first_part);
                Ok(())
            }

            RouteOperation::JoinSegments {
                first_segment_index,
                second_segment_index,
            } => {
                if second_segment_index.value() != first_segment_index.value() + 1 {
                    return Err(RouteEditingError::InvalidOperation {
                        message: "segments must be adjacent to join".to_string(),
                    });
                }
                self.validate_segment_idx(*first_segment_index)?;
                self.validate_segment_idx(*second_segment_index)?;
                let second = self.geometry.remove(second_segment_index.value());
                let first = &mut self.geometry[first_segment_index.value()];
                // Deduplicate shared point (from prior split)
                if !first.is_empty() && !second.is_empty() && first.last() == second.first() {
                    first.extend(second.into_iter().skip(1));
                } else {
                    first.extend(second);
                }
                Ok(())
            }
        }
    }
}
