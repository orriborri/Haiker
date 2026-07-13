//! Comprehensive tests for the route editing bounded context.
use super::*;
use crate::activity_catalog::ActivityId;
use crate::identity::UserId;
use uuid::Uuid;

fn coord(lat: f64, lon: f64) -> Coordinate {
    Coordinate::new(lat, lon).unwrap()
}
fn pt(lat: f64, lon: f64) -> RoutePoint {
    RoutePoint::new(coord(lat, lon), None)
}

fn sample_geo() -> Vec<Vec<RoutePoint>> {
    vec![vec![
        pt(45.0, 10.0),
        pt(45.1, 10.1),
        pt(45.2, 10.2),
        pt(45.3, 10.3),
    ]]
}

fn two_seg_geo() -> Vec<Vec<RoutePoint>> {
    vec![
        vec![pt(45.0, 10.0), pt(45.1, 10.1), pt(45.2, 10.2)],
        vec![pt(46.0, 11.0), pt(46.1, 11.1), pt(46.2, 11.2)],
    ]
}

fn draft() -> RouteDraft {
    RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        sample_geo(),
    )
    .unwrap()
}

fn draft2() -> RouteDraft {
    RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        two_seg_geo(),
    )
    .unwrap()
}

fn op_id() -> OperationId {
    OperationId::generate()
}

// --- Coordinate validation ---

#[test]
fn coordinate_valid() {
    assert!(Coordinate::new(0.0, 0.0).is_ok());
    assert!(Coordinate::new(90.0, 180.0).is_ok());
    assert!(Coordinate::new(-90.0, -180.0).is_ok());
}

#[test]
fn coordinate_rejects_invalid_lat() {
    assert!(matches!(
        Coordinate::new(90.1, 0.0),
        Err(RouteEditingError::InvalidCoordinate { .. })
    ));
    assert!(matches!(
        Coordinate::new(-90.1, 0.0),
        Err(RouteEditingError::InvalidCoordinate { .. })
    ));
}

#[test]
fn coordinate_rejects_invalid_lon() {
    assert!(matches!(
        Coordinate::new(0.0, 180.1),
        Err(RouteEditingError::InvalidCoordinate { .. })
    ));
    assert!(matches!(
        Coordinate::new(0.0, -180.1),
        Err(RouteEditingError::InvalidCoordinate { .. })
    ));
}

// --- Draft creation ---

#[test]
fn create_draft_valid() {
    let d = draft();
    assert_eq!(d.revision, 0);
    assert_eq!(d.state, DraftState::Active);
    assert_eq!(d.geometry.len(), 1);
    assert_eq!(d.geometry[0].len(), 4);
}

#[test]
fn create_draft_rejects_empty_geometry() {
    let r = RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        vec![],
    );
    assert!(matches!(r, Err(RouteEditingError::InvalidOperation { .. })));
}

#[test]
fn create_draft_rejects_insufficient_points() {
    let r = RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        vec![vec![pt(1.0, 1.0)]],
    );
    assert!(matches!(
        r,
        Err(RouteEditingError::InsufficientPoints {
            minimum: 2,
            actual: 1
        })
    ));
}

// --- MovePoint ---

#[test]
fn move_point_success() {
    let mut d = draft();
    let new_pos = coord(50.0, 15.0);
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(1),
            new_position: new_pos,
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0][1].coordinate, new_pos);
    assert_eq!(d.revision, 1);
}

#[test]
fn move_point_invalid_segment() {
    let mut d = draft();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(5),
            point_index: PointIndex::new(0),
            new_position: coord(1.0, 1.0),
        },
        0,
    );
    assert!(matches!(
        r,
        Err(RouteEditingError::InvalidSegmentIndex { .. })
    ));
}

#[test]
fn move_point_invalid_point_index() {
    let mut d = draft();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(99),
            new_position: coord(1.0, 1.0),
        },
        0,
    );
    assert!(matches!(
        r,
        Err(RouteEditingError::InvalidPointIndex { .. })
    ));
}

