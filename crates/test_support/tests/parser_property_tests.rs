//! Parser property-style tests.
//!
//! These tests verify important invariants of the GPX parser:
//! - Random bytes never cause a panic (always returns Result)
//! - Valid coordinate values round-trip through Coordinate construction
//! - Valid GPX with N points produces exactly N points in output

use haiker_app::imports::gpx_parser::parse_gpx;
use haiker_app::recorded_activity::Coordinate;

/// Random byte sequences fed to the parser never cause a panic.
/// They may produce Ok (if they happen to be valid XML) or Err, but never panic.
#[test]
fn random_bytes_never_panic() {
    let test_inputs: Vec<Vec<u8>> = vec![
        vec![],
        vec![0],
        vec![0xFF; 100],
        vec![0x00; 1000],
        b"not xml at all".to_vec(),
        b"<".to_vec(),
        b"<gpx".to_vec(),
        b"<?xml".to_vec(),
        b"<?xml version=\"1.0\"?>".to_vec(),
        b"\x00\x01\x02\x03\x04\x05".to_vec(),
        b"\xff\xfe\xfd\xfc".to_vec(),
        // UTF-8 BOM followed by garbage
        b"\xef\xbb\xbfgarbage".to_vec(),
        // Partial GPX - valid XML, parser returns Ok with empty tracks
        b"<?xml version=\"1.0\"?><gpx version=\"1.1\"></gpx>".to_vec(),
        // Deeply nested without gpx wrapper
        b"<a><a><a><a><a><a><a><a><a><a></a></a></a></a></a></a></a></a></a></a>".to_vec(),
    ];

    for input in &test_inputs {
        // This must never panic - it should always return a Result (Ok or Err)
        let _ = parse_gpx(input);
    }

    // Specifically verify that invalid/garbage inputs produce errors
    let must_error_inputs: Vec<Vec<u8>> = vec![
        vec![],
        vec![0],
        vec![0xFF; 100],
        vec![0x00; 1000],
        b"not xml at all".to_vec(),
        b"\x00\x01\x02\x03\x04\x05".to_vec(),
        b"\xff\xfe\xfd\xfc".to_vec(),
    ];

    for (i, input) in must_error_inputs.iter().enumerate() {
        let result = parse_gpx(input);
        assert!(
            result.is_err(),
            "Input #{i} should produce an error, got Ok: {:?}",
            result.unwrap()
        );
    }
}

/// Pseudo-random bytes generated deterministically never cause a panic.
#[test]
fn deterministic_pseudo_random_bytes_never_panic() {
    // Simple PRNG (xorshift) for deterministic "random" bytes
    let mut state: u64 = 12345;
    for _ in 0..100 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;

        let len = (state % 512) as usize + 1;
        let mut bytes = Vec::with_capacity(len);
        let mut s = state;
        for _ in 0..len {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            bytes.push((s & 0xFF) as u8);
        }

        // Must not panic
        let _ = parse_gpx(&bytes);
    }
}

/// Valid coordinate pairs successfully round-trip through Coordinate::new.
#[test]
fn coordinate_round_trip_valid_values() {
    let test_cases: Vec<(f64, f64)> = vec![
        (0.0, 0.0),
        (90.0, 180.0),
        (-90.0, -180.0),
        (45.5, 90.5),
        (-45.5, -90.5),
        (89.999999, 179.999999),
        (-89.999999, -179.999999),
        (47.2692, 11.3933),
        (35.6762, 139.6503),  // Tokyo
        (-33.8688, 151.2093), // Sydney
        (51.5074, -0.1278),   // London
        (-22.9068, -43.1729), // Rio
    ];

    for (lat, lon) in &test_cases {
        let coord = Coordinate::new(*lat, *lon)
            .unwrap_or_else(|_| panic!("Failed to create Coordinate({lat}, {lon})"));
        assert!(
            (coord.latitude - lat).abs() < 1e-10,
            "Latitude mismatch for ({lat}, {lon}): got {}",
            coord.latitude
        );
        assert!(
            (coord.longitude - lon).abs() < 1e-10,
            "Longitude mismatch for ({lat}, {lon}): got {}",
            coord.longitude
        );
    }
}

