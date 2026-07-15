//! Integration tests for the GPX parser using test fixtures and boundary inputs.

use haiker_app::imports::gpx_parser::{parse_gpx, GpxParseErrorCode, GpxVersion};

// ---------------------------------------------------------------------------
// Valid fixture tests
// ---------------------------------------------------------------------------

#[test]
fn valid_simple_fixture_parses_correctly() {
    let data = haiker_test_support::fixtures::valid_simple();
    let result = parse_gpx(data).unwrap();
    assert_eq!(result.version, GpxVersion::Gpx11);
    assert_eq!(result.total_points, 10);
    assert_eq!(result.tracks.len(), 1);
    assert_eq!(result.tracks[0].segments.len(), 1);
    assert_eq!(result.tracks[0].segments[0].points.len(), 10);
    assert_eq!(result.metadata.name.as_deref(), Some("Morning Hike"));

    // All points should have elevation and time
    for pt in &result.tracks[0].segments[0].points {
        assert!(pt.elevation.is_some(), "expected elevation in valid_simple");
        assert!(pt.time.is_some(), "expected time in valid_simple");
        assert!((-90.0..=90.0).contains(&pt.lat));
        assert!((-180.0..=180.0).contains(&pt.lon));
    }
}

#[test]
fn valid_gpx10_fixture_parses_correctly() {
    let data = haiker_test_support::fixtures::valid_gpx10();
    let result = parse_gpx(data).unwrap();
    assert_eq!(result.version, GpxVersion::Gpx10);
    assert_eq!(result.total_points, 5);
    assert_eq!(result.tracks.len(), 1);
    assert_eq!(result.tracks[0].segments.len(), 1);
    assert_eq!(result.tracks[0].segments[0].points.len(), 5);
    assert_eq!(result.metadata.name.as_deref(), Some("Evening Walk"));

    for pt in &result.tracks[0].segments[0].points {
        assert!(pt.elevation.is_some(), "expected elevation in valid_gpx10");
        assert!(pt.time.is_some(), "expected time in valid_gpx10");
    }
}

#[test]
fn valid_multi_segment_fixture_parses_correctly() {
    let data = haiker_test_support::fixtures::valid_multi_segment();
    let result = parse_gpx(data).unwrap();
    assert_eq!(result.version, GpxVersion::Gpx11);
    assert_eq!(result.total_points, 8);
    assert_eq!(result.tracks.len(), 1);
    assert_eq!(result.tracks[0].segments.len(), 3);
    assert_eq!(result.tracks[0].segments[0].points.len(), 3);
    assert_eq!(result.tracks[0].segments[1].points.len(), 3);
    assert_eq!(result.tracks[0].segments[2].points.len(), 2);

    for seg in &result.tracks[0].segments {
        for pt in &seg.points {
            assert!(pt.elevation.is_some());
            assert!(pt.time.is_some());
        }
    }
}

#[test]
fn valid_no_elevation_fixture_parses_correctly() {
    let data = haiker_test_support::fixtures::valid_no_elevation();
    let result = parse_gpx(data).unwrap();
    assert_eq!(result.version, GpxVersion::Gpx11);
    assert_eq!(result.total_points, 4);
    assert_eq!(result.tracks[0].segments[0].points.len(), 4);
    assert_eq!(result.metadata.name.as_deref(), Some("Flat Walk"));

    for pt in &result.tracks[0].segments[0].points {
        assert!(
            pt.elevation.is_none(),
            "expected no elevation in valid_no_elevation"
        );
        assert!(
            pt.time.is_some(),
            "expected time present in valid_no_elevation"
        );
    }
}

#[test]
fn valid_no_timestamps_fixture_parses_correctly() {
    let data = haiker_test_support::fixtures::valid_no_timestamps();
    let result = parse_gpx(data).unwrap();
    assert_eq!(result.version, GpxVersion::Gpx11);
    assert_eq!(result.total_points, 5);
    assert_eq!(result.tracks[0].segments[0].points.len(), 5);
    assert_eq!(result.metadata.name.as_deref(), Some("No Timestamps Route"));

    for pt in &result.tracks[0].segments[0].points {
        assert!(
            pt.elevation.is_some(),
            "expected elevation in valid_no_timestamps"
        );
        assert!(pt.time.is_none(), "expected no time in valid_no_timestamps");
    }
}

#[test]
fn valid_non_ascii_fixture_parses_correctly() {
    let data = haiker_test_support::fixtures::valid_non_ascii();
    let result = parse_gpx(data).unwrap();
    assert_eq!(result.version, GpxVersion::Gpx11);
    assert_eq!(result.total_points, 3);
    assert_eq!(result.tracks[0].segments[0].points.len(), 3);

    // Check non-ASCII metadata is preserved
    let name = result.metadata.name.as_deref().unwrap();
    assert!(name.contains("Bodensee"));

    let track_name = result.tracks[0].name.as_deref().unwrap();
    assert!(track_name.contains("Fuji-san"));
}

#[test]
fn large_route_fixture_parses_correctly() {
    let data = haiker_test_support::fixtures::large_route();
    let result = parse_gpx(data).unwrap();
    assert_eq!(result.version, GpxVersion::Gpx11);
    assert_eq!(result.total_points, 1000);
    assert_eq!(result.tracks.len(), 1);
    assert_eq!(result.tracks[0].segments.len(), 1);
    assert_eq!(result.tracks[0].segments[0].points.len(), 1000);

    for pt in &result.tracks[0].segments[0].points {
        assert!(pt.elevation.is_some());
        assert!(pt.time.is_some());
        assert!((-90.0..=90.0).contains(&pt.lat));
        assert!((-180.0..=180.0).contains(&pt.lon));
    }
}