// --- AddPoint ---

#[test]
fn add_point_success() {
    let mut d = draft();
    let new_pt = pt(45.05, 10.05);
    d.apply_operation(
        op_id(),
        RouteOperation::AddPoint {
            segment_index: SegmentIndex::new(0),
            after_point_index: PointIndex::new(0),
            point: new_pt.clone(),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 5);
    assert_eq!(d.geometry[0][1], new_pt);
}

// --- DeletePoint ---

#[test]
fn delete_point_success() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::DeletePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(1),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 3);
}

#[test]
fn delete_point_enforces_minimum() {
    let mut d = RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        vec![vec![pt(1.0, 1.0), pt(2.0, 2.0)]],
    )
    .unwrap();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::DeletePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
        },
        0,
    );
    assert!(matches!(
        r,
        Err(RouteEditingError::InsufficientPoints { .. })
    ));
}

// --- DeleteSection ---

#[test]
fn delete_section_success() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::DeleteSection {
            segment_index: SegmentIndex::new(0),
            start_index: PointIndex::new(1),
            end_index: PointIndex::new(2),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 2);
}

#[test]
fn delete_section_enforces_minimum() {
    let mut d = draft();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::DeleteSection {
            segment_index: SegmentIndex::new(0),
            start_index: PointIndex::new(0),
            end_index: PointIndex::new(2),
        },
        0,
    );
    assert!(matches!(
        r,
        Err(RouteEditingError::InsufficientPoints { .. })
    ));
}

// --- ReplaceSection ---

#[test]
fn replace_section_success() {
    let mut d = draft();
    let replacement = vec![pt(50.0, 15.0), pt(50.1, 15.1)];
    d.apply_operation(
        op_id(),
        RouteOperation::ReplaceSection {
            segment_index: SegmentIndex::new(0),
            start_index: PointIndex::new(1),
            end_index: PointIndex::new(2),
            replacement: replacement.clone(),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 4);
    assert_eq!(d.geometry[0][1], replacement[0]);
    assert_eq!(d.geometry[0][2], replacement[1]);
}

// --- SplitSegment ---

#[test]
fn split_segment_success() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::SplitSegment {
            segment_index: SegmentIndex::new(0),
            at_point_index: PointIndex::new(2),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry.len(), 2);
    assert_eq!(d.geometry[0].len(), 3);
    assert_eq!(d.geometry[1].len(), 2);
    assert_eq!(d.geometry[0][2], d.geometry[1][0]);
}

#[test]
fn split_segment_rejects_at_first() {
    let mut d = draft();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::SplitSegment {
            segment_index: SegmentIndex::new(0),
            at_point_index: PointIndex::new(0),
        },
        0,
    );
    assert!(matches!(r, Err(RouteEditingError::InvalidOperation { .. })));
}

#[test]
fn split_segment_rejects_at_last() {
    let mut d = draft();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::SplitSegment {
            segment_index: SegmentIndex::new(0),
            at_point_index: PointIndex::new(3),
        },
        0,
    );
    assert!(matches!(r, Err(RouteEditingError::InvalidOperation { .. })));
}

// --- JoinSegments ---

#[test]
fn join_segments_success() {
    let mut d = draft2();
    d.apply_operation(
        op_id(),
        RouteOperation::JoinSegments {
            first_segment_index: SegmentIndex::new(0),
            second_segment_index: SegmentIndex::new(1),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry.len(), 1);
    assert_eq!(d.geometry[0].len(), 6);
}

#[test]
fn join_segments_deduplicates_shared_point() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::SplitSegment {
            segment_index: SegmentIndex::new(0),
            at_point_index: PointIndex::new(2),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry.len(), 2);
    d.apply_operation(
        op_id(),
        RouteOperation::JoinSegments {
            first_segment_index: SegmentIndex::new(0),
            second_segment_index: SegmentIndex::new(1),
        },
        1,
    )
    .unwrap();
    assert_eq!(d.geometry.len(), 1);
    assert_eq!(d.geometry[0].len(), 4); // original size restored
}

