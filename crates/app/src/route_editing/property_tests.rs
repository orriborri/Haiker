//! Property-based tests for SplitSegment and JoinSegments topology operations.

use proptest::prelude::*;

use super::*;
use crate::activity_catalog::ActivityId;
use crate::identity::UserId;
use uuid::Uuid;

/// Strategy to generate a valid coordinate (lat in [-90, 90], lon in [-180, 180]).
fn arb_coordinate() -> impl Strategy<Value = Coordinate> {
    (-90.0f64..=90.0, -180.0f64..=180.0).prop_map(|(lat, lon)| Coordinate::new(lat, lon).unwrap())
}

/// Strategy to generate a valid RoutePoint with no elevation.
fn arb_route_point() -> impl Strategy<Value = RoutePoint> {
    arb_coordinate().prop_map(|c| RoutePoint::new(c, None))
}

/// Strategy to generate a valid segment (3 to 20 points).
fn arb_segment() -> impl Strategy<Value = Vec<RoutePoint>> {
    proptest::collection::vec(arb_route_point(), 3..=20)
}

/// Strategy to generate a valid geometry (1 to 3 segments, each 3-20 points).
fn arb_geometry() -> impl Strategy<Value = Vec<Vec<RoutePoint>>> {
    proptest::collection::vec(arb_segment(), 1..=3)
}

/// Helper to create a draft from geometry.
fn make_draft(geometry: Vec<Vec<RoutePoint>>) -> RouteDraft {
    RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        geometry,
    )
    .unwrap()
}

