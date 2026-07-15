//! Track normalization: converts raw parsed GPX data into domain-validated
//! recorded track structures with computed statistics.
//!
//! Responsibilities:
//! - Construct validated Coordinate values from raw lat/lon
//! - Preserve track and segment boundaries
//! - Calculate RecordedStatistics (distance, elevation gain/loss, duration)
//! - Calculate BoundingBox from all points
//! - Generate a preview geometry (simplified polyline for map display)

use chrono::{DateTime, Utc};

use crate::imports::gpx_parser::{GpxParseResult, GpxTrackPoint};

use super::{
    BoundingBox, Coordinate, Elevation, RecordedActivityError, RecordedStatistics, RecordedTrack,
    TrackPoint, TrackSegment,
};

/// Earth radius in meters for Haversine calculations.
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

/// Maximum number of points in the preview geometry.
const MAX_PREVIEW_POINTS: usize = 1000;

/// Result of normalizing parsed GPX data.
#[derive(Debug, Clone)]
pub struct NormalizedTrack {
    /// The fully constructed recorded track.
    pub recorded_track: RecordedTrack,
    /// A simplified polyline for map display (max 1000 points).
    pub preview_geometry: Vec<Coordinate>,
    /// The activity title derived from GPX metadata or track name.
    pub suggested_title: Option<String>,
    /// The earliest timestamp found in the track.
    pub started_at: Option<DateTime<Utc>>,
    /// The latest timestamp found in the track.
    pub ended_at: Option<DateTime<Utc>>,
}

/// Calculate the Haversine distance between two coordinates in meters.
///
/// Uses the standard formula with Earth radius 6371000 meters.
pub fn haversine_distance(from: &Coordinate, to: &Coordinate) -> f64 {
    let lat1 = from.latitude.to_radians();
    let lat2 = to.latitude.to_radians();
    let dlat = (to.latitude - from.latitude).to_radians();
    let dlon = (to.longitude - from.longitude).to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    EARTH_RADIUS_METERS * c
}

/// Calculate elevation gain and loss from a sequence of elevations.
///
/// Applies a dead-band threshold of 2.0 meters to filter GPS noise.
/// Only elevation differences with absolute value >= 2.0m are counted
/// as gain or loss. This prevents GPS barometric altitude noise from
/// inflating reported elevation statistics.
pub fn calculate_elevation_stats(elevations: &[Option<Elevation>]) -> (Option<f64>, Option<f64>) {
    /// Minimum elevation change (meters) to count as gain or loss.
    /// Filters out GPS noise which typically fluctuates +-2-5m per sample.
    const ELEVATION_DEAD_BAND: f64 = 2.0;

    let mut gain: f64 = 0.0;
    let mut loss: f64 = 0.0;
    let mut has_elevation = false;
    let mut prev_elevation: Option<f64> = None;

    for e in elevations.iter().flatten() {
        has_elevation = true;
        if let Some(prev) = prev_elevation {
            let diff = e.meters() - prev;
            if diff >= ELEVATION_DEAD_BAND {
                gain += diff;
            } else if diff <= -ELEVATION_DEAD_BAND {
                loss += diff.abs();
            }
        }
        prev_elevation = Some(e.meters());
    }

    if has_elevation {
        (Some(gain), Some(loss))
    } else {
        (None, None)
    }
}

/// Parse an ISO 8601 / RFC 3339 timestamp string into a DateTime<Utc>.
fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    s.parse::<DateTime<Utc>>().ok()
}

/// Convert a raw GpxTrackPoint into a domain TrackPoint.
fn gpx_point_to_track_point(pt: &GpxTrackPoint) -> Result<TrackPoint, RecordedActivityError> {
    let coordinate = Coordinate::new(pt.lat, pt.lon)?;
    let elevation = pt.elevation.map(Elevation::new);
    let timestamp = pt.time.as_deref().and_then(parse_timestamp);

    Ok(TrackPoint::new(coordinate, elevation, timestamp))
}