#[test]
fn join_segments_rejects_non_adjacent() {
    let mut d = RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        vec![
            vec![pt(1.0, 1.0), pt(2.0, 2.0)],
            vec![pt(3.0, 3.0), pt(4.0, 4.0)],
            vec![pt(5.0, 5.0), pt(6.0, 6.0)],
        ],
    )
    .unwrap();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::JoinSegments {
            first_segment_index: SegmentIndex::new(0),
            second_segment_index: SegmentIndex::new(2),
        },
        0,
    );
    assert!(matches!(r, Err(RouteEditingError::InvalidOperation { .. })));
}

// --- Revision conflict ---

#[test]
fn revision_conflict_on_stale_expected() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(50.0, 15.0),
        },
        0,
    )
    .unwrap();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(51.0, 16.0),
        },
        0,
    );
    assert!(matches!(
        r,
        Err(RouteEditingError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    ));
}

// --- Idempotency ---

#[test]
fn idempotent_operation_is_noop() {
    let mut d = draft();
    let id = op_id();
    d.apply_operation(
        id,
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(50.0, 15.0),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.revision, 1);
    // Same operation_id again with current revision - should be idempotent no-op
    d.apply_operation(
        id,
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(55.0, 20.0),
        },
        1,
    )
    .unwrap();
    assert_eq!(d.revision, 1);
    assert_eq!(d.geometry[0][0].coordinate, coord(50.0, 15.0));
}

// --- State guards ---

#[test]
fn cannot_edit_published_draft() {
    let mut d = draft();
    d.publish().unwrap();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(1.0, 1.0),
        },
        0,
    );
    assert!(matches!(r, Err(RouteEditingError::DraftNotActive)));
}

#[test]
fn cannot_edit_discarded_draft() {
    let mut d = draft();
    d.discard().unwrap();
    let r = d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(1.0, 1.0),
        },
        0,
    );
    assert!(matches!(r, Err(RouteEditingError::DraftNotActive)));
}

#[test]
fn cannot_undo_published_draft() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(50.0, 15.0),
        },
        0,
    )
    .unwrap();
    d.publish().unwrap();
    assert!(matches!(d.undo(1), Err(RouteEditingError::DraftNotActive)));
}

// --- Undo/Redo ---

#[test]
fn undo_move_point() {
    let mut d = draft();
    let original = d.geometry[0][1].coordinate;
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(1),
            new_position: coord(50.0, 15.0),
        },
        0,
    )
    .unwrap();
    assert_ne!(d.geometry[0][1].coordinate, original);
    d.undo(1).unwrap();
    assert_eq!(d.geometry[0][1].coordinate, original);
    assert_eq!(d.revision, 2);
}

#[test]
fn undo_add_point() {
    let mut d = draft();
    let orig_len = d.geometry[0].len();
    d.apply_operation(
        op_id(),
        RouteOperation::AddPoint {
            segment_index: SegmentIndex::new(0),
            after_point_index: PointIndex::new(1),
            point: pt(45.15, 10.15),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), orig_len + 1);
    d.undo(1).unwrap();
    assert_eq!(d.geometry[0].len(), orig_len);
}

#[test]
fn undo_delete_point() {
    let mut d = draft();
    let orig_geo = d.geometry.clone();
    d.apply_operation(
        op_id(),
        RouteOperation::DeletePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(2),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 3);
    d.undo(1).unwrap();
    assert_eq!(d.geometry, orig_geo);
}

