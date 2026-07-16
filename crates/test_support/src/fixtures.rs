//! Test fixture loading utilities.
//!
//! Provides access to immutable GPX test fixtures embedded at compile time
//! using `include_bytes!`. Each fixture covers a specific test scenario.

/// Valid GPX 1.1 file with a single track, single segment, and 10 points
/// including elevation and timestamps.
pub fn valid_simple() -> &'static [u8] {
    include_bytes!("../fixtures/valid_simple.gpx")
}

/// Valid GPX 1.0 format file with 5 points.
pub fn valid_gpx10() -> &'static [u8] {
    include_bytes!("../fixtures/valid_gpx10.gpx")
}

/// Valid GPX 1.1 file with multiple track segments (3 segments).
pub fn valid_multi_segment() -> &'static [u8] {
    include_bytes!("../fixtures/valid_multi_segment.gpx")
}

/// Valid GPX 1.1 file with points that have no elevation elements.
pub fn valid_no_elevation() -> &'static [u8] {
    include_bytes!("../fixtures/valid_no_elevation.gpx")
}

/// Valid GPX 1.1 file with points that have no time elements.
pub fn valid_no_timestamps() -> &'static [u8] {
    include_bytes!("../fixtures/valid_no_timestamps.gpx")
}

/// Valid GPX 1.1 file with non-ASCII metadata (German and Japanese characters).
pub fn valid_non_ascii() -> &'static [u8] {
    include_bytes!("../fixtures/valid_non_ascii.gpx")
}

/// Generated GPX 1.1 file with 1000 track points for performance testing.
pub fn large_route() -> &'static [u8] {
    include_bytes!("../fixtures/large_route.gpx")
}

/// Malformed XML that is not well-formed (missing closing tags).
pub fn malformed_xml() -> &'static [u8] {
    include_bytes!("../fixtures/malformed_xml.gpx")
}

/// GPX file with out-of-range coordinates (lat=999).
pub fn invalid_coordinates() -> &'static [u8] {
    include_bytes!("../fixtures/invalid_coordinates.gpx")
}

/// GPX file with DOCTYPE and external entity reference (XXE attack attempt).
pub fn xxe_attack() -> &'static [u8] {
    include_bytes!("../fixtures/xxe_attack.gpx")
}

/// GPX file with deeply nested XML elements exceeding the depth limit.
pub fn excessive_depth() -> &'static [u8] {
    include_bytes!("../fixtures/excessive_depth.gpx")
}

/// Expected GPX export output for a simple known input (3 points, single segment,
/// with elevation, activity name "Morning Hike"). Used for deterministic fixture testing.
pub fn expected_export_simple() -> &'static [u8] {
    include_bytes!("../fixtures/expected_export_simple.gpx")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_fixtures_load_successfully() {
        assert!(!valid_simple().is_empty());
        assert!(!valid_gpx10().is_empty());
        assert!(!valid_multi_segment().is_empty());
        assert!(!valid_no_elevation().is_empty());
        assert!(!valid_no_timestamps().is_empty());
        assert!(!valid_non_ascii().is_empty());
        assert!(!large_route().is_empty());
        assert!(!malformed_xml().is_empty());
        assert!(!invalid_coordinates().is_empty());
        assert!(!xxe_attack().is_empty());
        assert!(!excessive_depth().is_empty());
        assert!(!expected_export_simple().is_empty());
    }

    #[test]
    fn valid_simple_is_valid_utf8() {
        let content = std::str::from_utf8(valid_simple()).unwrap();
        assert!(content.contains("Morning Hike"));
        assert!(content.contains("version=\"1.1\""));
    }

    #[test]
    fn valid_gpx10_contains_version_marker() {
        let content = std::str::from_utf8(valid_gpx10()).unwrap();
        assert!(content.contains("version=\"1.0\""));
    }

    #[test]
    fn valid_non_ascii_contains_unicode() {
        let content = std::str::from_utf8(valid_non_ascii()).unwrap();
        assert!(content.contains("Bodensee"));
        assert!(content.contains("Fuji-san"));
    }
}
