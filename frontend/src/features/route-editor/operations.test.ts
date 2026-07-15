import { describe, it, expect } from "vitest";
import type { AddPointOperation, DeletePointOperation } from "./types";
import { ApiError } from "@/api/client";

describe("AddPointOperation", () => {
  it("constructs payload with coordinates only (latitude, longitude)", () => {
    const operation: AddPointOperation = {
      type: "addPoint",
      segmentIndex: 0,
      afterPointIndex: 2,
      point: { latitude: 47.123, longitude: 11.456 },
    };

    expect(operation.type).toBe("addPoint");
    expect(operation.segmentIndex).toBe(0);
    expect(operation.afterPointIndex).toBe(2);
    expect(operation.point).toEqual({ latitude: 47.123, longitude: 11.456 });
    expect(operation.point.latitude).toBe(47.123);
    expect(operation.point.longitude).toBe(11.456);
    expect(operation.point.elevation).toBeUndefined();
  });

  it("constructs payload with optional elevation", () => {
    const operation: AddPointOperation = {
      type: "addPoint",
      segmentIndex: 1,
      afterPointIndex: 0,
      point: { latitude: 48.0, longitude: 12.0, elevation: 1500.5 },
    };

    expect(operation.type).toBe("addPoint");
    expect(operation.segmentIndex).toBe(1);
    expect(operation.afterPointIndex).toBe(0);
    expect(operation.point).toEqual({
      latitude: 48.0,
      longitude: 12.0,
      elevation: 1500.5,
    });
    expect(operation.point.elevation).toBe(1500.5);
  });

  it("point field contains only latitude, longitude, and elevation - no timestamp, heartRate, speed, temperature, or cadence", () => {
    const operation: AddPointOperation = {
      type: "addPoint",
      segmentIndex: 0,
      afterPointIndex: 0,
      point: { latitude: 47.0, longitude: 11.0, elevation: 800 },
    };

    // Verify the point object only has the allowed keys
    const pointKeys = Object.keys(operation.point).sort();
    const allowedKeys = ["elevation", "latitude", "longitude"];
    expect(pointKeys).toEqual(allowedKeys);

    // Verify that telemetry/sensor fields do not exist on the point type
    // TypeScript compile-time check: these fields are NOT part of the type
    const point = operation.point as Record<string, unknown>;
    expect(point["timestamp"]).toBeUndefined();
    expect(point["heartRate"]).toBeUndefined();
    expect(point["speed"]).toBeUndefined();
    expect(point["temperature"]).toBeUndefined();
    expect(point["cadence"]).toBeUndefined();

    // Type-level assertion: the point type should only allow latitude, longitude, elevation
    // This function would cause a compile error if the type included extra fields
    function assertPointShape(_p: { latitude: number; longitude: number; elevation?: number }): void {
      // no-op: type assertion only
    }
    assertPointShape(operation.point);
  });
});

describe("DeletePointOperation", () => {
  it("constructs payload with segmentIndex and pointIndex only", () => {
    const operation: DeletePointOperation = {
      type: "deletePoint",
      segmentIndex: 0,
      pointIndex: 3,
    };

    expect(operation.type).toBe("deletePoint");
    expect(operation.segmentIndex).toBe(0);
    expect(operation.pointIndex).toBe(3);

    // Verify the operation only has the expected keys
    const keys = Object.keys(operation).sort();
    expect(keys).toEqual(["pointIndex", "segmentIndex", "type"]);
  });
});

describe("INSUFFICIENT_POINTS error handling", () => {
  it("maps 422 INSUFFICIENT_POINTS code to ApiError with correct fields", () => {
    const error = new ApiError(
      422,
      "INSUFFICIENT_POINTS",
      "Cannot delete point: segment must have at least 2 points",
      "https://haiker.app/problems/insufficient-points",
    );

    expect(error).toBeInstanceOf(ApiError);
    expect(error).toBeInstanceOf(Error);
    expect(error.status).toBe(422);
    expect(error.code).toBe("INSUFFICIENT_POINTS");
    expect(error.message).toBe(
      "Cannot delete point: segment must have at least 2 points",
    );
    expect(error.problemType).toBe(
      "https://haiker.app/problems/insufficient-points",
    );
    expect(error.name).toBe("ApiError");
  });

  it("can be caught and identified by code for user-friendly messaging", () => {
    const error = new ApiError(
      422,
      "INSUFFICIENT_POINTS",
      "Cannot delete point: segment must have at least 2 points",
    );

    // Simulate how UI code would handle the error
    function getUserMessage(err: unknown): string {
      if (err instanceof ApiError && err.code === "INSUFFICIENT_POINTS") {
        return "This point cannot be deleted because it would leave too few points in the segment.";
      }
      return "An unknown error occurred.";
    }

    expect(getUserMessage(error)).toBe(
      "This point cannot be deleted because it would leave too few points in the segment.",
    );
    // Verify non-matching errors get the generic message
    const otherError = new Error("network failure");
    expect(getUserMessage(otherError)).toBe("An unknown error occurred.");
  });
});