#[test]
fn undo_split_segment() {
    let mut d = draft();
    let orig_geo = d.geometry.clone();
    d.apply_operation(
        op_id(),
        RouteOperation::SplitSegment {
            segment_index: SegmentIndex::new(0),
            at_point_index: PointIndex::new(2),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry.len(), 2);
    d.undo(1).unwrap();
    assert_eq!(d.geometry.len(), 1);
    assert_eq!(d.geometry, orig_geo);
}

#[test]
fn undo_join_segments() {
    let mut d = draft2();
    let orig_geo = d.geometry.clone();
    d.apply_operation(
        op_id(),
        RouteOperation::JoinSegments {
            first_segment_index: SegmentIndex::new(0),
            second_segment_index: SegmentIndex::new(1),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry.len(), 1);
    d.undo(1).unwrap();
    assert_eq!(d.geometry.len(), 2);
    assert_eq!(d.geometry, orig_geo);
}

#[test]
fn redo_after_undo() {
    let mut d = draft();
    let new_pos = coord(50.0, 15.0);
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(1),
            new_position: new_pos,
        },
        0,
    )
    .unwrap();
    d.undo(1).unwrap();
    assert_ne!(d.geometry[0][1].coordinate, new_pos);
    d.redo(2).unwrap();
    assert_eq!(d.geometry[0][1].coordinate, new_pos);
    assert_eq!(d.revision, 3);
}

#[test]
fn redo_stack_cleared_on_new_operation() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(50.0, 15.0),
        },
        0,
    )
    .unwrap();
    d.undo(1).unwrap();
    assert!(!d.undone_operations.is_empty());
    // New operation should clear redo stack
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(51.0, 16.0),
        },
        2,
    )
    .unwrap();
    assert!(d.undone_operations.is_empty());
    assert!(matches!(d.redo(3), Err(RouteEditingError::NothingToRedo)));
}

#[test]
fn nothing_to_undo() {
    let mut d = draft();
    assert!(matches!(d.undo(0), Err(RouteEditingError::NothingToUndo)));
}

#[test]
fn nothing_to_redo() {
    let mut d = draft();
    assert!(matches!(d.redo(0), Err(RouteEditingError::NothingToRedo)));
}

#[test]
fn undo_redo_determinism_multi_op() {
    let mut d = draft();
    // Apply 3 operations
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(50.0, 15.0),
        },
        0,
    )
    .unwrap();
    d.apply_operation(
        op_id(),
        RouteOperation::AddPoint {
            segment_index: SegmentIndex::new(0),
            after_point_index: PointIndex::new(0),
            point: pt(49.0, 14.0),
        },
        1,
    )
    .unwrap();
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(2),
            new_position: coord(60.0, 20.0),
        },
        2,
    )
    .unwrap();
    // Undo 2
    d.undo(3).unwrap();
    d.undo(4).unwrap();
    // Redo 1
    d.redo(5).unwrap();
    // Now we should have: op1 applied, op2 re-applied, op3 still undone
    assert_eq!(d.geometry[0].len(), 5); // original 4 + 1 added
    assert_eq!(d.revision, 6);
}

// --- Reset ---

#[test]
fn reset_restores_geometry() {
    let mut d = draft();
    d.apply_operation(
        op_id(),
        RouteOperation::MovePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
            new_position: coord(50.0, 15.0),
        },
        0,
    )
    .unwrap();
    let new_geo = vec![vec![pt(1.0, 1.0), pt(2.0, 2.0), pt(3.0, 3.0)]];
    d.reset(1, new_geo.clone()).unwrap();
    assert_eq!(d.geometry, new_geo);
    assert!(d.applied_operations.is_empty());
    assert!(d.undone_operations.is_empty());
    assert_eq!(d.revision, 2);
}

#[test]
fn reset_rejects_invalid_geometry() {
    let mut d = draft();
    let r = d.reset(0, vec![]);
    assert!(matches!(r, Err(RouteEditingError::InvalidOperation { .. })));
}

// --- Error display ---