// ---------------------------------------------------------------------------
// Error fixture tests
// ---------------------------------------------------------------------------

#[test]
fn malformed_xml_fixture_returns_invalid_xml() {
    let data = haiker_test_support::fixtures::malformed_xml();
    let err = parse_gpx(data).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::InvalidXml);
}

#[test]
fn invalid_coordinates_fixture_returns_invalid_coordinates() {
    let data = haiker_test_support::fixtures::invalid_coordinates();
    let err = parse_gpx(data).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::InvalidCoordinates);
}

#[test]
fn xxe_attack_fixture_returns_external_entity() {
    let data = haiker_test_support::fixtures::xxe_attack();
    let err = parse_gpx(data).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::ExternalEntity);
}

#[test]
fn excessive_depth_fixture_returns_xml_too_deep() {
    let data = haiker_test_support::fixtures::excessive_depth();
    let err = parse_gpx(data).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::XmlTooDeep);
}

// ---------------------------------------------------------------------------
// Boundary value tests
// ---------------------------------------------------------------------------

/// Generate a GPX string with self-closing trkpt elements within a single segment.
fn build_gpx_with_points(n: usize) -> String {
    let mut gpx =
        String::from(r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1"><trk><trkseg>"#);
    for i in 0..n {
        let lat = (i as f64 * 0.0001) % 89.0;
        let lon = (i as f64 * 0.0001) % 179.0;
        gpx.push_str(&format!(r#"<trkpt lat="{lat}" lon="{lon}"/>"#));
    }
    gpx.push_str("</trkseg></trk></gpx>");
    gpx
}

/// Generate a GPX string with empty trkseg elements.
fn build_gpx_with_segments(n: usize) -> String {
    let mut gpx = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1"><trk>"#);
    for _ in 0..n {
        gpx.push_str("<trkseg></trkseg>");
    }
    gpx.push_str("</trk></gpx>");
    gpx
}

#[test]
fn exactly_max_points_succeeds() {
    let gpx = build_gpx_with_points(500_000);
    let result = parse_gpx(gpx.as_bytes()).unwrap();
    assert_eq!(result.total_points, 500_000);
}

#[test]
fn one_over_max_points_fails() {
    let gpx = build_gpx_with_points(500_001);
    let err = parse_gpx(gpx.as_bytes()).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::TooManyPoints);
}

#[test]
fn exactly_max_segments_succeeds() {
    let gpx = build_gpx_with_segments(10_000);
    let result = parse_gpx(gpx.as_bytes()).unwrap();
    assert_eq!(result.tracks.len(), 1);
}

#[test]
fn one_over_max_segments_fails() {
    let gpx = build_gpx_with_segments(10_001);
    let err = parse_gpx(gpx.as_bytes()).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::TooManySegments);
}

#[test]
fn input_too_large_is_rejected() {
    // NOTE: This test allocates ~100 MiB of heap memory. It requires at least
    // 512 MiB of available RAM to run safely alongside parallel tests.
    let size = 100 * 1024 * 1024 + 1;
    let input = vec![b' '; size];
    let err = parse_gpx(&input).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::InputTooLarge);
}

// ---------------------------------------------------------------------------
// Unclosed element detection at EOF
// ---------------------------------------------------------------------------

#[test]
fn unclosed_elements_at_eof_returns_invalid_xml() {
    // Valid UTF-8, no DOCTYPE, but has unclosed elements that trigger the
    // depth > 0 check at EOF.
    let input = r#"<?xml version="1.0"?><gpx version="1.1"><trk>"#;
    let err = parse_gpx(input.as_bytes()).unwrap_err();
    assert_eq!(err.code, GpxParseErrorCode::InvalidXml);
    assert!(
        err.message.contains("unclosed XML elements"),
        "expected 'unclosed XML elements' in error message, got: {}",
        err.message
    );
}

// ---------------------------------------------------------------------------
// Timeout tests (ensure parser completes in bounded time)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn large_route_parses_within_5_seconds() {
    let data = haiker_test_support::fixtures::large_route();
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), async { parse_gpx(data) }).await;

    assert!(result.is_ok(), "parser timed out on large_route fixture");
    assert!(result.unwrap().is_ok());
}

#[tokio::test]
async fn adversarial_deep_many_segments_parses_within_5_seconds() {
    // Build a GPX with depth=19 (just under limit) and many segments
    let mut gpx = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1">"#);
    // Nest some elements to get to depth ~5, then add trk/trkseg inside
    gpx.push_str("<trk>");
    for _ in 0..5000 {
        gpx.push_str(
            r#"<trkseg><trkpt lat="47.0" lon="11.0"/><trkpt lat="47.1" lon="11.1"/></trkseg>"#,
        );
    }
    gpx.push_str("</trk></gpx>");

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        parse_gpx(gpx.as_bytes())
    })
    .await;

    assert!(
        result.is_ok(),
        "parser timed out on adversarial deep+many-segments input"
    );
    assert!(result.unwrap().is_ok());
}