/// Invalid coordinate values are properly rejected.
#[test]
fn coordinate_rejects_out_of_range() {
    let invalid_cases: Vec<(f64, f64)> = vec![
        (91.0, 0.0),
        (-91.0, 0.0),
        (0.0, 181.0),
        (0.0, -181.0),
        (999.0, 0.0),
        (0.0, 999.0),
        (f64::INFINITY, 0.0),
        (0.0, f64::INFINITY),
        (f64::NEG_INFINITY, 0.0),
        (0.0, f64::NEG_INFINITY),
    ];

    for (lat, lon) in &invalid_cases {
        let result = Coordinate::new(*lat, *lon);
        assert!(result.is_err(), "Should reject ({lat}, {lon}) but got Ok");
    }
}

/// NaN coordinates are properly rejected.
#[test]
fn coordinate_rejects_nan() {
    let result = Coordinate::new(f64::NAN, 0.0);
    assert!(result.is_err());

    let result = Coordinate::new(0.0, f64::NAN);
    assert!(result.is_err());
}

/// A valid GPX with exactly N points produces exactly N points in the output.
#[test]
fn valid_gpx_point_count_matches() {
    let test_cases = vec![
        (2, generate_gpx_with_n_points(2)),
        (5, generate_gpx_with_n_points(5)),
        (10, generate_gpx_with_n_points(10)),
        (50, generate_gpx_with_n_points(50)),
        (100, generate_gpx_with_n_points(100)),
    ];

    for (expected_count, gpx_content) in test_cases {
        let result = parse_gpx(gpx_content.as_bytes())
            .unwrap_or_else(|e| panic!("Failed to parse GPX with {expected_count} points: {e}"));
        assert_eq!(
            result.total_points, expected_count,
            "Expected {expected_count} points, got {}",
            result.total_points
        );
    }
}

/// Generates a valid GPX 1.1 string with exactly `n` track points.
fn generate_gpx_with_n_points(n: usize) -> String {
    let mut gpx = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <gpx version=\"1.1\" creator=\"test\">\n\
         <trk><trkseg>\n",
    );

    for i in 0..n {
        let lat = 47.0 + (i as f64) * 0.0001;
        let lon = 11.0 + (i as f64) * 0.0001;
        gpx.push_str(&format!("<trkpt lat=\"{lat:.6}\" lon=\"{lon:.6}\"/>\n"));
    }

    gpx.push_str("</trkseg></trk>\n</gpx>");
    gpx
}

/// Parser handles all fixture files without panicking.
#[test]
fn all_fixtures_parse_without_panic() {
    use haiker_test_support::fixtures;

    let all_fixtures: Vec<(&str, &[u8])> = vec![
        ("valid_simple", fixtures::valid_simple()),
        ("valid_gpx10", fixtures::valid_gpx10()),
        ("valid_multi_segment", fixtures::valid_multi_segment()),
        ("valid_no_elevation", fixtures::valid_no_elevation()),
        ("valid_no_timestamps", fixtures::valid_no_timestamps()),
        ("valid_non_ascii", fixtures::valid_non_ascii()),
        ("large_route", fixtures::large_route()),
        ("malformed_xml", fixtures::malformed_xml()),
        ("invalid_coordinates", fixtures::invalid_coordinates()),
        ("xxe_attack", fixtures::xxe_attack()),
        ("excessive_depth", fixtures::excessive_depth()),
    ];

    for (name, data) in all_fixtures {
        // Should never panic regardless of outcome
        let _ = parse_gpx(data);
        // Verify valid fixtures actually parse
        if name.starts_with("valid_") || name == "large_route" {
            let result = parse_gpx(data);
            assert!(
                result.is_ok(),
                "Fixture '{name}' should parse successfully, got error: {:?}",
                result.unwrap_err()
            );
        }
    }
}
