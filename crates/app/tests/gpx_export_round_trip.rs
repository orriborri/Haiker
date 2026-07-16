//! Deterministic round-trip tests for GPX export generation and parsing.
//!
//! These tests verify that generating GPX from domain geometry and parsing
//! it back preserves structure, coordinates, and metadata within defined
//! tolerances.

use haiker_app::exports::{generate_gpx, GpxGeneratorInput, GpxPoint};
use haiker_app::imports::gpx_parser::parse_gpx;

/// Latitude/longitude tolerance: coordinates must match within 1e-6 degrees.
const COORD_TOLERANCE: f64 = 1e-6;

/// Elevation tolerance: values must match within 0.1 meters.
const ELEVATION_TOLERANCE: f64 = 0.1;

#[test]
fn round_trip_multiple_segments_preserves_geometry_within_tolerance() {
    let input = GpxGeneratorInput {
        activity_name: Some("Multi-Segment Hike".to_string()),
        segments: vec![
            vec![
                GpxPoint {
                    latitude: 47.2692124,
                    longitude: 11.3933257,
                    elevation: Some(574.2),
                },
                GpxPoint {
                    latitude: 47.2695011,
                    longitude: 11.3938400,
                    elevation: Some(578.5),
                },
            ],
            vec![
                GpxPoint {
                    latitude: 46.5000123,
                    longitude: 10.0000456,
                    elevation: Some(1200.3),
                },
                GpxPoint {
                    latitude: 46.5010789,
                    longitude: 10.0010123,
                    elevation: Some(1220.7),
                },
                GpxPoint {
                    latitude: 46.5020456,
                    longitude: 10.0020789,
                    elevation: Some(1240.1),
                },
            ],
            vec![
                GpxPoint {
                    latitude: 48.1234567,
                    longitude: 12.9876543,
                    elevation: Some(800.0),
                },
                GpxPoint {
                    latitude: 48.1240000,
                    longitude: 12.9880000,
                    elevation: Some(810.5),
                },
                GpxPoint {
                    latitude: 48.1250000,
                    longitude: 12.9890000,
                    elevation: Some(820.0),
                },
                GpxPoint {
                    latitude: 48.1260000,
                    longitude: 12.9900000,
                    elevation: Some(830.0),
                },
            ],
        ],
    };

    let gpx_bytes = generate_gpx(&input).expect("generation should succeed");
    let parsed = parse_gpx(&gpx_bytes).expect("parsing should succeed");

    // Must have exactly one track
    assert_eq!(parsed.tracks.len(), 1);
    let track = &parsed.tracks[0];

    // Segment count must match (3 non-empty segments)
    assert_eq!(track.segments.len(), 3);

    // Verify point counts per segment
    assert_eq!(track.segments[0].points.len(), 2);
    assert_eq!(track.segments[1].points.len(), 3);
    assert_eq!(track.segments[2].points.len(), 4);

    // Verify coordinates within tolerance for all segments
    for (seg_idx, (parsed_seg, input_seg)) in
        track.segments.iter().zip(input.segments.iter()).enumerate()
    {
        for (pt_idx, (parsed_pt, input_pt)) in
            parsed_seg.points.iter().zip(input_seg.iter()).enumerate()
        {
            let lat_diff = (parsed_pt.lat - input_pt.latitude).abs();
            let lon_diff = (parsed_pt.lon - input_pt.longitude).abs();
            assert!(
                lat_diff < COORD_TOLERANCE,
                "latitude mismatch at seg {} pt {}: {} vs {} (diff {})",
                seg_idx,
                pt_idx,
                parsed_pt.lat,
                input_pt.latitude,
                lat_diff
            );
            assert!(
                lon_diff < COORD_TOLERANCE,
                "longitude mismatch at seg {} pt {}: {} vs {} (diff {})",
                seg_idx,
                pt_idx,
                parsed_pt.lon,
                input_pt.longitude,
                lon_diff
            );

            if let (Some(parsed_ele), Some(input_ele)) = (parsed_pt.elevation, input_pt.elevation) {
                let ele_diff = (parsed_ele - input_ele).abs();
                assert!(
                    ele_diff < ELEVATION_TOLERANCE,
                    "elevation mismatch at seg {} pt {}: {} vs {} (diff {})",
                    seg_idx,
                    pt_idx,
                    parsed_ele,
                    input_ele,
                    ele_diff
                );
            }
        }
    }
}

#[test]
fn round_trip_optional_elevation_preserves_presence_and_absence() {
    let input = GpxGeneratorInput {
        activity_name: Some("Elevation Mix".to_string()),
        segments: vec![vec![
            GpxPoint {
                latitude: 47.0,
                longitude: 11.0,
                elevation: Some(500.0),
            },
            GpxPoint {
                latitude: 47.001,
                longitude: 11.001,
                elevation: None,
            },
            GpxPoint {
                latitude: 47.002,
                longitude: 11.002,
                elevation: Some(510.3),
            },
            GpxPoint {
                latitude: 47.003,
                longitude: 11.003,
                elevation: None,
            },
            GpxPoint {
                latitude: 47.004,
                longitude: 11.004,
                elevation: Some(520.7),
            },
        ]],
    };

    let gpx_bytes = generate_gpx(&input).expect("generation should succeed");
    let parsed = parse_gpx(&gpx_bytes).expect("parsing should succeed");

    assert_eq!(parsed.tracks.len(), 1);
    let track = &parsed.tracks[0];
    assert_eq!(track.segments.len(), 1);
    assert_eq!(track.segments[0].points.len(), 5);

    for (idx, (parsed_pt, input_pt)) in track.segments[0]
        .points
        .iter()
        .zip(input.segments[0].iter())
        .enumerate()
    {
        match (parsed_pt.elevation, input_pt.elevation) {
            (Some(parsed_ele), Some(input_ele)) => {
                let ele_diff = (parsed_ele - input_ele).abs();
                assert!(
                    ele_diff < ELEVATION_TOLERANCE,
                    "elevation mismatch at point {}: {} vs {} (diff {})",
                    idx,
                    parsed_ele,
                    input_ele,
                    ele_diff
                );
            }
            (None, None) => {} // both absent, correct
            (parsed_ele, input_ele) => {
                panic!(
                    "elevation presence mismatch at point {}: parsed={:?} input={:?}",
                    idx, parsed_ele, input_ele
                );
            }
        }
    }
}

