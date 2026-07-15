/**
 * Haversine distance calculation and formatting utilities for the route editor.
 */

const EARTH_RADIUS_METERS = 6_371_000;

/** Maximum number of points allowed in a replacement section */
export const MAX_REPLACEMENT_POINTS = 500;

/** Convert degrees to radians */
function toRadians(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

/**
 * Calculate the haversine (great-circle) distance in meters between two points
 * given as [latitude, longitude] in degrees.
 */
export function haversineDistance(
  lat1: number,
  lon1: number,
  lat2: number,
  lon2: number,
): number {
  const dLat = toRadians(lat2 - lat1);
  const dLon = toRadians(lon2 - lon1);
  const a =
    Math.sin(dLat / 2) * Math.sin(dLat / 2) +
    Math.cos(toRadians(lat1)) *
      Math.cos(toRadians(lat2)) *
      Math.sin(dLon / 2) *
      Math.sin(dLon / 2);
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return EARTH_RADIUS_METERS * c;
}

/**
 * Calculate the total distance of a polyline (sequence of points) in meters.
 * Sums haversine distance between consecutive points.
 */
export function polylineDistance(
  points: Array<{ latitude: number; longitude: number }>,
): number {
  let total = 0;
  for (let i = 0; i < points.length - 1; i++) {
    const current = points[i]!;
    const next = points[i + 1]!;
    total += haversineDistance(
      current.latitude,
      current.longitude,
      next.latitude,
      next.longitude,
    );
  }
  return total;
}

/**
 * Format a distance in meters to a human-readable string.
 * - Distances < 1000m are shown as whole meters (e.g. "42 m")
 * - Distances >= 1000m are shown in km with one decimal (e.g. "1.2 km")
 */
export function formatDistance(meters: number): string {
  if (meters < 1000) {
    return `${Math.round(meters)} m`;
  }
  return `${(meters / 1000).toFixed(1)} km`;
}
