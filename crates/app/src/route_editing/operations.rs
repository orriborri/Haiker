//! Route operations that can be applied to a draft's geometry.

use serde::{Deserialize, Serialize};

use super::value_objects::{Coordinate, PointIndex, RoutePoint, SegmentIndex};

/// An operation that can be applied to a route draft's geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteOperation {
    /// Move an existing point to a new coordinate.
    MovePoint {
        segment_index: SegmentIndex,
        point_index: PointIndex,
        new_position: Coordinate,
    },

    /// Add a new point after an existing point in a segment.
    AddPoint {
        segment_index: SegmentIndex,
        after_point_index: PointIndex,
        point: RoutePoint,
    },

    /// Delete a single point from a segment.
    DeletePoint {
        segment_index: SegmentIndex,
        point_index: PointIndex,
    },

    /// Delete a contiguous section of points from a segment.
    DeleteSection {
        segment_index: SegmentIndex,
        start_index: PointIndex,
        end_index: PointIndex,
    },

    /// Replace a contiguous section of points with new points.
    ReplaceSection {
        segment_index: SegmentIndex,
        start_index: PointIndex,
        end_index: PointIndex,
        replacement: Vec<RoutePoint>,
    },

    /// Split a segment into two at the given point (point appears in both).
    SplitSegment {
        segment_index: SegmentIndex,
        at_point_index: PointIndex,
    },

    /// Join two adjacent segments into one.
    JoinSegments {
        first_segment_index: SegmentIndex,
        second_segment_index: SegmentIndex,
    },
}