#[test]
fn round_trip_non_ascii_activity_name_preserved() {
    // Use Japanese, German, and special characters
    let name = "\u{5c71}\u{306e}\u{30cf}\u{30a4}\u{30ad}\u{30f3}\u{30b0} - Bergwanderung \u{00fc}ber den H\u{00f6}henweg <special>&\"test\"";
    let input = GpxGeneratorInput {
        activity_name: Some(name.to_string()),
        segments: vec![vec![
            GpxPoint {
                latitude: 47.0,
                longitude: 11.0,
                elevation: Some(500.0),
            },
            GpxPoint {
                latitude: 47.001,
                longitude: 11.001,
                elevation: Some(510.0),
            },
        ]],
    };

    let gpx_bytes = generate_gpx(&input).expect("generation should succeed");
    let parsed = parse_gpx(&gpx_bytes).expect("parsing should succeed");

    assert_eq!(parsed.tracks.len(), 1);
    let track = &parsed.tracks[0];

    // The parser should unescape XML entities, restoring the original name.
    // Note: the generator XML-escapes special chars, the parser unescapes them.
    let parsed_name = track.name.as_deref().expect("track name should be present");

    // Full string equality ensures no truncation, reordering, or extra characters
    assert_eq!(
        parsed_name, name,
        "round-tripped name should exactly match the original"
    );
}

#[test]
fn round_trip_large_track_1000_points_within_tolerance() {
    // Generate 1000 points with incrementing coordinates
    let points: Vec<GpxPoint> = (0..1000)
        .map(|i| GpxPoint {
            latitude: 47.0 + (i as f64) * 0.0001,
            longitude: 11.0 + (i as f64) * 0.00015,
            elevation: Some(500.0 + (i as f64) * 0.5),
        })
        .collect();

    let input = GpxGeneratorInput {
        activity_name: Some("Large Track Test".to_string()),
        segments: vec![points],
    };

    let gpx_bytes = generate_gpx(&input).expect("generation should succeed");
    let parsed = parse_gpx(&gpx_bytes).expect("parsing should succeed");

    assert_eq!(parsed.tracks.len(), 1);
    let track = &parsed.tracks[0];
    assert_eq!(track.segments.len(), 1);
    assert_eq!(track.segments[0].points.len(), 1000);

    for (idx, (parsed_pt, input_pt)) in track.segments[0]
        .points
        .iter()
        .zip(input.segments[0].iter())
        .enumerate()
    {
        let lat_diff = (parsed_pt.lat - input_pt.latitude).abs();
        let lon_diff = (parsed_pt.lon - input_pt.longitude).abs();
        assert!(
            lat_diff < COORD_TOLERANCE,
            "latitude mismatch at point {}: {} vs {} (diff {})",
            idx,
            parsed_pt.lat,
            input_pt.latitude,
            lat_diff
        );
        assert!(
            lon_diff < COORD_TOLERANCE,
            "longitude mismatch at point {}: {} vs {} (diff {})",
            idx,
            parsed_pt.lon,
            input_pt.longitude,
            lon_diff
        );

        if let (Some(parsed_ele), Some(input_ele)) = (parsed_pt.elevation, input_pt.elevation) {
            let ele_diff = (parsed_ele - input_ele).abs();
            assert!(
                ele_diff < ELEVATION_TOLERANCE,
                "elevation mismatch at point {}: {} vs {} (diff {})",
                idx,
                parsed_ele,
                input_ele,
                ele_diff
            );
        }
    }
}

#[test]
fn round_trip_empty_segments_are_omitted() {
    let input = GpxGeneratorInput {
        activity_name: Some("Sparse Segments".to_string()),
        segments: vec![
            vec![], // empty - should be omitted
            vec![
                GpxPoint {
                    latitude: 47.0,
                    longitude: 11.0,
                    elevation: Some(500.0),
                },
                GpxPoint {
                    latitude: 47.001,
                    longitude: 11.001,
                    elevation: Some(510.0),
                },
            ],
            vec![], // empty - should be omitted
            vec![], // empty - should be omitted
            vec![GpxPoint {
                latitude: 48.0,
                longitude: 12.0,
                elevation: Some(800.0),
            }],
        ],
    };

    let gpx_bytes = generate_gpx(&input).expect("generation should succeed");
    let parsed = parse_gpx(&gpx_bytes).expect("parsing should succeed");

    assert_eq!(parsed.tracks.len(), 1);
    let track = &parsed.tracks[0];

    // Only 2 non-empty segments should survive the round-trip
    assert_eq!(
        track.segments.len(),
        2,
        "only non-empty segments should survive round-trip"
    );

    // First non-empty segment had 2 points
    assert_eq!(track.segments[0].points.len(), 2);
    // Second non-empty segment had 1 point
    assert_eq!(track.segments[1].points.len(), 1);

    // Verify first point of second non-empty segment
    let pt = &track.segments[1].points[0];
    assert!((pt.lat - 48.0).abs() < COORD_TOLERANCE);
    assert!((pt.lon - 12.0).abs() < COORD_TOLERANCE);
}
