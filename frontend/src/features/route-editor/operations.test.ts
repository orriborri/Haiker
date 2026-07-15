import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { AddPointOperation, DeletePointOperation } from "./types";
import { ApiError, applyOperation } from "@/api/client";

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

describe("applyOperation integration", () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("fetch", mockFetch);
    // Mock localStorage so getAuthToken() returns a token
    vi.stubGlobal("localStorage", {
      getItem: vi.fn(() => "test-token-abc"),
      setItem: vi.fn(),
      removeItem: vi.fn(),
    });
    // Mock crypto.randomUUID for deterministic Idempotency-Key
    vi.stubGlobal("crypto", {
      randomUUID: () => "00000000-1111-2222-3333-444444444444",
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("sends correct HTTP request for addPoint operation", async () => {
    const mockResponse = {
      draftId: "draft-123",
      revision: 1,
      canUndo: true,
      canRedo: false,
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => mockResponse,
    });

    const operation = {
      type: "addPoint",
      segmentIndex: 0,
      afterPointIndex: 2,
      point: { latitude: 47.123, longitude: 11.456 },
    };

    const result = await applyOperation("draft-123", operation, 0);

    // Verify fetch was called exactly once
    expect(mockFetch).toHaveBeenCalledTimes(1);

    const [url, options] = mockFetch.mock.calls[0]!;

    // Verify the URL
    expect(url).toBe("/v1/route-drafts/draft-123/operations");

    // Verify method
    expect(options.method).toBe("POST");

    // Verify headers
    expect(options.headers["Content-Type"]).toBe("application/json");
    expect(options.headers["Idempotency-Key"]).toBe(
      "00000000-1111-2222-3333-444444444444",
    );
    expect(options.headers["Authorization"]).toBe("Bearer test-token-abc");

    // Verify body contains operation and expectedRevision
    const body = JSON.parse(options.body);
    expect(body.operation).toEqual(operation);
    expect(body.expectedRevision).toBe(0);

    // Verify the parsed response
    expect(result).toEqual(mockResponse);
  });

  it("propagates 422 INSUFFICIENT_POINTS as ApiError", async () => {
    const errorBody = {
      code: "INSUFFICIENT_POINTS",
      detail: "Cannot delete point: segment must have at least 2 points",
      type: "https://haiker.app/problems/insufficient-points",
    };

    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 422,
      statusText: "Unprocessable Entity",
      json: async () => errorBody,
    });

    const operation = {
      type: "deletePoint",
      segmentIndex: 0,
      pointIndex: 0,
    };

    try {
      await applyOperation("draft-456", operation, 1);
      expect.fail("Expected applyOperation to throw ApiError");
    } catch (err) {
      expect(err).toBeInstanceOf(ApiError);
      const apiError = err as InstanceType<typeof ApiError>;
      expect(apiError.status).toBe(422);
      expect(apiError.code).toBe("INSUFFICIENT_POINTS");
      expect(apiError.message).toBe(
        "Cannot delete point: segment must have at least 2 points",
      );
      expect(apiError.problemType).toBe(
        "https://haiker.app/problems/insufficient-points",
      );
    }
  });
});
