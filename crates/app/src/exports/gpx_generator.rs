//! Pure GPX 1.1 generator for route version geometry.
//!
//! Converts domain geometry (segments of points with optional elevation)
//! into standards-compliant GPX 1.1 XML. The output is deterministic:
//! identical inputs always produce byte-for-byte identical output.
//!
//! This generator intentionally does NOT emit `<time>`, `<hr>`, `<speed>`,
//! `<cad>`, `<temp>`, or any sensor extension elements. Points that were
//! added or moved during route editing have no recorded telemetry.

use quick_xml::escape::escape;
use std::fmt::Write as FmtWrite;

/// A single point in the GPX track.
#[derive(Debug, Clone, PartialEq)]
pub struct GpxPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
}

/// Input for the GPX generator.
///
/// Contains an optional activity name and a list of segments,
/// where each segment is a list of points. This mirrors the
/// route version geometry shape (`Vec<Vec<GeometryPoint>>`).
#[derive(Debug, Clone, PartialEq)]
pub struct GpxGeneratorInput {
    pub activity_name: Option<String>,
    pub segments: Vec<Vec<GpxPoint>>,
}

/// Error type for GPX generation failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GpxGeneratorError {
    /// An internal formatting error occurred during XML generation.
    #[error("gpx generation failed: {message}")]
    GenerationFailed { message: String },
}

/// Generate GPX 1.1 XML bytes from the given input.
///
/// The output is deterministic and uses `\n` line endings.
/// Empty segments (containing zero points) are omitted from the output.
pub fn generate_gpx(input: &GpxGeneratorInput) -> Result<Vec<u8>, GpxGeneratorError> {
    let mut out = String::new();

    // XML declaration
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

    // GPX root element
    out.push_str(
        "<gpx xmlns=\"http://www.topografix.com/GPX/1/1\" version=\"1.1\" creator=\"Haiker\">\n",
    );

    // Track element
    out.push_str("<trk>\n");

    // Track name (optional, XML-escaped)
    if let Some(ref name) = input.activity_name {
        let escaped = escape(name);
        write_or_err(&mut out, |w| writeln!(w, "<name>{}</name>", escaped))?;
    }

    // Track segments (skip empty ones)
    for segment in &input.segments {
        if segment.is_empty() {
            continue;
        }

        out.push_str("<trkseg>\n");
        for point in segment {
            let lat = format_coordinate(point.latitude);
            let lon = format_coordinate(point.longitude);
            write_or_err(&mut out, |w| {
                write!(w, "<trkpt lat=\"{}\" lon=\"{}\">", lat, lon)
            })?;

            if let Some(ele) = point.elevation {
                let ele_str = format_coordinate(ele);
                write_or_err(&mut out, |w| write!(w, "<ele>{}</ele>", ele_str))?;
            }

            out.push_str("</trkpt>\n");
        }
        out.push_str("</trkseg>\n");
    }

    out.push_str("</trk>\n");
    out.push_str("</gpx>\n");

    Ok(out.into_bytes())
}