/// Normalize a parsed GPX result into a fully validated RecordedTrack with statistics.
///
/// This function:
/// 1. Converts raw GPS points into validated domain TrackPoints
/// 2. Preserves original segment boundaries
/// 3. Computes total distance using the Haversine formula
/// 4. Computes elevation gain/loss from sequential point comparisons
/// 5. Computes duration from earliest to latest timestamp
/// 6. Computes bounding box from all points
/// 7. Generates a simplified preview geometry
pub fn normalize_gpx(
    parse_result: &GpxParseResult,
) -> Result<NormalizedTrack, RecordedActivityError> {
    let mut all_segments: Vec<TrackSegment> = Vec::new();
    let mut all_coordinates: Vec<Coordinate> = Vec::new();
    let mut all_elevations: Vec<Option<Elevation>> = Vec::new();
    let mut all_timestamps: Vec<DateTime<Utc>> = Vec::new();
    let mut total_distance: f64 = 0.0;
    let mut total_points: u32 = 0;

    for track in &parse_result.tracks {
        for segment in &track.segments {
            if segment.points.len() < 2 {
                // Skip segments with fewer than 2 points (cannot form a valid TrackSegment)
                continue;
            }

            let mut track_points: Vec<TrackPoint> = Vec::new();

            for pt in &segment.points {
                let track_point = gpx_point_to_track_point(pt)?;
                all_coordinates.push(track_point.coordinate);
                all_elevations.push(track_point.elevation);
                if let Some(ts) = track_point.timestamp {
                    all_timestamps.push(ts);
                }
                track_points.push(track_point);
                total_points += 1;
            }

            // Calculate segment distance
            for window in track_points.windows(2) {
                total_distance += haversine_distance(&window[0].coordinate, &window[1].coordinate);
            }

            let segment = TrackSegment::new(track_points)?;
            all_segments.push(segment);
        }
    }

    if all_segments.is_empty() {
        return Err(RecordedActivityError::NoSegments);
    }

    // Calculate bounding box
    let bounding_box =
        BoundingBox::from_coordinates(&all_coordinates).ok_or(RecordedActivityError::NoSegments)?;

    // Calculate elevation stats
    let (elevation_gain, elevation_loss) = calculate_elevation_stats(&all_elevations);

    // Calculate duration from timestamps
    let duration_seconds = if all_timestamps.len() >= 2 {
        all_timestamps.sort();
        let first = all_timestamps.first().unwrap();
        let last = all_timestamps.last().unwrap();
        let duration = (*last - *first).num_milliseconds() as f64 / 1000.0;
        if duration > 0.0 {
            Some(duration)
        } else {
            None
        }
    } else {
        None
    };

    let started_at = all_timestamps.first().copied();
    let ended_at = all_timestamps.last().copied();

    let statistics = RecordedStatistics {
        distance_meters: total_distance,
        duration_seconds,
        elevation_gain_meters: elevation_gain,
        elevation_loss_meters: elevation_loss,
        point_count: total_points,
        segment_count: all_segments.len() as u32,
    };

    let recorded_track = RecordedTrack::new(all_segments, bounding_box, statistics)?;

    // Generate preview geometry (simplified polyline)
    let preview_geometry = simplify_preview(&all_coordinates, MAX_PREVIEW_POINTS);

    // Derive suggested title from track name or metadata
    let suggested_title = parse_result
        .tracks
        .first()
        .and_then(|t| t.name.clone())
        .or_else(|| parse_result.metadata.name.clone());

    Ok(NormalizedTrack {
        recorded_track,
        preview_geometry,
        suggested_title,
        started_at,
        ended_at,
    })
}

