//! Typed value object for corrected statistics computed from published geometry.
//!
//! CorrectedStatistics captures deterministic measurements derived from the
//! corrected route geometry during publication. These are separate from the
//! original recorded statistics captured by the device.

use serde::{Deserialize, Serialize};

use crate::recorded_activity::Coordinate;

/// The current calculation algorithm version.
pub const CALCULATION_VERSION: &str = "v1.0";

/// Statistics computed from the corrected (published) route geometry.
///
/// These values are deterministically reproducible from the geometry and
/// the algorithm version. They are stored alongside the route version and
/// never overwrite recorded statistics from the device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectedStatistics {
    /// Total distance in meters computed via haversine over the geometry.
    pub distance_meters: f64,
    /// Number of coordinate points in the geometry.
    pub point_count: u32,
    /// Identifies the algorithm version used to compute these statistics.
    pub calculation_version: String,
}

impl CorrectedStatistics {
    /// Create a new CorrectedStatistics with explicit values.
    pub fn new(distance_meters: f64, point_count: u32, calculation_version: String) -> Self {
        Self {
            distance_meters,
            point_count,
            calculation_version,
        }
    }

    /// Calculate corrected statistics from a geometry (ordered list of coordinates).
    ///
    /// Uses the haversine formula to compute total distance. The result is
    /// deterministic for the same input coordinates within f64 precision.
    pub fn calculate_from_geometry(coords: &[Coordinate]) -> Self {
        let distance_meters = compute_total_distance(coords);
        let point_count = coords.len() as u32;

        Self {
            distance_meters,
            point_count,
            calculation_version: CALCULATION_VERSION.to_string(),
        }
    }
}

/// Compute total distance in meters using haversine formula.
fn compute_total_distance(coords: &[Coordinate]) -> f64 {
    if coords.len() < 2 {
        return 0.0;
    }

    let mut total = 0.0;
    for window in coords.windows(2) {
        total += haversine_distance(&window[0], &window[1]);
    }
    total
}

/// Haversine distance between two coordinates in meters.
fn haversine_distance(a: &Coordinate, b: &Coordinate) -> f64 {
    let r = 6_371_000.0; // Earth radius in meters
    let lat1 = a.latitude.to_radians();
    let lat2 = b.latitude.to_radians();
    let dlat = (b.latitude - a.latitude).to_radians();
    let dlon = (b.longitude - a.longitude).to_radians();

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(lat: f64, lon: f64) -> Coordinate {
        Coordinate::new(lat, lon).unwrap()
    }

    #[test]
    fn calculate_from_geometry_empty_coords() {
        let stats = CorrectedStatistics::calculate_from_geometry(&[]);
        assert_eq!(stats.distance_meters, 0.0);
        assert_eq!(stats.point_count, 0);
        assert_eq!(stats.calculation_version, CALCULATION_VERSION);
    }

    #[test]
    fn calculate_from_geometry_single_point() {
        let coords = vec![coord(47.0, 11.0)];
        let stats = CorrectedStatistics::calculate_from_geometry(&coords);
        assert_eq!(stats.distance_meters, 0.0);
        assert_eq!(stats.point_count, 1);
    }

    #[test]
    fn calculate_from_geometry_two_points() {
        let coords = vec![coord(47.0, 11.0), coord(47.1, 11.1)];
        let stats = CorrectedStatistics::calculate_from_geometry(&coords);
        assert_eq!(stats.point_count, 2);
        // Distance should be positive and reasonable (approx 13-14 km between these points)
        assert!(stats.distance_meters > 10_000.0);
        assert!(stats.distance_meters < 20_000.0);
    }

    #[test]
    fn calculate_from_geometry_is_deterministic() {
        let coords = vec![coord(47.0, 11.0), coord(47.1, 11.1), coord(47.2, 11.2)];

        let stats1 = CorrectedStatistics::calculate_from_geometry(&coords);
        let stats2 = CorrectedStatistics::calculate_from_geometry(&coords);

        assert_eq!(stats1.distance_meters, stats2.distance_meters);
        assert_eq!(stats1.point_count, stats2.point_count);
        assert_eq!(stats1.calculation_version, stats2.calculation_version);
    }

    #[test]
    fn calculate_from_geometry_multi_point_sums_segments() {
        let coords = vec![coord(47.0, 11.0), coord(47.1, 11.1), coord(47.2, 11.2)];

        let stats = CorrectedStatistics::calculate_from_geometry(&coords);
        assert_eq!(stats.point_count, 3);
        // Total distance should be roughly 2x the two-point distance
        assert!(stats.distance_meters > 20_000.0);
        assert!(stats.distance_meters < 40_000.0);
    }

    #[test]
    fn calculation_version_is_v1() {
        let coords = vec![coord(47.0, 11.0), coord(47.1, 11.1)];
        let stats = CorrectedStatistics::calculate_from_geometry(&coords);
        assert_eq!(stats.calculation_version, "v1.0");
    }

    #[test]
    fn new_creates_with_explicit_values() {
        let stats = CorrectedStatistics::new(1234.5, 10, "v2.0".to_string());
        assert_eq!(stats.distance_meters, 1234.5);
        assert_eq!(stats.point_count, 10);
        assert_eq!(stats.calculation_version, "v2.0");
    }

    #[test]
    fn serialization_roundtrip() {
        let stats = CorrectedStatistics::new(5678.9, 42, "v1.0".to_string());
        let json = serde_json::to_value(&stats).unwrap();

        assert_eq!(json["distance_meters"], 5678.9);
        assert_eq!(json["point_count"], 42);
        assert_eq!(json["calculation_version"], "v1.0");

        let deserialized: CorrectedStatistics = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, stats);
    }

    #[test]
    fn reproducible_within_numeric_tolerance() {
        // Run the same calculation many times and verify exact bit-for-bit equality.
        // IEEE 754 operations on the same inputs must produce the same output.
        let coords = vec![
            coord(48.2082, 16.3738), // Vienna
            coord(47.2692, 11.4041), // Innsbruck
            coord(47.8095, 13.0550), // Salzburg
        ];

        let reference = CorrectedStatistics::calculate_from_geometry(&coords);
        for _ in 0..100 {
            let result = CorrectedStatistics::calculate_from_geometry(&coords);
            assert_eq!(result.distance_meters, reference.distance_meters);
        }
    }
}
