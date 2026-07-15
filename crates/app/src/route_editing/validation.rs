//! Publication validation for route drafts.
//!
//! Validates that a draft is ready for publication by checking ownership,
//! draft state, revision, coordinate ranges, point counts, and base version
//! existence. Collects ALL geometry errors rather than short-circuiting, so
//! the user sees all problems at once.

use crate::identity::UserId;

use super::draft::{DraftState, RouteDraft};

/// Minimum number of points required in a segment for publication.
pub const MIN_POINTS_PER_SEGMENT: usize = 2;

/// Errors that can occur during publication validation.
///
/// Authorization/precondition errors (NotOwner, DraftNotActive, RevisionMismatch)
/// short-circuit and prevent geometry validation from running.
/// Geometry errors (NoSegments, InsufficientPointsInSegment, InvalidCoordinateInSegment,
/// NoBaseVersion) are collected exhaustively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationValidationError {
    /// The requesting user is not the owner of the draft.
    NotOwner,
    /// The draft is not in Active state.
    DraftNotActive,
    /// The provided expectedRevision does not match the current draft revision.
    RevisionMismatch { expected: u64, actual: u64 },
    /// The draft has no segments in its geometry.
    NoSegments,
    /// A segment has fewer than the minimum required points.
    InsufficientPointsInSegment {
        segment_index: usize,
        minimum: usize,
        actual: usize,
    },
    /// A coordinate in a segment is outside valid range.
    InvalidCoordinateInSegment {
        segment_index: usize,
        point_index: usize,
        message: String,
    },
    /// The draft has no base route version set.
    NoBaseVersion,
}