/// Simplify a polyline to at most `max_points` using uniform sampling.
///
/// If the input has fewer than max_points, returns all coordinates unchanged.
/// Otherwise, uniformly samples points to reduce to max_points while always
/// including the first and last points.
fn simplify_preview(coords: &[Coordinate], max_points: usize) -> Vec<Coordinate> {
    if coords.len() <= max_points {
        return coords.to_vec();
    }

    if max_points < 2 {
        return coords.first().copied().into_iter().collect();
    }

    let mut result = Vec::with_capacity(max_points);
    let step = (coords.len() - 1) as f64 / (max_points - 1) as f64;

    for i in 0..max_points {
        let index = (i as f64 * step).round() as usize;
        let index = index.min(coords.len() - 1);
        result.push(coords[index]);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::gpx_parser::{
        GpxMetadata, GpxParseResult, GpxTrack, GpxTrackPoint, GpxTrackSegment, GpxVersion,
    };

    /// Known test case: distance from London to Paris is approximately 344 km.
    /// London: 51.5074, -0.1278
    /// Paris: 48.8566, 2.3522
    #[test]
    fn haversine_london_to_paris() {
        let london = Coordinate::new(51.5074, -0.1278).unwrap();
        let paris = Coordinate::new(48.8566, 2.3522).unwrap();

        let distance = haversine_distance(&london, &paris);
        let expected = 343_556.0; // approximately 343.5 km

        // Within 1% tolerance
        let tolerance = expected * 0.01;
        assert!(
            (distance - expected).abs() < tolerance,
            "Expected ~{expected}m, got {distance}m (diff: {}m)",
            (distance - expected).abs()
        );
    }

    /// Known test case: distance from New York to Los Angeles is approximately 3944 km.
    #[test]
    fn haversine_new_york_to_los_angeles() {
        let ny = Coordinate::new(40.7128, -74.0060).unwrap();
        let la = Coordinate::new(34.0522, -118.2437).unwrap();

        let distance = haversine_distance(&ny, &la);
        let expected = 3_944_000.0; // approximately 3944 km

        let tolerance = expected * 0.01;
        assert!(
            (distance - expected).abs() < tolerance,
            "Expected ~{expected}m, got {distance}m (diff: {}m)",
            (distance - expected).abs()
        );
    }

    /// Same point should have zero distance.
    #[test]
    fn haversine_same_point_is_zero() {
        let point = Coordinate::new(47.0, 11.0).unwrap();
        let distance = haversine_distance(&point, &point);
        assert!(distance.abs() < 0.001);
    }

    /// Short distance (nearby points ~111m apart = 0.001 degree at equator).
    #[test]
    fn haversine_short_distance() {
        let p1 = Coordinate::new(0.0, 0.0).unwrap();
        let p2 = Coordinate::new(0.001, 0.0).unwrap();

        let distance = haversine_distance(&p1, &p2);
        // 0.001 degrees latitude at equator ~ 111 meters
        let expected = 111.19;
        let tolerance = expected * 0.01;
        assert!(
            (distance - expected).abs() < tolerance,
            "Expected ~{expected}m, got {distance}m"
        );
    }

    #[test]
    fn elevation_gain_and_loss_calculated_correctly() {
        let elevations = vec![
            Some(Elevation::new(100.0)),
            Some(Elevation::new(150.0)),
            Some(Elevation::new(120.0)),
            Some(Elevation::new(200.0)),
            Some(Elevation::new(180.0)),
        ];

        let (gain, loss) = calculate_elevation_stats(&elevations);
        // Gain: 50 + 80 = 130
        // Loss: 30 + 20 = 50
        assert!((gain.unwrap() - 130.0).abs() < 0.001);
        assert!((loss.unwrap() - 50.0).abs() < 0.001);
    }

    #[test]
    fn elevation_stats_with_no_elevation_returns_none() {
        let elevations: Vec<Option<Elevation>> = vec![None, None, None];
        let (gain, loss) = calculate_elevation_stats(&elevations);
        assert!(gain.is_none());
        assert!(loss.is_none());
    }

    #[test]
    fn elevation_stats_with_single_point() {
        let elevations = vec![Some(Elevation::new(100.0))];
        let (gain, loss) = calculate_elevation_stats(&elevations);
        // Only one point, no differences to compute
        assert_eq!(gain, Some(0.0));
        assert_eq!(loss, Some(0.0));
    }

    #[test]
    fn elevation_stats_all_ascending() {
        let elevations = vec![
            Some(Elevation::new(100.0)),
            Some(Elevation::new(200.0)),
            Some(Elevation::new(300.0)),
        ];
        let (gain, loss) = calculate_elevation_stats(&elevations);
        assert!((gain.unwrap() - 200.0).abs() < 0.001);
        assert!((loss.unwrap() - 0.0).abs() < 0.001);
    }

    #[test]
    fn normalize_gpx_basic_track() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata {
                name: Some("Test Hike".to_string()),
                description: None,
                time: None,
            },
            tracks: vec![GpxTrack {
                name: Some("Track 1".to_string()),
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.0,
                            lon: 11.0,
                            elevation: Some(500.0),
                            time: Some("2024-01-15T08:00:00Z".to_string()),
                        },
                        GpxTrackPoint {
                            lat: 47.001,
                            lon: 11.001,
                            elevation: Some(520.0),
                            time: Some("2024-01-15T08:05:00Z".to_string()),
                        },
                        GpxTrackPoint {
                            lat: 47.002,
                            lon: 11.002,
                            elevation: Some(510.0),
                            time: Some("2024-01-15T08:10:00Z".to_string()),
                        },
                    ],
                }],
            }],
            total_points: 3,
        };

        let result = normalize_gpx(&parse_result).unwrap();

        assert_eq!(result.recorded_track.segments.len(), 1);
        assert_eq!(result.recorded_track.statistics.point_count, 3);
        assert_eq!(result.recorded_track.statistics.segment_count, 1);
        assert!(result.recorded_track.statistics.distance_meters > 0.0);
        assert!(
            result
                .recorded_track
                .statistics
                .elevation_gain_meters
                .unwrap()
                > 0.0
        );
        assert!(
            result
                .recorded_track
                .statistics
                .elevation_loss_meters
                .unwrap()
                > 0.0
        );
        assert!(result.recorded_track.statistics.duration_seconds.is_some());
        assert_eq!(result.suggested_title, Some("Track 1".to_string()));
        assert!(result.started_at.is_some());
        assert!(result.ended_at.is_some());

        // Verify duration is 10 minutes = 600 seconds
        assert!((result.recorded_track.statistics.duration_seconds.unwrap() - 600.0).abs() < 0.1);

        // Verify elevation: gain = 20, loss = 10
        assert!(
            (result
                .recorded_track
                .statistics
                .elevation_gain_meters
                .unwrap()
                - 20.0)
                .abs()
                < 0.001
        );
        assert!(
            (result
                .recorded_track
                .statistics
                .elevation_loss_meters
                .unwrap()
                - 10.0)
                .abs()
                < 0.001
        );
    }

    #[test]
    fn normalize_gpx_preserves_segment_boundaries() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![
                    GpxTrackSegment {
                        points: vec![
                            GpxTrackPoint {
                                lat: 47.0,
                                lon: 11.0,
                                elevation: None,
                                time: None,
                            },
                            GpxTrackPoint {
                                lat: 47.001,
                                lon: 11.001,
                                elevation: None,
                                time: None,
                            },
                        ],
                    },
                    GpxTrackSegment {
                        points: vec![
                            GpxTrackPoint {
                                lat: 48.0,
                                lon: 12.0,
                                elevation: None,
                                time: None,
                            },
                            GpxTrackPoint {
                                lat: 48.001,
                                lon: 12.001,
                                elevation: None,
                                time: None,
                            },
                        ],
                    },
                ],
            }],
            total_points: 4,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        assert_eq!(result.recorded_track.segments.len(), 2);
        assert_eq!(result.recorded_track.statistics.segment_count, 2);
        assert_eq!(result.recorded_track.statistics.point_count, 4);
    }

    #[test]
    fn normalize_gpx_preserves_coordinates() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.123456,
                            lon: 11.654321,
                            elevation: Some(1234.5),
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.234567,
                            lon: 11.765432,
                            elevation: Some(1345.6),
                            time: None,
                        },
                    ],
                }],
            }],
            total_points: 2,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        let points = result.recorded_track.segments[0].points();
        assert!((points[0].coordinate.latitude - 47.123456).abs() < 1e-10);
        assert!((points[0].coordinate.longitude - 11.654321).abs() < 1e-10);
        assert!((points[0].elevation.unwrap().meters() - 1234.5).abs() < 1e-10);
        assert!((points[1].coordinate.latitude - 47.234567).abs() < 1e-10);
        assert!((points[1].coordinate.longitude - 11.765432).abs() < 1e-10);
    }

    #[test]
    fn normalize_gpx_empty_segments_returns_error() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![],
            }],
            total_points: 0,
        };

        let result = normalize_gpx(&parse_result);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::NoSegments
        ));
    }

    #[test]
    fn normalize_gpx_skips_single_point_segments() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![
                    // This segment has only 1 point, should be skipped
                    GpxTrackSegment {
                        points: vec![GpxTrackPoint {
                            lat: 47.0,
                            lon: 11.0,
                            elevation: None,
                            time: None,
                        }],
                    },
                    // This segment is valid
                    GpxTrackSegment {
                        points: vec![
                            GpxTrackPoint {
                                lat: 48.0,
                                lon: 12.0,
                                elevation: None,
                                time: None,
                            },
                            GpxTrackPoint {
                                lat: 48.001,
                                lon: 12.001,
                                elevation: None,
                                time: None,
                            },
                        ],
                    },
                ],
            }],
            total_points: 3,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        assert_eq!(result.recorded_track.segments.len(), 1);
    }

    #[test]
    fn simplify_preview_under_limit_returns_all() {
        let coords: Vec<Coordinate> = (0..10)
            .map(|i| Coordinate::new(47.0 + i as f64 * 0.001, 11.0).unwrap())
            .collect();

        let preview = simplify_preview(&coords, 1000);
        assert_eq!(preview.len(), 10);
    }

    #[test]
    fn simplify_preview_over_limit_reduces() {
        let coords: Vec<Coordinate> = (0..2000)
            .map(|i| Coordinate::new(47.0 + i as f64 * 0.0001, 11.0).unwrap())
            .collect();

        let preview = simplify_preview(&coords, 1000);
        assert_eq!(preview.len(), 1000);
        // First and last points should be included
        assert_eq!(preview[0].latitude, coords[0].latitude);
        assert_eq!(preview[999].latitude, coords[1999].latitude);
    }

    #[test]
    fn bounding_box_computed_correctly() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.0,
                            lon: 11.0,
                            elevation: None,
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.5,
                            lon: 11.5,
                            elevation: None,
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.2,
                            lon: 11.2,
                            elevation: None,
                            time: None,
                        },
                    ],
                }],
            }],
            total_points: 3,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        let bbox = result.recorded_track.bounding_box;
        assert!((bbox.south_west.latitude - 47.0).abs() < 1e-10);
        assert!((bbox.south_west.longitude - 11.0).abs() < 1e-10);
        assert!((bbox.north_east.latitude - 47.5).abs() < 1e-10);
        assert!((bbox.north_east.longitude - 11.5).abs() < 1e-10);
    }

    #[test]
    fn suggested_title_from_track_name_takes_priority() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata {
                name: Some("Metadata Name".to_string()),
                description: None,
                time: None,
            },
            tracks: vec![GpxTrack {
                name: Some("Track Name".to_string()),
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.0,
                            lon: 11.0,
                            elevation: None,
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.001,
                            lon: 11.001,
                            elevation: None,
                            time: None,
                        },
                    ],
                }],
            }],
            total_points: 2,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        assert_eq!(result.suggested_title, Some("Track Name".to_string()));
    }

    #[test]
    fn suggested_title_falls_back_to_metadata_name() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata {
                name: Some("Metadata Name".to_string()),
                description: None,
                time: None,
            },
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.0,
                            lon: 11.0,
                            elevation: None,
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.001,
                            lon: 11.001,
                            elevation: None,
                            time: None,
                        },
                    ],
                }],
            }],
            total_points: 2,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        assert_eq!(result.suggested_title, Some("Metadata Name".to_string()));
    }

    #[test]
    fn normalize_500k_points_within_time_budget() {
        use std::time::Instant;

        // Generate a realistic hiking path with 500,000 points across 5 segments.
        // Simulates a hike starting at Innsbruck heading northeast with elevation changes.
        let points_per_segment = 100_000;
        let num_segments = 5;
        let total_expected = points_per_segment * num_segments;

        let mut segments = Vec::with_capacity(num_segments);
        let base_lat = 47.2692;
        let base_lon = 11.4041;
        let base_elevation = 574.0;

        for seg_idx in 0..num_segments {
            let mut points = Vec::with_capacity(points_per_segment);
            for pt_idx in 0..points_per_segment {
                let global_idx = seg_idx * points_per_segment + pt_idx;
                // Small increments to simulate real GPS track points
                let lat = base_lat + (global_idx as f64) * 0.000005;
                let lon = base_lon + (global_idx as f64) * 0.000008;
                // Sinusoidal elevation to simulate hills
                let elevation = base_elevation + 200.0 * (global_idx as f64 * 0.0001).sin();
                let seconds = global_idx as u64 * 2; // 2 seconds between points
                                                     // Use a realistic timestamp that advances
                let hour = 6 + (seconds / 3600);
                let minute = (seconds % 3600) / 60;
                let second = seconds % 60;
                let time_str = format!("2024-06-01T{:02}:{:02}:{:02}Z", hour % 24, minute, second);

                points.push(GpxTrackPoint {
                    lat,
                    lon,
                    elevation: Some(elevation),
                    time: Some(time_str),
                });
            }
            segments.push(GpxTrackSegment { points });
        }

        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata {
                name: Some("500k Point Stress Test".to_string()),
                description: None,
                time: None,
            },
            tracks: vec![GpxTrack {
                name: Some("Stress Test Track".to_string()),
                segments,
            }],
            total_points: total_expected,
        };

        let start = Instant::now();
        let result = normalize_gpx(&parse_result).unwrap();
        let elapsed = start.elapsed();

        // Verify: completes within 10 seconds
        assert!(
            elapsed.as_secs() < 10,
            "Normalization took {:?}, exceeds 10 second budget",
            elapsed
        );

        // Verify: total point count is 500,000
        assert_eq!(
            result.recorded_track.statistics.point_count,
            total_expected as u32
        );

        // Verify: preview geometry is exactly 1000 points
        assert_eq!(result.preview_geometry.len(), MAX_PREVIEW_POINTS);

        // Verify: all segment boundaries are preserved (5 segments)
        assert_eq!(result.recorded_track.segments.len(), num_segments);
        assert_eq!(
            result.recorded_track.statistics.segment_count,
            num_segments as u32
        );

        // Verify each segment has the expected number of points
        for segment in &result.recorded_track.segments {
            assert_eq!(segment.point_count(), points_per_segment);
        }
    }

    #[test]
    fn coordinate_values_preserved_bit_for_bit() {
        // Use high-precision coordinates to verify no floating-point alteration
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.123456789012,
                            lon: 11.987654321098,
                            elevation: None,
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.987654321098,
                            lon: 11.123456789012,
                            elevation: None,
                            time: None,
                        },
                    ],
                }],
            }],
            total_points: 2,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        let points = result.recorded_track.segments[0].points();

        // Bit-for-bit equality (not approximately equal)
        assert_eq!(points[0].coordinate.latitude, 47.123456789012);
        assert_eq!(points[0].coordinate.longitude, 11.987654321098);
        assert_eq!(points[1].coordinate.latitude, 47.987654321098);
        assert_eq!(points[1].coordinate.longitude, 11.123456789012);
    }

    #[test]
    fn elevation_values_preserved_exactly() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.0,
                            lon: 11.0,
                            elevation: Some(1234.56789),
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.001,
                            lon: 11.001,
                            elevation: Some(9876.54321),
                            time: None,
                        },
                    ],
                }],
            }],
            total_points: 2,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        let points = result.recorded_track.segments[0].points();

        // Bit-for-bit equality for elevation values
        assert_eq!(points[0].elevation.unwrap().meters(), 1234.56789);
        assert_eq!(points[1].elevation.unwrap().meters(), 9876.54321);
    }

    #[test]
    fn timestamp_strings_parsed_and_preserved() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.0,
                            lon: 11.0,
                            elevation: None,
                            time: Some("2024-03-15T10:30:45Z".to_string()),
                        },
                        GpxTrackPoint {
                            lat: 47.001,
                            lon: 11.001,
                            elevation: None,
                            time: Some("2024-03-15T11:45:30Z".to_string()),
                        },
                    ],
                }],
            }],
            total_points: 2,
        };

        let result = normalize_gpx(&parse_result).unwrap();
        let points = result.recorded_track.segments[0].points();

        // Verify timestamps are parsed correctly
        let ts0 = points[0].timestamp.unwrap();
        assert_eq!(ts0.to_rfc3339(), "2024-03-15T10:30:45+00:00");

        let ts1 = points[1].timestamp.unwrap();
        assert_eq!(ts1.to_rfc3339(), "2024-03-15T11:45:30+00:00");

        // Verify started_at and ended_at
        assert_eq!(result.started_at.unwrap(), ts0);
        assert_eq!(result.ended_at.unwrap(), ts1);
    }

    #[test]
    fn geojson_coordinate_order_from_normalized_track() {
        let parse_result = GpxParseResult {
            version: GpxVersion::Gpx11,
            metadata: GpxMetadata::default(),
            tracks: vec![GpxTrack {
                name: None,
                segments: vec![GpxTrackSegment {
                    points: vec![
                        GpxTrackPoint {
                            lat: 47.2692,
                            lon: 11.4041,
                            elevation: Some(574.0),
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.2700,
                            lon: 11.4050,
                            elevation: Some(580.0),
                            time: None,
                        },
                        GpxTrackPoint {
                            lat: 47.2710,
                            lon: 11.4060,
                            elevation: Some(590.0),
                            time: None,
                        },
                    ],
                }],
            }],
            total_points: 3,
        };

        let result = normalize_gpx(&parse_result).unwrap();

        // Verify GeoJSON position ordering from the NormalizedTrack's segments
        let segment = &result.recorded_track.segments[0];
        for point in segment.points() {
            let position = point.coordinate.as_geojson_position();
            // GeoJSON: [longitude, latitude]
            assert_eq!(position[0], point.coordinate.longitude);
            assert_eq!(position[1], point.coordinate.latitude);
            // Verify longitude is first (different from internal lat/lon storage)
            assert_ne!(position[0], point.coordinate.latitude);
        }

        // Explicit check for first point
        let first = segment.points()[0].coordinate.as_geojson_position();
        assert_eq!(first[0], 11.4041); // longitude first
        assert_eq!(first[1], 47.2692); // latitude second

        // Verify preview geometry also produces correct GeoJSON positions
        for coord in &result.preview_geometry {
            let pos = coord.as_geojson_position();
            assert_eq!(pos[0], coord.longitude);
            assert_eq!(pos[1], coord.latitude);
        }

        // Verify bounding box GeoJSON ordering
        let bbox = result.recorded_track.bounding_box;
        let geojson_bbox = bbox.as_geojson_bbox();
        // [west, south, east, north]
        assert_eq!(geojson_bbox[0], 11.4041); // west = min longitude
        assert_eq!(geojson_bbox[1], 47.2692); // south = min latitude
        assert_eq!(geojson_bbox[2], 11.4060); // east = max longitude
        assert_eq!(geojson_bbox[3], 47.2710); // north = max latitude
    }
}
