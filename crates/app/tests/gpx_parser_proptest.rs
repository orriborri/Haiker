//! Property-based tests (proptest) for the GPX parser.
//!
//! Ensures the parser never panics on arbitrary or adversarial inputs.

use haiker_app::imports::gpx_parser::{parse_gpx, GpxParseErrorCode};
use proptest::prelude::*;

/// Strategy to generate a single trkpt XML element with optional ele/time.
fn trkpt_strategy() -> impl Strategy<Value = String> {
    (
        -90.0f64..=90.0f64,
        -180.0f64..=180.0f64,
        proptest::option::of(-1000.0f64..=9000.0f64),
        proptest::option::of(0u32..=1_000_000u32),
    )
        .prop_map(|(lat, lon, ele, time_offset)| {
            let mut s = format!(r#"<trkpt lat="{lat}" lon="{lon}">"#);
            if let Some(e) = ele {
                s.push_str(&format!("<ele>{e}</ele>"));
            }
            if let Some(t) = time_offset {
                s.push_str(&format!(
                    "<time>2024-01-01T{:02}:{:02}:{:02}Z</time>",
                    t / 3600 % 24,
                    t / 60 % 60,
                    t % 60
                ));
            }
            s.push_str("</trkpt>");
            s
        })
}

/// Strategy to generate a valid GPX document with random track points.
fn valid_gpx_strategy() -> impl Strategy<Value = (String, usize)> {
    proptest::collection::vec(trkpt_strategy(), 1..50).prop_map(|points| {
        let count = points.len();
        let mut gpx = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1"><trk><trkseg>"#,
        );
        for pt in &points {
            gpx.push_str(pt);
        }
        gpx.push_str("</trkseg></trk></gpx>");
        (gpx, count)
    })
}

proptest! {
    /// Valid random GPX documents should parse successfully with correct point count.
    #[test]
    fn valid_random_gpx_never_panics_and_returns_ok((gpx, expected_count) in valid_gpx_strategy()) {
        let result = parse_gpx(gpx.as_bytes());
        prop_assert!(result.is_ok(), "Expected Ok but got: {:?}", result.err());
        let parsed = result.unwrap();
        prop_assert_eq!(parsed.total_points, expected_count);
    }

    /// Arbitrary byte slices should never cause a panic.
    /// Uses up to 64 KiB to exercise parser paths beyond the initial UTF-8 gate.
    #[test]
    fn arbitrary_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..65536)) {
        let result = parse_gpx(&data);
        // Must be Ok or Err, never panic
        prop_assert!(result.is_ok() || result.is_err());
    }

    /// Out-of-range coordinates should always yield InvalidCoordinates.
    #[test]
    fn out_of_range_lat_returns_invalid_coordinates(lat in prop_oneof!(
        -1000.0f64..=-90.001,
        90.001f64..=1000.0
    )) {
        let gpx = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1"><trk><trkseg><trkpt lat="{lat}" lon="11.0"/></trkseg></trk></gpx>"#
        );
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        prop_assert_eq!(err.code, GpxParseErrorCode::InvalidCoordinates);
    }

    /// Out-of-range longitude should always yield InvalidCoordinates.
    #[test]
    fn out_of_range_lon_returns_invalid_coordinates(lon in prop_oneof!(
        -1000.0f64..=-180.001,
        180.001f64..=1000.0
    )) {
        let gpx = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1"><trk><trkseg><trkpt lat="47.0" lon="{lon}"/></trkseg></trk></gpx>"#
        );
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        prop_assert_eq!(err.code, GpxParseErrorCode::InvalidCoordinates);
    }
}
