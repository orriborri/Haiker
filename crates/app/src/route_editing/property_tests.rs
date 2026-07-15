//! Property-based tests for route editing operations: SplitSegment/JoinSegments topology,
//! undo/redo sequence determinism, and mixed operation sequence invariants.

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

// ---------------------------------------------------------------------------
// Mixed Operation Sequence Property Tests
// ---------------------------------------------------------------------------

/// Enum representing a randomly chosen operation type for generation purposes.
#[derive(Debug, Clone, Copy)]
enum OpKind {
    MovePoint,
    AddPoint,
    DeletePoint,
    DeleteSection,
    ReplaceSection,
    SplitSegment,
    JoinSegments,
}

/// Derive a pseudo-random index within `[0, range)` from an f64 value.
/// Uses the fractional bits of the float to produce variation.
fn derive_index(source: f64, range: usize) -> usize {
    if range == 0 {
        return 0;
    }
    // Use the absolute value's bit representation for pseudo-random variation
    let bits = source.abs().to_bits();
    (bits as usize) % range
}

/// Generate a valid operation for the current draft geometry state.
/// Uses `rng_coord` latitude and longitude to derive varied segment and point indices
/// so operations target different parts of the geometry across test runs.
/// Returns None if no valid operation of any kind can be generated given the geometry.
fn generate_valid_operation(
    geometry: &[Vec<RoutePoint>],
    kind: OpKind,
    rng_coord: Coordinate,
) -> Option<RouteOperation> {
    match kind {
        OpKind::MovePoint => {
            let seg_idx = derive_index(rng_coord.latitude, geometry.len());
            let pt_idx = derive_index(rng_coord.longitude, geometry[seg_idx].len());
            Some(RouteOperation::MovePoint {
                segment_index: SegmentIndex::new(seg_idx),
                point_index: PointIndex::new(pt_idx),
                new_position: rng_coord,
            })
        }
        OpKind::AddPoint => {
            let seg_idx = derive_index(rng_coord.latitude, geometry.len());
            let after_idx = derive_index(rng_coord.longitude, geometry[seg_idx].len());
            Some(RouteOperation::AddPoint {
                segment_index: SegmentIndex::new(seg_idx),
                after_point_index: PointIndex::new(after_idx),
                point: RoutePoint::new(rng_coord, None),
            })
        }
        OpKind::DeletePoint => {
            // Find a segment with > 2 points, starting from a derived offset
            let start = derive_index(rng_coord.latitude, geometry.len());
            let seg_idx = (0..geometry.len())
                .map(|i| (start + i) % geometry.len())
                .find(|&i| geometry[i].len() > 2)?;
            // Delete at a varied interior index (not first or last to avoid endpoint issues)
            let interior_count = geometry[seg_idx].len() - 2; // indices 1..len-1
            let offset = derive_index(rng_coord.longitude, interior_count);
            let pt_idx = 1 + offset;
            Some(RouteOperation::DeletePoint {
                segment_index: SegmentIndex::new(seg_idx),
                point_index: PointIndex::new(pt_idx),
            })
        }
        OpKind::DeleteSection => {
            // Need a segment with > 3 points to delete a section and keep >= 2
            let start = derive_index(rng_coord.latitude, geometry.len());
            let seg_idx = (0..geometry.len())
                .map(|i| (start + i) % geometry.len())
                .find(|&i| geometry[i].len() > 3)?;
            let seg_len = geometry[seg_idx].len();
            // Deletable interior range: indices 1..=(seg_len - 2)
            // Must keep at least 2 points after deletion, so max deletable = seg_len - 2
            let max_deletable = seg_len - 2;
            // Derive start_index in interior range [1, seg_len - 2]
            let interior_start = 1 + derive_index(rng_coord.longitude, max_deletable);
            // Derive end_index >= start_index but within bounds, and ensure >= 2 pts remain
            // Points remaining = (start_index) + (seg_len - 1 - end_index)
            // We need start_index + (seg_len - 1 - end_index) >= 2
            // => end_index <= seg_len - 1 - (2 - start_index) = seg_len - 3 + start_index
            let max_end = (seg_len - 3 + interior_start).min(seg_len - 2);
            let end_range = max_end - interior_start + 1;
            let end_offset = derive_index(rng_coord.latitude + rng_coord.longitude, end_range);
            let interior_end = interior_start + end_offset;
            Some(RouteOperation::DeleteSection {
                segment_index: SegmentIndex::new(seg_idx),
                start_index: PointIndex::new(interior_start),
                end_index: PointIndex::new(interior_end),
            })
        }
        OpKind::ReplaceSection => {
            // Need a segment with >= 4 points for a multi-point range replacement
            let start = derive_index(rng_coord.latitude, geometry.len());
            let seg_idx = (0..geometry.len())
                .map(|i| (start + i) % geometry.len())
                .find(|&i| geometry[i].len() >= 4)?;
            let seg = &geometry[seg_idx];
            let seg_len = seg.len();
            // Choose start_index in [1, seg_len - 3] so there is room for end > start
            let start_range = seg_len - 3; // at least 1
            let start_idx = 1 + derive_index(rng_coord.longitude, start_range);
            // Choose end_index in [start_idx + 1, seg_len - 2] for a multi-point range
            let end_range = (seg_len - 2) - start_idx;
            let end_idx = if end_range > 0 {
                start_idx + 1 + derive_index(rng_coord.latitude + rng_coord.longitude, end_range)
            } else {
                start_idx
            };
            // Endpoint continuity: replacement[0] must match geometry[start_idx],
            // replacement[last] must match geometry[end_idx]
            let start_coord = seg[start_idx].coordinate;
            let end_coord = seg[end_idx].coordinate;
            // Build replacement with 2-4 intermediate points plus matching endpoints
            let num_intermediate = 1 + derive_index(rng_coord.latitude * rng_coord.longitude, 3);
            let mut replacement = Vec::with_capacity(num_intermediate + 2);
            replacement.push(RoutePoint::new(start_coord, None));
            for i in 0..num_intermediate {
                // Generate intermediate points by interpolating + offset from rng_coord
                let frac = (i + 1) as f64 / (num_intermediate + 1) as f64;
                let lat = start_coord.latitude + (end_coord.latitude - start_coord.latitude) * frac;
                let lon = start_coord.longitude
                    + (end_coord.longitude - start_coord.longitude) * frac
                    + rng_coord.longitude * 0.001;
                let interp_coord =
                    Coordinate::new(lat.clamp(-90.0, 90.0), lon.clamp(-180.0, 180.0)).unwrap();
                replacement.push(RoutePoint::new(interp_coord, None));
            }
            replacement.push(RoutePoint::new(end_coord, None));
            Some(RouteOperation::ReplaceSection {
                segment_index: SegmentIndex::new(seg_idx),
                start_index: PointIndex::new(start_idx),
                end_index: PointIndex::new(end_idx),
                replacement,
            })
        }
        OpKind::SplitSegment => {
            // Need a segment with >= 3 points to have a valid interior split point
            let start = derive_index(rng_coord.latitude, geometry.len());
            let seg_idx = (0..geometry.len())
                .map(|i| (start + i) % geometry.len())
                .find(|&i| geometry[i].len() >= 3)?;
            // Interior split point: index in [1, seg_len - 2]
            let interior_count = geometry[seg_idx].len() - 2;
            let split_offset = derive_index(rng_coord.longitude, interior_count);
            let at_point = 1 + split_offset;
            Some(RouteOperation::SplitSegment {
                segment_index: SegmentIndex::new(seg_idx),
                at_point_index: PointIndex::new(at_point),
            })
        }
        OpKind::JoinSegments => {
            // Need at least 2 segments
            if geometry.len() < 2 {
                return None;
            }
            // Pick an adjacent pair based on rng_coord
            let max_first = geometry.len() - 1; // first can be 0..=(len-2)
            let first_idx = derive_index(rng_coord.latitude, max_first);
            Some(RouteOperation::JoinSegments {
                first_segment_index: SegmentIndex::new(first_idx),
                second_segment_index: SegmentIndex::new(first_idx + 1),
            })
        }
    }
}

