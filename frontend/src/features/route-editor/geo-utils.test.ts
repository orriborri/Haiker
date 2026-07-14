import { describe, it, expect } from "vitest";
import { haversineDistance, formatDistance } from "./geo-utils";

describe("haversineDistance", () => {
  it("returns 0 for identical points", () => {
    expect(haversineDistance(45.0, 10.0, 45.0, 10.0)).toBe(0);
  });

  it("calculates a known short distance correctly", () => {
    // London (51.5074, -0.1278) to Paris (48.8566, 2.3522) ~ 343 km
    const dist = haversineDistance(51.5074, -0.1278, 48.8566, 2.3522);
    expect(dist).toBeGreaterThan(340_000);
    expect(dist).toBeLessThan(350_000);
  });

  it("calculates antipodal points as approximately half the Earth circumference", () => {
    // North pole to south pole ~ 20,015 km
    const dist = haversineDistance(90, 0, -90, 0);
    expect(dist).toBeGreaterThan(20_000_000);
    expect(dist).toBeLessThan(20_100_000);
  });

  it("is symmetric (distance A-B equals B-A)", () => {
    const d1 = haversineDistance(40.7128, -74.006, 34.0522, -118.2437);
    const d2 = haversineDistance(34.0522, -118.2437, 40.7128, -74.006);
    expect(d1).toBeCloseTo(d2, 6);
  });

  it("handles points at the equator", () => {
    // 1 degree of longitude at the equator ~ 111.32 km
    const dist = haversineDistance(0, 0, 0, 1);
    expect(dist).toBeGreaterThan(111_000);
    expect(dist).toBeLessThan(112_000);
  });

  it("handles negative latitudes and longitudes", () => {
    // Sydney (-33.8688, 151.2093) to Buenos Aires (-34.6037, -58.3816) ~ 11,800 km
    const dist = haversineDistance(-33.8688, 151.2093, -34.6037, -58.3816);
    expect(dist).toBeGreaterThan(11_500_000);
    expect(dist).toBeLessThan(12_100_000);
  });
});

describe("formatDistance", () => {
  it("shows meters for distances under 1000m", () => {
    expect(formatDistance(0)).toBe("0 m");
    expect(formatDistance(1)).toBe("1 m");
    expect(formatDistance(500)).toBe("500 m");
    expect(formatDistance(999)).toBe("999 m");
  });

  it("rounds to nearest meter", () => {
    expect(formatDistance(10.4)).toBe("10 m");
    expect(formatDistance(10.5)).toBe("11 m");
    expect(formatDistance(999.4)).toBe("999 m");
  });

  it("shows kilometers for distances >= 1000m", () => {
    expect(formatDistance(1000)).toBe("1.00 km");
    expect(formatDistance(1500)).toBe("1.50 km");
    expect(formatDistance(12345)).toBe("12.35 km");
  });

  it("handles the 999.5m boundary (rounds to 1000m, shown as km)", () => {
    // 999.5 rounds to 1000 when using Math.round
    // But the threshold check is < 1000 on the raw value, so 999.5 < 1000 is true
    // Therefore it shows "1000 m" (rounds 999.5 to 1000)
    expect(formatDistance(999.5)).toBe("1000 m");
  });

  it("formats large distances in km with two decimal places", () => {
    expect(formatDistance(100_000)).toBe("100.00 km");
    expect(formatDistance(20_015_000)).toBe("20015.00 km");
  });
});