proptest! {
    /// Splitting a segment at any valid interior point preserves all original points.
    /// The union of first_part and second_part (with the shared split point counted once)
    /// must contain every point from the original segment in order.
    #[test]
    fn split_preserves_all_points(segment in arb_segment()) {
        let seg_len = segment.len();
        // Valid interior split indices: 1..seg_len-1
        let split_idx = 1; // We'll test the first valid interior point as a baseline
        // Use a parameterized split point below
        for split_at in 1..seg_len - 1 {
            let geo = vec![segment.clone()];
            let mut draft = make_draft(geo);
            draft.apply_operation(
                OperationId::generate(),
                RouteOperation::SplitSegment {
                    segment_index: SegmentIndex::new(0),
                    at_point_index: PointIndex::new(split_at),
                },
                0,
            ).unwrap();

            let first = &draft.geometry[0];
            let second = &draft.geometry[1];

            // Union without duplication of split point
            let mut combined: Vec<RoutePoint> = first.clone();
            combined.extend(second.iter().skip(1).cloned());

            prop_assert_eq!(&combined, &segment);
        }
        // Suppress unused variable warning
        let _ = split_idx;
    }

    /// Splitting at any valid interior point and then joining the resulting segments
    /// restores the original geometry exactly.
    #[test]
    fn split_then_join_roundtrip(
        segment in arb_segment(),
        split_offset in 1usize..19,
    ) {
        let seg_len = segment.len();
        // Clamp split_offset to a valid interior index
        let split_at = split_offset % (seg_len - 2) + 1;

        let geo = vec![segment.clone()];
        let mut draft = make_draft(geo);

        // Split
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(split_at),
            },
            0,
        ).unwrap();
        prop_assert_eq!(draft.geometry.len(), 2);

        // Join
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::JoinSegments {
                first_segment_index: SegmentIndex::new(0),
                second_segment_index: SegmentIndex::new(1),
            },
            1,
        ).unwrap();

        prop_assert_eq!(draft.geometry.len(), 1);
        prop_assert_eq!(&draft.geometry[0], &segment);
    }

    /// After splitting, the shared endpoint appears once in the first segment's tail
    /// and once in the second segment's head. When joined, it is deduplicated (only
    /// appears once in the resulting segment).
    #[test]
    fn join_deduplicates_shared_endpoint(
        segment in arb_segment(),
        split_offset in 1usize..19,
    ) {
        let seg_len = segment.len();
        let split_at = split_offset % (seg_len - 2) + 1;

        let geo = vec![segment.clone()];
        let mut draft = make_draft(geo);

        // Split
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(split_at),
            },
            0,
        ).unwrap();

        // Verify the shared point exists in both segments
        let shared_point = draft.geometry[0].last().unwrap().clone();
        prop_assert_eq!(&shared_point, &draft.geometry[1][0]);

        // Join
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::JoinSegments {
                first_segment_index: SegmentIndex::new(0),
                second_segment_index: SegmentIndex::new(1),
            },
            1,
        ).unwrap();

        // The joined result should have exactly the original number of points
        // (shared point was deduplicated)
        prop_assert_eq!(draft.geometry[0].len(), seg_len);
    }

    /// Splitting at the first point (index 0) always fails with InvalidOperation.
    #[test]
    fn split_rejects_boundary_first(segment in arb_segment()) {
        let geo = vec![segment.clone()];
        let mut draft = make_draft(geo);

        let result = draft.apply_operation(
            OperationId::generate(),
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(0),
            },
            0,
        );
        let is_invalid_op = matches!(result, Err(RouteEditingError::InvalidOperation { .. }));
        prop_assert!(is_invalid_op, "expected InvalidOperation, got {:?}", result);
    }

    /// Splitting at the last point always fails with InvalidOperation.
    #[test]
    fn split_rejects_boundary_last(segment in arb_segment()) {
        let last_idx = segment.len() - 1;
        let geo = vec![segment.clone()];
        let mut draft = make_draft(geo);

        let result = draft.apply_operation(
            OperationId::generate(),
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(last_idx),
            },
            0,
        );
        let is_invalid_op = matches!(result, Err(RouteEditingError::InvalidOperation { .. }));
        prop_assert!(is_invalid_op, "expected InvalidOperation, got {:?}", result);
    }

    /// Joining non-adjacent segments always fails with InvalidOperation.
    #[test]
    fn join_rejects_non_adjacent(geometry in arb_geometry()) {
        // Only test when we have 3 segments so index 0 and 2 are non-adjacent
        prop_assume!(geometry.len() >= 3);

        let mut draft = make_draft(geometry);

        let result = draft.apply_operation(
            OperationId::generate(),
            RouteOperation::JoinSegments {
                first_segment_index: SegmentIndex::new(0),
                second_segment_index: SegmentIndex::new(2),
            },
            0,
        );
        let is_invalid_op = matches!(result, Err(RouteEditingError::InvalidOperation { .. }));
        prop_assert!(is_invalid_op, "expected InvalidOperation, got {:?}", result);
    }

    /// Applying any operation with a wrong (stale) expected_revision leaves geometry unchanged.
    #[test]
    fn stale_revision_leaves_geometry_unchanged(
        segment in arb_segment(),
        split_offset in 1usize..19,
    ) {
        let seg_len = segment.len();
        let split_at = split_offset % (seg_len - 2) + 1;

        let geo = vec![segment.clone()];
        let mut draft = make_draft(geo);

        // First, apply a valid operation to advance revision to 1
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(split_at),
            },
            0,
        ).unwrap();

        let geometry_before_stale = draft.geometry.clone();

        // Now try with stale revision 0 (actual is 1)
        let result = draft.apply_operation(
            OperationId::generate(),
            RouteOperation::JoinSegments {
                first_segment_index: SegmentIndex::new(0),
                second_segment_index: SegmentIndex::new(1),
            },
            0, // stale
        );

        let is_conflict = matches!(result, Err(RouteEditingError::RevisionConflict { .. }));
        prop_assert!(is_conflict, "expected RevisionConflict, got {:?}", result);
        prop_assert_eq!(&draft.geometry, &geometry_before_stale);
    }

    /// Re-applying the same operation_id does not modify geometry or revision.
    #[test]
    fn idempotent_operation_does_not_change_state(
        segment in arb_segment(),
        split_offset in 1usize..19,
    ) {
        let seg_len = segment.len();
        let split_at = split_offset % (seg_len - 2) + 1;

        let geo = vec![segment.clone()];
        let mut draft = make_draft(geo);

        let op_id = OperationId::generate();

        // Apply the split
        draft.apply_operation(
            op_id,
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(split_at),
            },
            0,
        ).unwrap();

        let geometry_after_first = draft.geometry.clone();
        let revision_after_first = draft.revision;

        // Re-apply with same operation_id (but current revision)
        draft.apply_operation(
            op_id,
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(1),
            },
            1,
        ).unwrap();

        prop_assert_eq!(&draft.geometry, &geometry_after_first);
        prop_assert_eq!(draft.revision, revision_after_first);
    }
}
