//! Property-based tests (proptest) for the GPX export generator.
//!
//! Verifies that generated GPX preserves geometry within floating-point tolerance,
//! maintains segment boundaries, produces deterministic output, never fabricates
//! sensor data, and always produces well-formed XML.

use haiker_app::exports::{generate_gpx, GpxGeneratorInput, GpxPoint};
use haiker_app::imports::gpx_parser::parse_gpx;
use proptest::prelude::*;
use quick_xml::events::Event;
use quick_xml::Reader;

/// Strategy to generate an optional elevation value in [-500.0, 9000.0].
fn elevation_strategy() -> impl Strategy<Value = Option<f64>> {
    proptest::option::of(-500.0f64..=9000.0f64)
}

/// Strategy to generate a single GpxPoint with valid coordinates.
fn gpx_point_strategy() -> impl Strategy<Value = GpxPoint> {
    (
        -90.0f64..=90.0f64,
        -180.0f64..=180.0f64,
        elevation_strategy(),
    )
        .prop_map(|(latitude, longitude, elevation)| GpxPoint {
            latitude,
            longitude,
            elevation,
        })
}

/// Strategy to generate a non-empty segment of 2-50 GpxPoints.
fn segment_strategy() -> impl Strategy<Value = Vec<GpxPoint>> {
    proptest::collection::vec(gpx_point_strategy(), 2..=50)
}

/// Strategy to generate 1-5 non-empty segments.
fn segments_strategy() -> impl Strategy<Value = Vec<Vec<GpxPoint>>> {
    proptest::collection::vec(segment_strategy(), 1..=5)
}

/// Strategy to generate an activity name with arbitrary printable Unicode characters,
/// including XML special chars like <, >, &, ', ".
fn activity_name_strategy() -> impl Strategy<Value = Option<String>> {
    proptest::option::of(
        proptest::collection::vec(
            prop_oneof![
                // Normal printable ASCII
                proptest::char::range(' ', '~'),
                // XML special characters
                Just('<'),
                Just('>'),
                Just('&'),
                Just('\''),
                Just('"'),
                // Some Unicode characters
                proptest::char::range('\u{00C0}', '\u{00FF}'),
                proptest::char::range('\u{4E00}', '\u{4E50}'),
            ],
            1..=100,
        )
        .prop_map(|chars| chars.into_iter().collect::<String>()),
    )
}