/// Validate a draft for publication.
///
/// This is a read-only check that does NOT modify any state.
///
/// - Ownership and state checks short-circuit (return immediately) since they are
///   authorization/precondition failures that prevent geometry validation.
/// - Geometry checks collect all errors so the user sees every problem at once.
///
/// Returns `Ok(())` if the draft is valid for publication.
/// Returns `Err(errors)` if one or more validation errors are found.
pub fn validate_for_publication(
    draft: &RouteDraft,
    expected_revision: u64,
    owner: UserId,
) -> Result<(), Vec<PublicationValidationError>> {
    // Authorization/precondition checks - short-circuit on first failure
    if draft.owner_id != owner {
        return Err(vec![PublicationValidationError::NotOwner]);
    }

    if draft.state != DraftState::Active {
        return Err(vec![PublicationValidationError::DraftNotActive]);
    }

    if draft.revision != expected_revision {
        return Err(vec![PublicationValidationError::RevisionMismatch {
            expected: expected_revision,
            actual: draft.revision,
        }]);
    }

    // Geometry validation - collect all errors
    let mut errors = Vec::new();

    if draft.geometry.is_empty() {
        errors.push(PublicationValidationError::NoSegments);
    } else {
        for (seg_idx, segment) in draft.geometry.iter().enumerate() {
            if segment.len() < MIN_POINTS_PER_SEGMENT {
                errors.push(PublicationValidationError::InsufficientPointsInSegment {
                    segment_index: seg_idx,
                    minimum: MIN_POINTS_PER_SEGMENT,
                    actual: segment.len(),
                });
            }

            for (pt_idx, point) in segment.iter().enumerate() {
                let lat = point.coordinate.latitude;
                let lon = point.coordinate.longitude;

                if !(-90.0..=90.0).contains(&lat) {
                    errors.push(PublicationValidationError::InvalidCoordinateInSegment {
                        segment_index: seg_idx,
                        point_index: pt_idx,
                        message: format!("latitude must be between -90 and 90, got {lat}"),
                    });
                }

                if !(-180.0..=180.0).contains(&lon) {
                    errors.push(PublicationValidationError::InvalidCoordinateInSegment {
                        segment_index: seg_idx,
                        point_index: pt_idx,
                        message: format!("longitude must be between -180 and 180, got {lon}"),
                    });
                }
            }
        }
    }

    if draft.base_route_version_id.is_none() {
        errors.push(PublicationValidationError::NoBaseVersion);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_catalog::ActivityId;
    use crate::identity::UserId;
    use crate::route_editing::{Coordinate, RouteDraft, RoutePoint};
    use uuid::Uuid;

    fn valid_draft(owner: UserId) -> RouteDraft {
        let geometry = vec![vec![
            RoutePoint::new(Coordinate::new(47.0, 11.0).unwrap(), None),
            RoutePoint::new(Coordinate::new(47.1, 11.1).unwrap(), None),
            RoutePoint::new(Coordinate::new(47.2, 11.2).unwrap(), None),
        ]];

        let draft = RouteDraft::create_from_geometry(
            owner,
            ActivityId::new(Uuid::new_v4()),
            Some(Uuid::new_v4()),
            geometry,
        )
        .unwrap();

        // Ensure it is in known state
        assert_eq!(draft.revision, 0);
        assert_eq!(draft.state, DraftState::Active);
        draft
    }

    #[test]
    fn valid_geometry_passes() {
        let owner = UserId::new(Uuid::new_v4());
        let draft = valid_draft(owner);
        let result = validate_for_publication(&draft, 0, owner);
        assert!(result.is_ok());
    }

    #[test]
    fn wrong_owner_fails() {
        let owner = UserId::new(Uuid::new_v4());
        let other = UserId::new(Uuid::new_v4());
        let draft = valid_draft(owner);
        let result = validate_for_publication(&draft, 0, other);
        assert_eq!(result, Err(vec![PublicationValidationError::NotOwner]));
    }

    #[test]
    fn draft_not_active_fails() {
        let owner = UserId::new(Uuid::new_v4());
        let mut draft = valid_draft(owner);
        draft.publish().unwrap();
        let result = validate_for_publication(&draft, 0, owner);
        assert_eq!(
            result,
            Err(vec![PublicationValidationError::DraftNotActive])
        );
    }

    #[test]
    fn revision_mismatch_fails() {
        let owner = UserId::new(Uuid::new_v4());
        let draft = valid_draft(owner);
        let result = validate_for_publication(&draft, 99, owner);
        assert_eq!(
            result,
            Err(vec![PublicationValidationError::RevisionMismatch {
                expected: 99,
                actual: 0,
            }])
        );
    }

    #[test]
    fn empty_segments_fails() {
        let owner = UserId::new(Uuid::new_v4());
        let mut draft = valid_draft(owner);
        draft.geometry = vec![];
        let result = validate_for_publication(&draft, 0, owner);
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, PublicationValidationError::NoSegments)));
    }

    #[test]
    fn segment_with_fewer_than_two_points_fails() {
        let owner = UserId::new(Uuid::new_v4());
        let mut draft = valid_draft(owner);
        // Set segment with only 1 point
        draft.geometry = vec![vec![RoutePoint::new(
            Coordinate::new(47.0, 11.0).unwrap(),
            None,
        )]];
        let result = validate_for_publication(&draft, 0, owner);
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(
            e,
            PublicationValidationError::InsufficientPointsInSegment {
                segment_index: 0,
                minimum: 2,
                actual: 1,
            }
        )));
    }

    #[test]
    fn invalid_coordinates_detected() {
        let owner = UserId::new(Uuid::new_v4());
        let mut draft = valid_draft(owner);
        // Bypass the Coordinate constructor to inject an invalid coordinate
        draft.geometry = vec![vec![
            RoutePoint {
                coordinate: Coordinate {
                    latitude: 91.0,
                    longitude: 11.0,
                },
                elevation: None,
            },
            RoutePoint::new(Coordinate::new(47.1, 11.1).unwrap(), None),
        ]];
        let result = validate_for_publication(&draft, 0, owner);
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(
            e,
            PublicationValidationError::InvalidCoordinateInSegment {
                segment_index: 0,
                point_index: 0,
                ..
            }
        )));
    }

    #[test]
    fn no_base_version_fails() {
        let owner = UserId::new(Uuid::new_v4());
        let geometry = vec![vec![
            RoutePoint::new(Coordinate::new(47.0, 11.0).unwrap(), None),
            RoutePoint::new(Coordinate::new(47.1, 11.1).unwrap(), None),
        ]];
        let draft = RouteDraft::create_from_geometry(
            owner,
            ActivityId::new(Uuid::new_v4()),
            None, // No base version
            geometry,
        )
        .unwrap();
        let result = validate_for_publication(&draft, 0, owner);
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, PublicationValidationError::NoBaseVersion)));
    }

    #[test]
    fn multiple_errors_collected_in_single_call() {
        let owner = UserId::new(Uuid::new_v4());
        let mut draft = valid_draft(owner);
        // Set up: empty geometry AND no base version
        draft.geometry = vec![];
        draft.base_route_version_id = None;
        let result = validate_for_publication(&draft, 0, owner);
        let errors = result.unwrap_err();
        // Should have at least NoSegments and NoBaseVersion
        assert!(errors.len() >= 2);
        assert!(errors
            .iter()
            .any(|e| matches!(e, PublicationValidationError::NoSegments)));
        assert!(errors
            .iter()
            .any(|e| matches!(e, PublicationValidationError::NoBaseVersion)));
    }

    #[test]
    fn invalid_longitude_detected() {
        let owner = UserId::new(Uuid::new_v4());
        let mut draft = valid_draft(owner);
        draft.geometry = vec![vec![
            RoutePoint {
                coordinate: Coordinate {
                    latitude: 47.0,
                    longitude: 181.0,
                },
                elevation: None,
            },
            RoutePoint::new(Coordinate::new(47.1, 11.1).unwrap(), None),
        ]];
        let result = validate_for_publication(&draft, 0, owner);
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(
            e,
            PublicationValidationError::InvalidCoordinateInSegment {
                segment_index: 0,
                point_index: 0,
                ..
            }
        )));
    }

    #[test]
    fn multiple_segment_errors_collected() {
        let owner = UserId::new(Uuid::new_v4());
        let mut draft = valid_draft(owner);
        // Two segments, both with insufficient points
        draft.geometry = vec![
            vec![RoutePoint::new(Coordinate::new(47.0, 11.0).unwrap(), None)],
            vec![RoutePoint::new(Coordinate::new(48.0, 12.0).unwrap(), None)],
        ];
        let result = validate_for_publication(&draft, 0, owner);
        let errors = result.unwrap_err();
        let insufficient_count = errors
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    PublicationValidationError::InsufficientPointsInSegment { .. }
                )
            })
            .count();
        assert_eq!(insufficient_count, 2);
    }
}
