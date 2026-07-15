//! Property-based tests for route editing operations: SplitSegment/JoinSegments topology
//! and undo/redo sequence determinism.

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
    fn split_preserves_all_points(
        segment in arb_segment(),
        split_offset in 1usize..19,
    ) {
        let seg_len = segment.len();
        let split_at = split_offset % (seg_len - 2) + 1;

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

// ---------------------------------------------------------------------------
// Undo/Redo Sequence Determinism Property Tests
// ---------------------------------------------------------------------------

proptest! {
    /// Applying 1-10 random MovePoint operations then undoing all of them
    /// restores the original geometry exactly.
    #[test]
    fn undo_all_restores_original_geometry(
        geometry in arb_geometry(),
        op_count in 1usize..=10,
        // Generate max operations worth of indices and coordinates
        seg_indices in proptest::collection::vec(0usize..3, 10),
        pt_indices in proptest::collection::vec(0usize..20, 10),
        new_positions in proptest::collection::vec(arb_coordinate(), 10),
    ) {
        let original_geometry = geometry.clone();
        let mut draft = make_draft(geometry);

        let seg_count = draft.geometry.len();
        let mut revision = 0u64;

        // Apply op_count MovePoint operations with valid indices
        for i in 0..op_count {
            let seg_idx = seg_indices[i] % seg_count;
            let pt_count = draft.geometry[seg_idx].len();
            let pt_idx = pt_indices[i] % pt_count;

            draft.apply_operation(
                OperationId::generate(),
                RouteOperation::MovePoint {
                    segment_index: SegmentIndex::new(seg_idx),
                    point_index: PointIndex::new(pt_idx),
                    new_position: new_positions[i],
                },
                revision,
            ).unwrap();
            revision += 1;
        }

        // Undo all operations one by one
        for _ in 0..op_count {
            draft.undo(revision).unwrap();
            revision += 1;
        }

        prop_assert_eq!(&draft.geometry, &original_geometry);
    }

    /// Apply N operations, undo K of them, redo some of those K,
    /// and verify each redo restores the exact geometry that existed
    /// after the original apply.
    #[test]
    fn mixed_undo_redo_is_deterministic(
        geometry in arb_geometry(),
        n in 2usize..=8,
        k in 1usize..=8,
        redo_count in 1usize..=8,
        seg_indices in proptest::collection::vec(0usize..3, 8),
        pt_indices in proptest::collection::vec(0usize..20, 8),
        new_positions in proptest::collection::vec(arb_coordinate(), 8),
    ) {
        // Ensure k <= n and redo_count <= k for valid undo/redo depths
        prop_assume!(k <= n);
        prop_assume!(redo_count <= k);

        let mut draft = make_draft(geometry);
        let seg_count = draft.geometry.len();
        let mut revision = 0u64;

        // Record geometry after each operation
        let mut geometry_snapshots: Vec<Vec<Vec<RoutePoint>>> = Vec::new();

        // Apply n operations
        for i in 0..n {
            let seg_idx = seg_indices[i] % seg_count;
            let pt_count = draft.geometry[seg_idx].len();
            let pt_idx = pt_indices[i] % pt_count;

            draft.apply_operation(
                OperationId::generate(),
                RouteOperation::MovePoint {
                    segment_index: SegmentIndex::new(seg_idx),
                    point_index: PointIndex::new(pt_idx),
                    new_position: new_positions[i],
                },
                revision,
            ).unwrap();
            revision += 1;
            geometry_snapshots.push(draft.geometry.clone());
        }

        // Undo k of them
        for _ in 0..k {
            draft.undo(revision).unwrap();
            revision += 1;
        }

        // Redo redo_count of those k
        for j in 0..redo_count {
            draft.redo(revision).unwrap();
            revision += 1;

            // After redo j, geometry should match snapshot at index (n - k + j)
            let expected_idx = n - k + j;
            prop_assert_eq!(
                &draft.geometry,
                &geometry_snapshots[expected_idx],
                "Redo {} did not restore expected geometry (snapshot index {})",
                j,
                expected_idx,
            );
        }
    }

    /// Topology-changing operations (SplitSegment, JoinSegments) are correctly
    /// reversed and replayed through undo/redo, preserving exact geometry including
    /// segment count changes.
    #[test]
    fn topology_operations_undo_redo_determinism(
        segment in proptest::collection::vec(arb_route_point(), 5..=15),
        split_offset in 1usize..14,
        move_positions in proptest::collection::vec(arb_coordinate(), 3),
    ) {
        let seg_len = segment.len();
        // Clamp split_at to a valid interior index (not first, not last)
        let split_at = split_offset % (seg_len - 2) + 1;

        let original_geometry = vec![segment];
        let mut draft = make_draft(original_geometry.clone());
        let mut revision = 0u64;

        // Step 1: Move a point (pre-split)
        let pt_idx = 0;
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::MovePoint {
                segment_index: SegmentIndex::new(0),
                point_index: PointIndex::new(pt_idx),
                new_position: move_positions[0],
            },
            revision,
        ).unwrap();
        revision += 1;
        let geometry_after_move = draft.geometry.clone();

        // Step 2: Split the segment (topology change: 1 segment -> 2 segments)
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(0),
                at_point_index: PointIndex::new(split_at),
            },
            revision,
        ).unwrap();
        revision += 1;
        let geometry_after_split = draft.geometry.clone();
        prop_assert_eq!(geometry_after_split.len(), 2, "split should produce 2 segments");

        // Step 3: Move a point in the second segment (post-split)
        let second_seg_len = draft.geometry[1].len();
        let pt_in_second = if second_seg_len > 1 { 1 } else { 0 };
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::MovePoint {
                segment_index: SegmentIndex::new(1),
                point_index: PointIndex::new(pt_in_second),
                new_position: move_positions[1],
            },
            revision,
        ).unwrap();
        revision += 1;
        let geometry_after_second_move = draft.geometry.clone();

        // Step 4: Join segments back (topology change: 2 segments -> 1 segment)
        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::JoinSegments {
                first_segment_index: SegmentIndex::new(0),
                second_segment_index: SegmentIndex::new(1),
            },
            revision,
        ).unwrap();
        revision += 1;
        let geometry_after_join = draft.geometry.clone();
        prop_assert_eq!(geometry_after_join.len(), 1, "join should produce 1 segment");

        // Now undo all 4 operations and verify geometry at each step
        // Undo join -> back to 2 segments
        draft.undo(revision).unwrap();
        revision += 1;
        prop_assert_eq!(&draft.geometry, &geometry_after_second_move);

        // Undo second move -> back to post-split geometry
        draft.undo(revision).unwrap();
        revision += 1;
        prop_assert_eq!(&draft.geometry, &geometry_after_split);

        // Undo split -> back to 1 segment
        draft.undo(revision).unwrap();
        revision += 1;
        prop_assert_eq!(&draft.geometry, &geometry_after_move);
        prop_assert_eq!(draft.geometry.len(), 1, "undo split should restore 1 segment");

        // Undo first move -> back to original
        draft.undo(revision).unwrap();
        revision += 1;
        prop_assert_eq!(&draft.geometry, &original_geometry);

        // Redo all 4 and verify determinism at each step
        draft.redo(revision).unwrap();
        revision += 1;
        prop_assert_eq!(&draft.geometry, &geometry_after_move);

        draft.redo(revision).unwrap();
        revision += 1;
        prop_assert_eq!(&draft.geometry, &geometry_after_split);

        draft.redo(revision).unwrap();
        revision += 1;
        prop_assert_eq!(&draft.geometry, &geometry_after_second_move);

        draft.redo(revision).unwrap();
        // revision += 1; // not needed after last operation
        prop_assert_eq!(&draft.geometry, &geometry_after_join);
    }

    /// After undoing operations, applying a new operation clears the redo stack.
    /// Subsequent redo attempts return NothingToRedo.
    #[test]
    fn new_operation_after_undo_clears_redo_stack(
        geometry in arb_geometry(),
        op_count in 2usize..=6,
        undo_count_ratio in 1usize..=100,
        seg_indices in proptest::collection::vec(0usize..3, 7),
        pt_indices in proptest::collection::vec(0usize..20, 7),
        new_positions in proptest::collection::vec(arb_coordinate(), 7),
    ) {
        let mut draft = make_draft(geometry);
        let seg_count = draft.geometry.len();
        let mut revision = 0u64;

        // Apply op_count operations
        for i in 0..op_count {
            let seg_idx = seg_indices[i] % seg_count;
            let pt_count = draft.geometry[seg_idx].len();
            let pt_idx = pt_indices[i] % pt_count;

            draft.apply_operation(
                OperationId::generate(),
                RouteOperation::MovePoint {
                    segment_index: SegmentIndex::new(seg_idx),
                    point_index: PointIndex::new(pt_idx),
                    new_position: new_positions[i],
                },
                revision,
            ).unwrap();
            revision += 1;
        }

        // Undo some operations (at least 1)
        let undo_count = (undo_count_ratio * op_count / 100).clamp(1, op_count);
        for _ in 0..undo_count {
            draft.undo(revision).unwrap();
            revision += 1;
        }

        // Apply a new operation (uses the last available position/index)
        let new_seg_idx = seg_indices[op_count] % seg_count;
        let new_pt_count = draft.geometry[new_seg_idx].len();
        let new_pt_idx = pt_indices[op_count] % new_pt_count;

        draft.apply_operation(
            OperationId::generate(),
            RouteOperation::MovePoint {
                segment_index: SegmentIndex::new(new_seg_idx),
                point_index: PointIndex::new(new_pt_idx),
                new_position: new_positions[op_count],
            },
            revision,
        ).unwrap();
        revision += 1;

        // Redo should now fail with NothingToRedo
        let result = draft.redo(revision);
        let is_nothing_to_redo = matches!(result, Err(RouteEditingError::NothingToRedo));
        prop_assert!(
            is_nothing_to_redo,
            "expected NothingToRedo, got {:?}",
            result,
        );
    }
}
