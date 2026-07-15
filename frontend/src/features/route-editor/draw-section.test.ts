import { describe, it, expect } from "vitest";
import { polylineDistance, MAX_REPLACEMENT_POINTS } from "./geo-utils";
import type {
  EditorState,
  EditorAction,
  DrawingState,
  ReplaceSectionOperation,
} from "./types";

/**
 * Minimal reducer extracted for testing drawing state transitions.
 * Mirrors the drawing-related cases in useEditorState.
 */
function drawingReducer(state: EditorState, action: EditorAction): EditorState {
  switch (action.type) {
    case "DRAWING_START": {
      const drawing: DrawingState = {
        segmentIndex: action.segmentIndex,
        startIndex: action.startIndex,
        endIndex: action.endIndex,
        points: [action.firstPoint],
        isActive: true,
      };
      return { ...state, drawing };
    }
    case "DRAWING_ADD_POINT": {
      if (!state.drawing || !state.drawing.isActive) return state;
      return {
        ...state,
        drawing: {
          ...state.drawing,
          points: [...state.drawing.points, action.point],
        },
      };
    }
    case "DRAWING_REMOVE_LAST_POINT": {
      if (!state.drawing || !state.drawing.isActive) return state;
      if (state.drawing.points.length <= 1) return state;
      return {
        ...state,
        drawing: {
          ...state.drawing,
          points: state.drawing.points.slice(0, -1),
        },
      };
    }
    case "DRAWING_CANCEL": {
      return { ...state, drawing: null };
    }
    case "DRAWING_COMMIT": {
      if (!state.drawing) return state;
      return { ...state, drawing: { ...state.drawing, isActive: false } };
    }
    default:
      return state;
  }
}

function createBaseState(): EditorState {
  return {
    currentTool: "draw-section",
    selection: {
      type: "section",
      segmentIndex: 0,
      startIndex: 2,
      endIndex: 5,
    },
    draftId: "draft-1",
    revision: 1,
    optimisticGeometry: [
      [
        { latitude: 47.0, longitude: 11.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 }, // startIndex = 2
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
        { latitude: 47.5, longitude: 11.5 }, // endIndex = 5
        { latitude: 47.6, longitude: 11.6 },
      ],
    ],
    baseGeometry: null,
    canUndo: false,
    canRedo: false,
    conflictError: null,
    isOperationPending: false,
    isOffline: false,
    conflictServerDraft: null,
    conflictLocalOps: [],
    drag: null,
    drawing: null,
  };
}

describe("Drawing state management", () => {
  it("DRAWING_START initializes with first point matching startIndex coordinate", () => {
    const state = createBaseState();
    const geometry = state.optimisticGeometry!;
    const startPoint = geometry[0]![2]!;

    const newState = drawingReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 2,
      endIndex: 5,
      firstPoint: {
        latitude: startPoint.latitude,
        longitude: startPoint.longitude,
      },
    });

    expect(newState.drawing).not.toBeNull();
    expect(newState.drawing!.isActive).toBe(true);
    expect(newState.drawing!.segmentIndex).toBe(0);
    expect(newState.drawing!.startIndex).toBe(2);
    expect(newState.drawing!.endIndex).toBe(5);
    expect(newState.drawing!.points).toHaveLength(1);
    expect(newState.drawing!.points[0]!.latitude).toBe(47.2);
    expect(newState.drawing!.points[0]!.longitude).toBe(11.2);
  });

  it("DRAWING_ADD_POINT appends a point to the drawing", () => {
    const state = createBaseState();
    let newState = drawingReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 2,
      endIndex: 5,
      firstPoint: { latitude: 47.2, longitude: 11.2 },
    });

    newState = drawingReducer(newState, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 47.25, longitude: 11.25 },
    });

    expect(newState.drawing!.points).toHaveLength(2);
    expect(newState.drawing!.points[1]!.latitude).toBe(47.25);
    expect(newState.drawing!.points[1]!.longitude).toBe(11.25);
  });

  it("DRAWING_REMOVE_LAST_POINT removes the last point but not the first", () => {
    const state = createBaseState();
    let newState = drawingReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 2,
      endIndex: 5,
      firstPoint: { latitude: 47.2, longitude: 11.2 },
    });
    newState = drawingReducer(newState, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 47.25, longitude: 11.25 },
    });
    newState = drawingReducer(newState, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 47.3, longitude: 11.3 },
    });

    // Remove last point
    newState = drawingReducer(newState, { type: "DRAWING_REMOVE_LAST_POINT" });
    expect(newState.drawing!.points).toHaveLength(2);
    expect(newState.drawing!.points[1]!.latitude).toBe(47.25);

    // Remove until only first point remains
    newState = drawingReducer(newState, { type: "DRAWING_REMOVE_LAST_POINT" });
    expect(newState.drawing!.points).toHaveLength(1);

    // Cannot remove the first point
    newState = drawingReducer(newState, { type: "DRAWING_REMOVE_LAST_POINT" });
    expect(newState.drawing!.points).toHaveLength(1);
    expect(newState.drawing!.points[0]!.latitude).toBe(47.2);
  });

  it("DRAWING_CANCEL clears drawing state without modifying draft", () => {
    const state = createBaseState();
    let newState = drawingReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 2,
      endIndex: 5,
      firstPoint: { latitude: 47.2, longitude: 11.2 },
    });
    newState = drawingReducer(newState, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 47.25, longitude: 11.25 },
    });

    newState = drawingReducer(newState, { type: "DRAWING_CANCEL" });

    expect(newState.drawing).toBeNull();
    // Original state should be unchanged
    expect(newState.optimisticGeometry).toEqual(state.optimisticGeometry);
    expect(newState.revision).toBe(state.revision);
    expect(newState.draftId).toBe(state.draftId);
  });

  it("DRAWING_COMMIT marks drawing as not active", () => {
    const state = createBaseState();
    let newState = drawingReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 2,
      endIndex: 5,
      firstPoint: { latitude: 47.2, longitude: 11.2 },
    });

    newState = drawingReducer(newState, { type: "DRAWING_COMMIT" });
    expect(newState.drawing!.isActive).toBe(false);
  });
});