/// Try to generate any valid operation for the current geometry.
/// Tries all operation kinds in a deterministic order and returns the first valid one.
#[allow(dead_code)]
fn any_valid_operation(
    geometry: &[Vec<RoutePoint>],
    rng_coord: Coordinate,
) -> Option<RouteOperation> {
    let kinds = [
        OpKind::MovePoint,
        OpKind::AddPoint,
        OpKind::DeletePoint,
        OpKind::SplitSegment,
        OpKind::JoinSegments,
        OpKind::DeleteSection,
        OpKind::ReplaceSection,
    ];
    for kind in kinds {
        if let Some(op) = generate_valid_operation(geometry, kind, rng_coord) {
            return Some(op);
        }
    }
    None
}

/// Pick a valid operation kind based on a selector byte and the current geometry.
fn pick_operation(
    geometry: &[Vec<RoutePoint>],
    selector: u8,
    rng_coord: Coordinate,
) -> Option<RouteOperation> {
    let all_kinds = [
        OpKind::MovePoint,
        OpKind::AddPoint,
        OpKind::DeletePoint,
        OpKind::DeleteSection,
        OpKind::ReplaceSection,
        OpKind::SplitSegment,
        OpKind::JoinSegments,
    ];
    // Rotate the list using the selector to get variety
    let start = (selector as usize) % all_kinds.len();
    for i in 0..all_kinds.len() {
        let kind = all_kinds[(start + i) % all_kinds.len()];
        if let Some(op) = generate_valid_operation(geometry, kind, rng_coord) {
            return Some(op);
        }
    }
    None
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Applying N random mixed operations (all types) then undoing all N
    /// restores the original geometry exactly.
    #[test]
    fn mixed_ops_undo_all_restores_original(
        geometry in arb_geometry(),
        op_selectors in proptest::collection::vec(0u8..255, 3..=8),
        coords in proptest::collection::vec(arb_coordinate(), 8),
    ) {
        let original_geometry = geometry.clone();
        let mut draft = make_draft(geometry);
        let mut revision = 0u64;
        let mut applied_count = 0usize;

        for (i, &selector) in op_selectors.iter().enumerate() {
            let coord = coords[i % coords.len()];
            if let Some(op) = pick_operation(&draft.geometry, selector, coord) {
                draft.apply_operation(
                    OperationId::generate(),
                    op,
                    revision,
                ).unwrap();
                revision += 1;
                applied_count += 1;
            }
        }

        // Undo all applied operations
        for _ in 0..applied_count {
            draft.undo(revision).unwrap();
            revision += 1;
        }

        prop_assert_eq!(&draft.geometry, &original_geometry);
    }

    /// After any sequence of valid operations, every segment in the geometry
    /// always has >= 2 points (topology preservation invariant).
    #[test]
    fn mixed_ops_preserve_minimum_segment_length(
        geometry in arb_geometry(),
        op_selectors in proptest::collection::vec(0u8..255, 3..=10),
        coords in proptest::collection::vec(arb_coordinate(), 10),
    ) {
        let mut draft = make_draft(geometry);
        let mut revision = 0u64;

        for (i, &selector) in op_selectors.iter().enumerate() {
            let coord = coords[i % coords.len()];
            if let Some(op) = pick_operation(&draft.geometry, selector, coord) {
                draft.apply_operation(
                    OperationId::generate(),
                    op,
                    revision,
                ).unwrap();
                revision += 1;

                // Invariant check: every segment must have >= 2 points
                for (seg_idx, seg) in draft.geometry.iter().enumerate() {
                    prop_assert!(
                        seg.len() >= 2,
                        "Segment {} has only {} points after operation {}",
                        seg_idx,
                        seg.len(),
                        i,
                    );
                }
            }
        }
    }

    /// Applying random operations, then interleaving undo and redo calls,
    /// always results in geometry matching the expected position in the
    /// operation history.
    #[test]
    fn mixed_undo_redo_interleave_determinism(
        geometry in arb_geometry(),
        op_selectors in proptest::collection::vec(0u8..255, 3..=6),
        coords in proptest::collection::vec(arb_coordinate(), 6),
        undo_count_raw in 1usize..=6,
        redo_count_raw in 1usize..=6,
    ) {
        let mut draft = make_draft(geometry);
        let mut revision = 0u64;
        let mut snapshots: Vec<Vec<Vec<RoutePoint>>> = Vec::new();

        // Apply operations and record snapshots
        for (i, &selector) in op_selectors.iter().enumerate() {
            let coord = coords[i % coords.len()];
            if let Some(op) = pick_operation(&draft.geometry, selector, coord) {
                draft.apply_operation(
                    OperationId::generate(),
                    op,
                    revision,
                ).unwrap();
                revision += 1;
                snapshots.push(draft.geometry.clone());
            }
        }

        let n = snapshots.len();
        prop_assume!(n >= 2);

        let undo_count = undo_count_raw.min(n);
        let redo_count = redo_count_raw.min(undo_count);

        // Undo
        for _ in 0..undo_count {
            draft.undo(revision).unwrap();
            revision += 1;
        }

        // After undoing `undo_count` ops from n total, we are at position n - undo_count
        if n > undo_count {
            prop_assert_eq!(&draft.geometry, &snapshots[n - undo_count - 1]);
        }

        // Redo
        for j in 0..redo_count {
            draft.redo(revision).unwrap();
            revision += 1;

            let expected_idx = n - undo_count + j;
            prop_assert_eq!(
                &draft.geometry,
                &snapshots[expected_idx],
                "After redo {}, geometry did not match snapshot at index {}",
                j,
                expected_idx,
            );
        }
    }
}