/// Strategy to generate a complete GpxGeneratorInput with arbitrary valid geometry.
fn gpx_input_strategy() -> impl Strategy<Value = GpxGeneratorInput> {
    (activity_name_strategy(), segments_strategy()).prop_map(|(activity_name, segments)| {
        GpxGeneratorInput {
            activity_name,
            segments,
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Property test 1: Round-trip preservation.
    ///
    /// Generate GPX from arbitrary geometry, parse it back using parse_gpx,
    /// and verify that coordinates and structure are preserved within tolerance.
    #[test]
    fn round_trip_preservation(input in gpx_input_strategy()) {
        let gpx_bytes = generate_gpx(&input).expect("generation must succeed");
        let parsed = parse_gpx(&gpx_bytes).expect("generated GPX must be parseable");

        // Filter out empty segments from input (generator skips them)
        let non_empty_segments: Vec<&Vec<GpxPoint>> = input.segments.iter()
            .filter(|s| !s.is_empty())
            .collect();

        // Must have exactly one track
        prop_assert_eq!(parsed.tracks.len(), 1, "expected exactly 1 track");
        let track = &parsed.tracks[0];

        // (a) Number of segments matches
        prop_assert_eq!(
            track.segments.len(),
            non_empty_segments.len(),
            "segment count mismatch: parsed {} vs input {}",
            track.segments.len(),
            non_empty_segments.len()
        );

        // (b) and (c) Number of points per segment matches, coordinates within tolerance
        for (seg_idx, (parsed_seg, input_seg)) in
            track.segments.iter().zip(non_empty_segments.iter()).enumerate()
        {
            prop_assert_eq!(
                parsed_seg.points.len(),
                input_seg.len(),
                "point count mismatch in segment {}",
                seg_idx
            );

            for (pt_idx, (parsed_pt, input_pt)) in
                parsed_seg.points.iter().zip(input_seg.iter()).enumerate()
            {
                let lat_diff = (parsed_pt.lat - input_pt.latitude).abs();
                let lon_diff = (parsed_pt.lon - input_pt.longitude).abs();

                prop_assert!(
                    lat_diff < 1e-6,
                    "latitude mismatch at segment {} point {}: parsed={} input={} diff={}",
                    seg_idx, pt_idx, parsed_pt.lat, input_pt.latitude, lat_diff
                );
                prop_assert!(
                    lon_diff < 1e-6,
                    "longitude mismatch at segment {} point {}: parsed={} input={} diff={}",
                    seg_idx, pt_idx, parsed_pt.lon, input_pt.longitude, lon_diff
                );

                // (d) Elevation within 0.1m tolerance when present
                match (parsed_pt.elevation, input_pt.elevation) {
                    (Some(parsed_ele), Some(input_ele)) => {
                        let ele_diff = (parsed_ele - input_ele).abs();
                        prop_assert!(
                            ele_diff < 0.1,
                            "elevation mismatch at segment {} point {}: parsed={} input={} diff={}",
                            seg_idx, pt_idx, parsed_ele, input_ele, ele_diff
                        );
                    }
                    (None, None) => {} // both absent, OK
                    (parsed_ele, input_ele) => {
                        prop_assert!(
                            false,
                            "elevation presence mismatch at segment {} point {}: parsed={:?} input={:?}",
                            seg_idx, pt_idx, parsed_ele, input_ele
                        );
                    }
                }
            }
        }
    }

    /// Property test 2: Determinism.
    ///
    /// Generate GPX from the same input twice, assert byte-for-byte equality.
    #[test]
    fn determinism(input in gpx_input_strategy()) {
        let result1 = generate_gpx(&input).expect("first generation must succeed");
        let result2 = generate_gpx(&input).expect("second generation must succeed");

        prop_assert_eq!(
            result1,
            result2,
            "identical inputs must produce byte-for-byte identical output"
        );
    }

    /// Property test 3: No fabricated timestamps or sensor data.
    ///
    /// Generate GPX from arbitrary geometry (with no timestamps in input),
    /// parse it back, and verify that no track point has a timestamp.
    #[test]
    fn no_fabricated_timestamps(input in gpx_input_strategy()) {
        let gpx_bytes = generate_gpx(&input).expect("generation must succeed");
        let parsed = parse_gpx(&gpx_bytes).expect("generated GPX must be parseable");

        for track in &parsed.tracks {
            for segment in &track.segments {
                for (pt_idx, point) in segment.points.iter().enumerate() {
                    prop_assert!(
                        point.time.is_none(),
                        "track point {} has fabricated timestamp: {:?}",
                        pt_idx,
                        point.time
                    );
                }
            }
        }

        // Also verify no <extensions> block in the raw XML
        let xml_str = std::str::from_utf8(&gpx_bytes).expect("valid UTF-8");
        prop_assert!(
            !xml_str.contains("<extensions>"),
            "generated GPX must not contain <extensions> block"
        );
    }

    /// Property test 4: Segment boundary preservation.
    ///
    /// Generate GPX from geometry with N non-empty segments, parse back,
    /// verify exactly N segments are returned.
    #[test]
    fn segment_boundary_preservation(input in gpx_input_strategy()) {
        let gpx_bytes = generate_gpx(&input).expect("generation must succeed");
        let parsed = parse_gpx(&gpx_bytes).expect("generated GPX must be parseable");

        let non_empty_count = input.segments.iter().filter(|s| !s.is_empty()).count();

        prop_assert_eq!(parsed.tracks.len(), 1, "expected exactly 1 track");
        prop_assert_eq!(
            parsed.tracks[0].segments.len(),
            non_empty_count,
            "segment count must match number of non-empty input segments"
        );
    }

    /// Property test 5: Well-formed XML.
    ///
    /// Generate GPX from arbitrary geometry, attempt to parse the output bytes
    /// with quick-xml event reader, verify no parse errors occur.
    #[test]
    fn well_formed_xml(input in gpx_input_strategy()) {
        let gpx_bytes = generate_gpx(&input).expect("generation must succeed");

        let mut reader = Reader::from_reader(gpx_bytes.as_slice());
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => {
                    prop_assert!(
                        false,
                        "generated GPX is not well-formed XML: {}",
                        e
                    );
                }
            }
            buf.clear();
        }
    }
}
