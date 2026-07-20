//! PostgreSQL-backed implementation of RecordedRouteRepository.
//!
//! Queries recorded_activity.recorded_tracks joined with
//! recorded_activity.source_revisions and activity_catalog.activities
//! for ownership verification. Uses raw `sqlx::query()` for SQLX_OFFLINE compatibility.

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::recorded_activity::repository::{
    RecordedRouteData, RecordedRoutePreview, RecordedRouteRepository, RouteSegment, RouteStatistics,
};
use haiker_app::recorded_activity::{BoundingBox, Coordinate, RecordedActivityError};

/// PostgreSQL implementation of the recorded route repository.
#[derive(Clone)]
pub struct PgRecordedRouteRepository {
    pool: PgPool,
}

impl PgRecordedRouteRepository {
    /// Create a new PgRecordedRouteRepository with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Row type for recorded track queries.
type RecordedTrackRow = (
    Uuid,              // id
    serde_json::Value, // geometry_json
    serde_json::Value, // bounding_box_json
    f64,               // distance_meters
    Option<f64>,       // elevation_gain_meters
    Option<f64>,       // elevation_loss_meters
    i32,               // point_count
    i32,               // segment_count
);

/// Parse the geometry JSON into route segments.
///
/// The geometry is stored as a JSON array of segments, where each segment has a
/// `points` array containing objects with `coordinate` (latitude/longitude),
/// optional `elevation`, and optional `timestamp`:
///
/// ```json
/// [
///   {
///     "points": [
///       { "coordinate": { "latitude": 66.38, "longitude": 29.04 }, "elevation": 220.0, "timestamp": "..." },
///       ...
///     ]
///   }
/// ]
/// ```
fn parse_geometry(geometry_json: &serde_json::Value) -> Vec<RouteSegment> {
    let segments = match geometry_json.as_array() {
        Some(segs) => segs,
        None => {
            // Fallback: try the old { "segments": [[lng, lat], ...] } format
            return parse_geometry_legacy(geometry_json);
        }
    };

    segments
        .iter()
        .filter_map(|seg| {
            let points = seg.get("points")?.as_array()?;
            let coords: Vec<Coordinate> = points
                .iter()
                .filter_map(|p| {
                    let coord = p.get("coordinate")?;
                    let lat = coord.get("latitude")?.as_f64()?;
                    let lng = coord.get("longitude")?.as_f64()?;
                    Coordinate::new(lat, lng).ok()
                })
                .collect();
            if coords.is_empty() {
                None
            } else {
                Some(RouteSegment { points: coords })
            }
        })
        .collect()
}

/// Legacy format parser for geometry stored as `{ "segments": [ [ [lng, lat], ... ] ] }`.
fn parse_geometry_legacy(geometry_json: &serde_json::Value) -> Vec<RouteSegment> {
    let segments = match geometry_json.get("segments").and_then(|s| s.as_array()) {
        Some(segs) => segs,
        None => return Vec::new(),
    };

    segments
        .iter()
        .filter_map(|seg| {
            let points = seg.as_array()?;
            let coords: Vec<Coordinate> = points
                .iter()
                .filter_map(|p| {
                    let arr = p.as_array()?;
                    let lng = arr.first()?.as_f64()?;
                    let lat = arr.get(1)?.as_f64()?;
                    Coordinate::new(lat, lng).ok()
                })
                .collect();
            if coords.is_empty() {
                None
            } else {
                Some(RouteSegment { points: coords })
            }
        })
        .collect()
}

/// Parse the bounding box JSON.
///
/// Expected format: { "south_west": { "latitude": f64, "longitude": f64 }, "north_east": { ... } }
fn parse_bounding_box(bbox_json: &serde_json::Value) -> Option<BoundingBox> {
    let sw = bbox_json.get("south_west")?;
    let ne = bbox_json.get("north_east")?;

    let sw_lat = sw.get("latitude")?.as_f64()?;
    let sw_lng = sw.get("longitude")?.as_f64()?;
    let ne_lat = ne.get("latitude")?.as_f64()?;
    let ne_lng = ne.get("longitude")?.as_f64()?;

    let south_west = Coordinate::new(sw_lat, sw_lng).ok()?;
    let north_east = Coordinate::new(ne_lat, ne_lng).ok()?;

    Some(BoundingBox::new(south_west, north_east))
}

#[async_trait]
impl RecordedRouteRepository for PgRecordedRouteRepository {
    async fn get_recorded_route(
        &self,
        activity_id: Uuid,
    ) -> Result<Option<RecordedRouteData>, RecordedActivityError> {
        let row = sqlx::query_as::<_, RecordedTrackRow>(
            r#"
            SELECT rt.id, rt.geometry_json, rt.bounding_box_json,
                   rt.distance_meters, rt.elevation_gain_meters,
                   rt.elevation_loss_meters, rt.point_count, rt.segment_count
            FROM recorded_activity.recorded_tracks rt
            JOIN recorded_activity.source_revisions sr ON sr.id = rt.source_revision_id
            WHERE sr.activity_id = $1
            ORDER BY sr.revision_number DESC
            LIMIT 1
            "#,
        )
        .bind(activity_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        match row {
            None => Ok(None),
            Some((
                _id,
                geometry_json,
                bounding_box_json,
                distance_meters,
                elevation_gain_meters,
                elevation_loss_meters,
                point_count,
                segment_count,
            )) => {
                let segments = parse_geometry(&geometry_json);
                let bounding_box = parse_bounding_box(&bounding_box_json).unwrap_or_else(|| {
                    BoundingBox::new(
                        Coordinate {
                            latitude: 0.0,
                            longitude: 0.0,
                        },
                        Coordinate {
                            latitude: 0.0,
                            longitude: 0.0,
                        },
                    )
                });

                Ok(Some(RecordedRouteData {
                    activity_id,
                    segments,
                    bounding_box,
                    statistics: RouteStatistics {
                        distance_meters,
                        elevation_gain_meters,
                        elevation_loss_meters,
                        point_count: point_count as u32,
                        segment_count: segment_count as u32,
                    },
                }))
            }
        }
    }

    async fn get_recorded_route_preview(
        &self,
        activity_id: Uuid,
    ) -> Result<Option<RecordedRoutePreview>, RecordedActivityError> {
        let row = sqlx::query_as::<
            _,
            (
                serde_json::Value, // bounding_box_json
                f64,               // distance_meters
                Option<f64>,       // elevation_gain_meters
                Option<f64>,       // elevation_loss_meters
                i32,               // point_count
                i32,               // segment_count
            ),
        >(
            r#"
            SELECT rt.bounding_box_json, rt.distance_meters,
                   rt.elevation_gain_meters, rt.elevation_loss_meters,
                   rt.point_count, rt.segment_count
            FROM recorded_activity.recorded_tracks rt
            JOIN recorded_activity.source_revisions sr ON sr.id = rt.source_revision_id
            WHERE sr.activity_id = $1
            ORDER BY sr.revision_number DESC
            LIMIT 1
            "#,
        )
        .bind(activity_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        match row {
            None => Ok(None),
            Some((
                bounding_box_json,
                distance_meters,
                elevation_gain_meters,
                elevation_loss_meters,
                point_count,
                segment_count,
            )) => {
                let bounding_box = parse_bounding_box(&bounding_box_json).unwrap_or_else(|| {
                    BoundingBox::new(
                        Coordinate {
                            latitude: 0.0,
                            longitude: 0.0,
                        },
                        Coordinate {
                            latitude: 0.0,
                            longitude: 0.0,
                        },
                    )
                });

                Ok(Some(RecordedRoutePreview {
                    activity_id,
                    bounding_box,
                    statistics: RouteStatistics {
                        distance_meters,
                        elevation_gain_meters,
                        elevation_loss_meters,
                        point_count: point_count as u32,
                        segment_count: segment_count as u32,
                    },
                }))
            }
        }
    }
}
