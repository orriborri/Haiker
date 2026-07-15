import { describe, it, expect } from "vitest";
import { haversineDistance, formatDistance, polylineDistance, MAX_REPLACEMENT_POINTS } from "./geo-utils";

describe("haversineDistance", () => {
  it("returns 0 for identical points", () => {
    expect(haversineDistance(47.0, 11.0, 47.0, 11.0)).toBe(0);
  });

  it("calculates short distance correctly (< 1 km)", () => {
    // Approximately 100m apart along latitude at equator
    const d = haversineDistance(0.0, 0.0, 0.0009, 0.0);
    expect(d).toBeGreaterThan(90);
    expect(d).toBeLessThan(110);
  });

  it("calculates medium distance correctly (a few km)", () => {
    // London to a nearby point ~10 km
    const d = haversineDistance(51.5074, -0.1278, 51.5974, -0.1278);
    expect(d).toBeGreaterThan(9_500);
    expect(d).toBeLessThan(10_500);
  });

  it("calculates long distance correctly (London to Paris ~340 km)", () => {
    const d = haversineDistance(51.5074, -0.1278, 48.8566, 2.3522);
    expect(d).toBeGreaterThan(330_000);
    expect(d).toBeLessThan(350_000);
  });

  it("handles negative coordinates", () => {
    const d = haversineDistance(-33.8688, 151.2093, -37.8136, 144.9631);
    // Sydney to Melbourne ~714 km
    expect(d).toBeGreaterThan(700_000);
    expect(d).toBeLessThan(730_000);
  });

  it("handles antipodal points", () => {
    // North pole to south pole
    const d = haversineDistance(90, 0, -90, 0);
    // Should be approximately half the earth circumference (20,015 km)
    expect(d).toBeGreaterThan(20_000_000);
    expect(d).toBeLessThan(20_100_000);
  });

  it("is symmetric", () => {
    const d1 = haversineDistance(47.0, 11.0, 48.0, 12.0);
    const d2 = haversineDistance(48.0, 12.0, 47.0, 11.0);
    expect(d1).toBeCloseTo(d2, 6);
  });
});

describe("formatDistance", () => {
  it("formats distances below 1000m in meters", () => {
    expect(formatDistance(42)).toBe("42 m");
  });

  it("rounds sub-kilometer distances to whole meters", () => {
    expect(formatDistance(123.7)).toBe("124 m");
  });

  it("formats 0 meters", () => {
    expect(formatDistance(0)).toBe("0 m");
  });

  it("formats distances at or above 1000m in km with one decimal", () => {
    expect(formatDistance(1000)).toBe("1.0 km");
    expect(formatDistance(1234)).toBe("1.2 km");
    expect(formatDistance(15678)).toBe("15.7 km");
  });
});

describe("polylineDistance", () => {
  it("returns 0 for empty array", () => {
    expect(polylineDistance([])).toBe(0);
  });

  it("returns 0 for a single point", () => {
    expect(polylineDistance([{ latitude: 47.0, longitude: 11.0 }])).toBe(0);
  });

  it("returns haversine distance for two points", () => {
    const points = [
      { latitude: 47.0, longitude: 11.0 },
      { latitude: 48.0, longitude: 12.0 },
    ];
    const expected = haversineDistance(47.0, 11.0, 48.0, 12.0);
    expect(polylineDistance(points)).toBeCloseTo(expected, 6);
  });

  it("sums distances between consecutive points", () => {
    const points = [
      { latitude: 47.0, longitude: 11.0 },
      { latitude: 47.5, longitude: 11.5 },
      { latitude: 48.0, longitude: 12.0 },
    ];
    const d1 = haversineDistance(47.0, 11.0, 47.5, 11.5);
    const d2 = haversineDistance(47.5, 11.5, 48.0, 12.0);
    expect(polylineDistance(points)).toBeCloseTo(d1 + d2, 6);
  });

  it("handles three identical points (zero distance)", () => {
    const points = [
      { latitude: 47.0, longitude: 11.0 },
      { latitude: 47.0, longitude: 11.0 },
      { latitude: 47.0, longitude: 11.0 },
    ];
    expect(polylineDistance(points)).toBe(0);
  });

  it("calculates a realistic multi-point trail distance", () => {
    // A short trail with 4 waypoints
    const points = [
      { latitude: 47.2692, longitude: 11.3927 },
      { latitude: 47.2700, longitude: 11.3940 },
      { latitude: 47.2710, longitude: 11.3960 },
      { latitude: 47.2720, longitude: 11.3980 },
    ];
    const total = polylineDistance(points);
    // Should be a few hundred meters
    expect(total).toBeGreaterThan(100);
    expect(total).toBeLessThan(1000);
  });
});

describe("MAX_REPLACEMENT_POINTS", () => {
  it("is set to 500", () => {
    expect(MAX_REPLACEMENT_POINTS).toBe(500);
  });
});