/// Format a coordinate value with at most 7 decimal places,
/// trimming trailing zeros for cleaner output while remaining deterministic.
fn format_coordinate(value: f64) -> String {
    // Format with 7 decimal places then trim trailing zeros and trailing dot.
    let formatted = format!("{:.7}", value);
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

/// Helper to write formatted content, converting fmt::Error to our error type.
fn write_or_err(
    out: &mut String,
    f: impl FnOnce(&mut String) -> std::fmt::Result,
) -> Result<(), GpxGeneratorError> {
    f(out).map_err(|e| GpxGeneratorError::GenerationFailed {
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_xml(bytes: &[u8]) -> quick_xml::Reader<&[u8]> {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        reader.config_mut().trim_text(true);
        reader
    }

    /// Verify the output is well-formed XML by parsing it completely.
    fn assert_well_formed_xml(bytes: &[u8]) {
        let mut reader = parse_xml(bytes);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("XML is not well-formed: {}", e),
            }
            buf.clear();
        }
    }

    #[test]
    fn single_segment_with_elevation() {
        let input = GpxGeneratorInput {
            activity_name: Some("Morning Hike".to_string()),
            segments: vec![vec![
                GpxPoint {
                    latitude: 47.2692000,
                    longitude: 11.3933000,
                    elevation: Some(574.0),
                },
                GpxPoint {
                    latitude: 47.2695000,
                    longitude: 11.3938000,
                    elevation: Some(578.5),
                },
            ]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result.clone()).expect("valid UTF-8");

        assert_well_formed_xml(&result);

        // Check XML declaration
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));

        // Check GPX root attributes
        assert!(xml.contains("xmlns=\"http://www.topografix.com/GPX/1/1\""));
        assert!(xml.contains("version=\"1.1\""));
        assert!(xml.contains("creator=\"Haiker\""));

        // Check track name
        assert!(xml.contains("<name>Morning Hike</name>"));

        // Check track segment
        assert!(xml.contains("<trkseg>"));
        assert!(xml.contains("</trkseg>"));

        // Check points with elevation
        assert!(xml.contains("lat=\"47.2692\""));
        assert!(xml.contains("lon=\"11.3933\""));
        assert!(xml.contains("<ele>574</ele>"));
        assert!(xml.contains("<ele>578.5</ele>"));
    }

    #[test]
    fn multiple_segments_preserves_boundaries() {
        let input = GpxGeneratorInput {
            activity_name: Some("Split Trail".to_string()),
            segments: vec![
                vec![
                    GpxPoint {
                        latitude: 46.5000,
                        longitude: 10.0000,
                        elevation: Some(1200.0),
                    },
                    GpxPoint {
                        latitude: 46.5010,
                        longitude: 10.0010,
                        elevation: Some(1220.0),
                    },
                ],
                vec![
                    GpxPoint {
                        latitude: 46.5100,
                        longitude: 10.0100,
                        elevation: Some(1300.0),
                    },
                    GpxPoint {
                        latitude: 46.5110,
                        longitude: 10.0110,
                        elevation: Some(1320.0),
                    },
                ],
                vec![GpxPoint {
                    latitude: 46.5200,
                    longitude: 10.0200,
                    elevation: Some(1400.0),
                }],
            ],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result.clone()).expect("valid UTF-8");

        assert_well_formed_xml(&result);

        // Count <trkseg> occurrences -- should be 3
        let trkseg_count = xml.matches("<trkseg>").count();
        assert_eq!(trkseg_count, 3, "expected 3 track segments");

        // Verify segment boundaries: second segment starts with 46.51
        let first_seg_end = xml.find("</trkseg>").expect("first trkseg close");
        let after_first = &xml[first_seg_end..];
        assert!(
            after_first.contains("lat=\"46.51\""),
            "second segment point should appear after first segment"
        );
    }

    #[test]
    fn points_without_elevation_emit_no_ele() {
        let input = GpxGeneratorInput {
            activity_name: None,
            segments: vec![vec![
                GpxPoint {
                    latitude: 47.0,
                    longitude: 11.0,
                    elevation: None,
                },
                GpxPoint {
                    latitude: 47.1,
                    longitude: 11.1,
                    elevation: Some(500.0),
                },
            ]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result.clone()).expect("valid UTF-8");

        assert_well_formed_xml(&result);

        // The first point should NOT have <ele>
        let first_trkpt_start = xml.find("<trkpt").expect("first trkpt");
        let first_trkpt_end = xml[first_trkpt_start..]
            .find("</trkpt>")
            .expect("first trkpt end")
            + first_trkpt_start;
        let first_trkpt = &xml[first_trkpt_start..first_trkpt_end];
        assert!(
            !first_trkpt.contains("<ele>"),
            "point without elevation should not have <ele>"
        );

        // The second point SHOULD have <ele>
        let second_trkpt_start =
            xml[first_trkpt_end..].find("<trkpt").expect("second trkpt") + first_trkpt_end;
        let second_trkpt_end = xml[second_trkpt_start..]
            .find("</trkpt>")
            .expect("second trkpt end")
            + second_trkpt_start;
        let second_trkpt = &xml[second_trkpt_start..second_trkpt_end];
        assert!(
            second_trkpt.contains("<ele>500</ele>"),
            "point with elevation should have <ele>"
        );
    }

    #[test]
    fn xml_escaping_special_characters_in_name() {
        let input = GpxGeneratorInput {
            activity_name: Some("Trail <Rock & Roll> \"best\" 'ever'".to_string()),
            segments: vec![vec![GpxPoint {
                latitude: 47.0,
                longitude: 11.0,
                elevation: None,
            }]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result.clone()).expect("valid UTF-8");

        assert_well_formed_xml(&result);

        // Verify special characters are escaped
        assert!(xml.contains("&lt;"));
        assert!(xml.contains("&gt;"));
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&quot;"));
        assert!(xml.contains("&apos;"));
        // Ensure raw characters are NOT present in the name element
        assert!(!xml.contains("<name>Trail <Rock"));
    }

    #[test]
    fn empty_segments_are_omitted() {
        let input = GpxGeneratorInput {
            activity_name: Some("Sparse".to_string()),
            segments: vec![
                vec![], // empty - should be omitted
                vec![GpxPoint {
                    latitude: 47.0,
                    longitude: 11.0,
                    elevation: None,
                }],
                vec![], // empty - should be omitted
                vec![GpxPoint {
                    latitude: 48.0,
                    longitude: 12.0,
                    elevation: None,
                }],
            ],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result.clone()).expect("valid UTF-8");

        assert_well_formed_xml(&result);

        // Only 2 non-empty segments should appear
        let trkseg_count = xml.matches("<trkseg>").count();
        assert_eq!(trkseg_count, 2, "expected 2 track segments (empty omitted)");
    }

    #[test]
    fn deterministic_output() {
        let input = GpxGeneratorInput {
            activity_name: Some("Deterministic Test".to_string()),
            segments: vec![
                vec![
                    GpxPoint {
                        latitude: 47.1234567,
                        longitude: 11.7654321,
                        elevation: Some(1234.5678),
                    },
                    GpxPoint {
                        latitude: 47.9999999,
                        longitude: 11.0000001,
                        elevation: None,
                    },
                ],
                vec![GpxPoint {
                    latitude: 48.0,
                    longitude: 12.0,
                    elevation: Some(0.0),
                }],
            ],
        };

        let result1 = generate_gpx(&input).expect("first generation");
        let result2 = generate_gpx(&input).expect("second generation");

        assert_eq!(
            result1, result2,
            "identical inputs must produce byte-for-byte identical output"
        );
    }

    #[test]
    fn coordinate_precision_at_most_7_decimal_places() {
        let input = GpxGeneratorInput {
            activity_name: None,
            segments: vec![vec![GpxPoint {
                latitude: 47.12345678901234,
                longitude: 11.98765432109876,
                elevation: Some(1234.56789012),
            }]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result).expect("valid UTF-8");

        // lat should be at most 7 decimal places: 47.1234568 (rounded)
        assert!(
            xml.contains("lat=\"47.1234568\""),
            "latitude should be formatted to at most 7 decimal places, got: {}",
            xml
        );
        // lon should be at most 7 decimal places: 11.9876543 (rounded)
        assert!(
            xml.contains("lon=\"11.9876543\""),
            "longitude should be formatted to at most 7 decimal places, got: {}",
            xml
        );
        // elevation should also be at most 7 decimal places
        assert!(
            xml.contains("<ele>1234.5678901</ele>"),
            "elevation should be at most 7 decimal places, got: {}",
            xml
        );
    }

    #[test]
    fn no_time_or_sensor_elements_in_output() {
        let input = GpxGeneratorInput {
            activity_name: Some("Sensor-free Export".to_string()),
            segments: vec![vec![
                GpxPoint {
                    latitude: 47.0,
                    longitude: 11.0,
                    elevation: Some(500.0),
                },
                GpxPoint {
                    latitude: 47.1,
                    longitude: 11.1,
                    elevation: None,
                },
            ]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result).expect("valid UTF-8");

        // None of these elements should appear
        assert!(!xml.contains("<time>"), "must not contain <time>");
        assert!(!xml.contains("<hr>"), "must not contain <hr>");
        assert!(!xml.contains("<speed>"), "must not contain <speed>");
        assert!(!xml.contains("<cad>"), "must not contain <cad>");
        assert!(!xml.contains("<temp>"), "must not contain <temp>");
        assert!(
            !xml.contains("<extensions>"),
            "must not contain <extensions>"
        );
    }

    #[test]
    fn no_activity_name_omits_name_element() {
        let input = GpxGeneratorInput {
            activity_name: None,
            segments: vec![vec![GpxPoint {
                latitude: 47.0,
                longitude: 11.0,
                elevation: None,
            }]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result.clone()).expect("valid UTF-8");

        assert_well_formed_xml(&result);
        assert!(
            !xml.contains("<name>"),
            "no name element when activity_name is None"
        );
    }

    #[test]
    fn uses_newline_line_endings_not_crlf() {
        let input = GpxGeneratorInput {
            activity_name: Some("LF Test".to_string()),
            segments: vec![vec![GpxPoint {
                latitude: 47.0,
                longitude: 11.0,
                elevation: None,
            }]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let xml = String::from_utf8(result).expect("valid UTF-8");

        assert!(!xml.contains("\r\n"), "must not contain CRLF line endings");
        assert!(xml.contains('\n'), "must use LF line endings");
    }

    #[test]
    fn deterministic_fixture() {
        let input = GpxGeneratorInput {
            activity_name: Some("Morning Hike".to_string()),
            segments: vec![vec![
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
                GpxPoint {
                    latitude: 47.2698500,
                    longitude: 11.3942100,
                    elevation: Some(582.0),
                },
            ]],
        };

        let result = generate_gpx(&input).expect("generation should succeed");
        let expected = haiker_test_support::fixtures::expected_export_simple();

        assert_eq!(
            result,
            expected,
            "generated GPX must match fixture byte-for-byte.\nGenerated:\n{}\nExpected:\n{}",
            String::from_utf8_lossy(&result),
            String::from_utf8_lossy(expected),
        );
    }
}