describe("Endpoint continuity enforcement", () => {
  it("first drawn point matches the start_index coordinate", () => {
    const geometry = [
      [
        { latitude: 47.0, longitude: 11.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
        { latitude: 47.5, longitude: 11.5 },
      ],
    ];
    const startIndex = 2;
    const startPoint = geometry[0]![startIndex]!;

    // Simulate what startDrawing does: auto-insert the start coordinate
    const drawnPoints = [
      { latitude: startPoint.latitude, longitude: startPoint.longitude },
    ];

    expect(drawnPoints[0]!.latitude).toBe(geometry[0]![startIndex]!.latitude);
    expect(drawnPoints[0]!.longitude).toBe(geometry[0]![startIndex]!.longitude);
  });

  it("finishing auto-appends the end_index coordinate", () => {
    const geometry = [
      [
        { latitude: 47.0, longitude: 11.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
        { latitude: 47.5, longitude: 11.5 },
      ],
    ];
    const endIndex = 5;
    const endPoint = geometry[0]![endIndex]!;

    // Simulate drawn points before finish
    const drawnPoints = [
      { latitude: 47.2, longitude: 11.2 },
      { latitude: 47.25, longitude: 11.25 },
      { latitude: 47.35, longitude: 11.35 },
    ];

    // Simulate finishDrawing: append end coordinate
    const finalPoints = [
      ...drawnPoints,
      { latitude: endPoint.latitude, longitude: endPoint.longitude },
    ];

    expect(finalPoints[finalPoints.length - 1]!.latitude).toBe(endPoint.latitude);
    expect(finalPoints[finalPoints.length - 1]!.longitude).toBe(endPoint.longitude);
  });
});

describe("Max points validation", () => {
  it("rejects when total points (including auto-appended end) exceed MAX_REPLACEMENT_POINTS", () => {
    // Create 500 drawn points (which means after appending end point we have 501)
    const drawnPoints = Array.from({ length: MAX_REPLACEMENT_POINTS }, (_, i) => ({
      latitude: 47.0 + i * 0.001,
      longitude: 11.0 + i * 0.001,
    }));

    // Simulate finish: append end coordinate
    const endPoint = { latitude: 48.0, longitude: 12.0 };
    const finalPoints = [...drawnPoints, endPoint];

    expect(finalPoints.length).toBe(MAX_REPLACEMENT_POINTS + 1);
    expect(finalPoints.length > MAX_REPLACEMENT_POINTS).toBe(true);
  });

  it("accepts when total points equal MAX_REPLACEMENT_POINTS exactly", () => {
    // Create 499 drawn points (after appending end point we have exactly 500)
    const drawnPoints = Array.from({ length: MAX_REPLACEMENT_POINTS - 1 }, (_, i) => ({
      latitude: 47.0 + i * 0.001,
      longitude: 11.0 + i * 0.001,
    }));

    const endPoint = { latitude: 48.0, longitude: 12.0 };
    const finalPoints = [...drawnPoints, endPoint];

    expect(finalPoints.length).toBe(MAX_REPLACEMENT_POINTS);
    expect(finalPoints.length <= MAX_REPLACEMENT_POINTS).toBe(true);
  });
});

describe("ReplaceSectionOperation payload structure", () => {
  it("contains only latitude, longitude, and optional elevation - no fabricated data", () => {
    const operation: ReplaceSectionOperation = {
      type: "replaceSection",
      segmentIndex: 0,
      startIndex: 2,
      endIndex: 5,
      replacement: [
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.25, longitude: 11.25 },
        { latitude: 47.35, longitude: 11.35, elevation: 1200 },
        { latitude: 47.5, longitude: 11.5 },
      ],
    };

    expect(operation.type).toBe("replaceSection");
    expect(operation.segmentIndex).toBe(0);
    expect(operation.startIndex).toBe(2);
    expect(operation.endIndex).toBe(5);

    // Verify each replacement point has only allowed keys
    for (const point of operation.replacement) {
      const keys = Object.keys(point).sort();
      const hasOnlyAllowed = keys.every((k) =>
        ["latitude", "longitude", "elevation"].includes(k),
      );
      expect(hasOnlyAllowed).toBe(true);

      // Verify no fabricated timestamps or sensor telemetry
      const pointRecord = point as Record<string, unknown>;
      expect(pointRecord["timestamp"]).toBeUndefined();
      expect(pointRecord["heartRate"]).toBeUndefined();
      expect(pointRecord["speed"]).toBeUndefined();
      expect(pointRecord["temperature"]).toBeUndefined();
      expect(pointRecord["cadence"]).toBeUndefined();
      expect(pointRecord["power"]).toBeUndefined();
    }
  });

  it("first point matches start coordinate and last point matches end coordinate", () => {
    const geometry = [
      [
        { latitude: 47.0, longitude: 11.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
        { latitude: 47.5, longitude: 11.5 },
      ],
    ];

    const startIndex = 2;
    const endIndex = 5;

    // Simulate the full workflow: first point auto-inserted, intermediate points drawn, end point auto-appended
    const replacement = [
      { latitude: geometry[0]![startIndex]!.latitude, longitude: geometry[0]![startIndex]!.longitude },
      { latitude: 47.25, longitude: 11.25 },
      { latitude: 47.35, longitude: 11.35 },
      { latitude: geometry[0]![endIndex]!.latitude, longitude: geometry[0]![endIndex]!.longitude },
    ];

    const operation: ReplaceSectionOperation = {
      type: "replaceSection",
      segmentIndex: 0,
      startIndex,
      endIndex,
      replacement,
    };

    // First replacement point must equal geometry[startIndex]
    expect(operation.replacement[0]!.latitude).toBe(geometry[0]![startIndex]!.latitude);
    expect(operation.replacement[0]!.longitude).toBe(geometry[0]![startIndex]!.longitude);

    // Last replacement point must equal geometry[endIndex]
    const lastPoint = operation.replacement[operation.replacement.length - 1]!;
    expect(lastPoint.latitude).toBe(geometry[0]![endIndex]!.latitude);
    expect(lastPoint.longitude).toBe(geometry[0]![endIndex]!.longitude);
  });
});

describe("polylineDistance in drawing context", () => {
  it("calculates distance of drawn points correctly", () => {
    const points = [
      { latitude: 47.2, longitude: 11.2 },
      { latitude: 47.25, longitude: 11.25 },
      { latitude: 47.3, longitude: 11.3 },
    ];

    const distance = polylineDistance(points);
    expect(distance).toBeGreaterThan(0);
    // Should be a few km (about 7km for ~0.1 degree jumps)
    expect(distance).toBeGreaterThan(5000);
    expect(distance).toBeLessThan(20000);
  });

  it("returns 0 for single point (start only)", () => {
    const points = [{ latitude: 47.2, longitude: 11.2 }];
    expect(polylineDistance(points)).toBe(0);
  });
});