#[test]
fn error_display() {
    assert_eq!(
        RouteEditingError::DraftNotFound.to_string(),
        "draft not found"
    );
    assert_eq!(
        RouteEditingError::RevisionConflict {
            expected: 1,
            actual: 2
        }
        .to_string(),
        "revision conflict: expected 1, got 2"
    );
    assert_eq!(
        RouteEditingError::DraftNotActive.to_string(),
        "draft is not active"
    );
    assert_eq!(
        RouteEditingError::NothingToUndo.to_string(),
        "nothing to undo"
    );
    assert_eq!(
        RouteEditingError::NothingToRedo.to_string(),
        "nothing to redo"
    );
    assert_eq!(
        RouteEditingError::InsufficientPoints {
            minimum: 2,
            actual: 1
        }
        .to_string(),
        "insufficient points: minimum 2, got 1"
    );
    assert_eq!(
        RouteEditingError::InvalidSegmentIndex { index: 5, count: 2 }.to_string(),
        "invalid segment index: 5, segment count: 2"
    );
    assert_eq!(
        RouteEditingError::InvalidPointIndex {
            index: 10,
            count: 4
        }
        .to_string(),
        "invalid point index: 10, point count: 4"
    );
}

// --- RoutePoint with elevation ---

#[test]
fn route_point_with_elevation() {
    let p = RoutePoint::new(coord(45.0, 10.0), Some(Elevation::new(500.0)));
    assert_eq!(p.elevation.unwrap().meters(), 500.0);
}

#[test]
fn route_point_without_elevation() {
    let p = pt(45.0, 10.0);
    assert!(p.elevation.is_none());
}

// --- OperationId ---

#[test]
fn operation_id_display() {
    let id = Uuid::new_v4();
    let op = OperationId::new(id);
    assert_eq!(op.to_string(), id.to_string());
}

// --- RouteDraftId ---

#[test]
fn route_draft_id_display() {
    let id = Uuid::new_v4();
    let draft_id = RouteDraftId::new(id);
    assert_eq!(draft_id.to_string(), id.to_string());
}

// --- Undo delete point at index 0 ---

#[test]
fn undo_delete_point_at_index_0() {
    let mut d = draft();
    let orig_geo = d.geometry.clone();
    d.apply_operation(
        op_id(),
        RouteOperation::DeletePoint {
            segment_index: SegmentIndex::new(0),
            point_index: PointIndex::new(0),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 3);
    d.undo(1).unwrap();
    assert_eq!(d.geometry, orig_geo);
}

// --- Undo delete section ---

#[test]
fn undo_delete_section() {
    let mut d = draft();
    let orig_geo = d.geometry.clone();
    d.apply_operation(
        op_id(),
        RouteOperation::DeleteSection {
            segment_index: SegmentIndex::new(0),
            start_index: PointIndex::new(1),
            end_index: PointIndex::new(2),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 2);
    d.undo(1).unwrap();
    assert_eq!(d.geometry, orig_geo);
}

// --- Undo replace section ---

#[test]
fn undo_replace_section() {
    let mut d = draft();
    let orig_geo = d.geometry.clone();
    d.apply_operation(
        op_id(),
        RouteOperation::ReplaceSection {
            segment_index: SegmentIndex::new(0),
            start_index: PointIndex::new(1),
            end_index: PointIndex::new(2),
            replacement: vec![pt(50.0, 15.0), pt(50.1, 15.1), pt(50.2, 15.2)],
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 5); // removed 2, added 3
    d.undo(1).unwrap();
    assert_eq!(d.geometry, orig_geo);
}

// --- Undo delete section at start ---

#[test]
fn undo_delete_section_at_start() {
    let geo = vec![vec![
        pt(1.0, 1.0),
        pt(2.0, 2.0),
        pt(3.0, 3.0),
        pt(4.0, 4.0),
        pt(5.0, 5.0),
    ]];
    let mut d = RouteDraft::create_from_geometry(
        UserId::new(Uuid::new_v4()),
        ActivityId::generate(),
        None,
        geo.clone(),
    )
    .unwrap();
    d.apply_operation(
        op_id(),
        RouteOperation::DeleteSection {
            segment_index: SegmentIndex::new(0),
            start_index: PointIndex::new(0),
            end_index: PointIndex::new(1),
        },
        0,
    )
    .unwrap();
    assert_eq!(d.geometry[0].len(), 3);
    d.undo(1).unwrap();
    assert_eq!(d.geometry, geo);
}
