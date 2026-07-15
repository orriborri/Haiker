import { describe, it, expect } from "vitest";
import { editorReducer, initialState } from "./useEditorState";
import type { EditorState, EditorAction, PendingOperation } from "./types";
import type { RouteDraftResponse } from "@/api/client";

/**
 * Helper to dispatch a sequence of actions and return the final state.
 */
function applyActions(state: EditorState, actions: EditorAction[]): EditorState {
  return actions.reduce((s, a) => editorReducer(s, a), state);
}

/**
 * Creates a base state with a loaded draft, useful for most journey tests.
 */
function createLoadedState(): EditorState {
  return editorReducer(initialState, {
    type: "SET_DRAFT",
    draftId: "draft-abc",
    revision: 0,
    geometry: [
      [
        { latitude: 47.0, longitude: 11.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
      ],
    ],
    baseGeometry: [
      [
        { latitude: 47.0, longitude: 11.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
      ],
    ],
  });
}

describe("Point correction journey (drag flow)", () => {
  it("SET_DRAFT -> DRAG_START -> DRAG_PREVIEW -> DRAG_END updates optimistic geometry", () => {
    const state = createLoadedState();

    // Start drag on point [0][1]
    const afterDragStart = editorReducer(state, {
      type: "DRAG_START",
      segmentIndex: 0,
      pointIndex: 1,
      latitude: 47.1,
      longitude: 11.1,
    });

    expect(afterDragStart.drag).not.toBeNull();
    expect(afterDragStart.drag!.segmentIndex).toBe(0);
    expect(afterDragStart.drag!.pointIndex).toBe(1);
    expect(afterDragStart.drag!.originalLatitude).toBe(47.1);
    expect(afterDragStart.drag!.originalLongitude).toBe(11.1);

    // Preview at new position
    const afterPreview = editorReducer(afterDragStart, {
      type: "DRAG_PREVIEW",
      latitude: 47.15,
      longitude: 11.15,
    });

    expect(afterPreview.drag!.previewLatitude).toBe(47.15);
    expect(afterPreview.drag!.previewLongitude).toBe(11.15);
    // Geometry is updated optimistically
    expect(afterPreview.optimisticGeometry![0]![1]!.latitude).toBe(47.15);
    expect(afterPreview.optimisticGeometry![0]![1]!.longitude).toBe(11.15);

    // End drag
    const afterEnd = editorReducer(afterPreview, { type: "DRAG_END" });

    expect(afterEnd.drag).toBeNull();
    // Geometry retains the preview position
    expect(afterEnd.optimisticGeometry![0]![1]!.latitude).toBe(47.15);
    expect(afterEnd.optimisticGeometry![0]![1]!.longitude).toBe(11.15);
  });

  it("DRAG_CANCEL restores the original point coordinate in optimistic geometry", () => {
    const state = createLoadedState();

    const afterDragStart = editorReducer(state, {
      type: "DRAG_START",
      segmentIndex: 0,
      pointIndex: 2,
      latitude: 47.2,
      longitude: 11.2,
    });

    // Move the point via preview
    const afterPreview = editorReducer(afterDragStart, {
      type: "DRAG_PREVIEW",
      latitude: 48.0,
      longitude: 12.0,
    });

    expect(afterPreview.optimisticGeometry![0]![2]!.latitude).toBe(48.0);

    // Cancel drag
    const afterCancel = editorReducer(afterPreview, { type: "DRAG_CANCEL" });

    expect(afterCancel.drag).toBeNull();
    // Original coordinate is restored
    expect(afterCancel.optimisticGeometry![0]![2]!.latitude).toBe(47.2);
    expect(afterCancel.optimisticGeometry![0]![2]!.longitude).toBe(11.2);
  });

  it("multiple DRAG_PREVIEW updates only affect the dragged point", () => {
    const state = createLoadedState();

    const afterDrag = applyActions(state, [
      { type: "DRAG_START", segmentIndex: 0, pointIndex: 0, latitude: 47.0, longitude: 11.0 },
      { type: "DRAG_PREVIEW", latitude: 47.01, longitude: 11.01 },
      { type: "DRAG_PREVIEW", latitude: 47.02, longitude: 11.02 },
      { type: "DRAG_PREVIEW", latitude: 47.03, longitude: 11.03 },
    ]);

    // Dragged point is at latest preview
    expect(afterDrag.optimisticGeometry![0]![0]!.latitude).toBe(47.03);
    // Other points are unchanged
    expect(afterDrag.optimisticGeometry![0]![1]!.latitude).toBe(47.1);
    expect(afterDrag.optimisticGeometry![0]![2]!.latitude).toBe(47.2);
  });
});

describe("Section replacement journey (draw-section flow)", () => {
  it("SET_TOOL(draw-section) -> SET_SELECTION(section) -> DRAWING_START -> DRAWING_ADD_POINT -> DRAWING_COMMIT -> DRAWING_FINISH", () => {
    let state = createLoadedState();

    // Switch to draw-section tool
    state = editorReducer(state, { type: "SET_TOOL", tool: "draw-section" });
    expect(state.currentTool).toBe("draw-section");
    expect(state.selection).toBeNull(); // tool change clears selection

    // Select a section
    state = editorReducer(state, {
      type: "SET_SELECTION",
      selection: { type: "section", segmentIndex: 0, startIndex: 1, endIndex: 3 },
    });
    expect(state.selection).toEqual({ type: "section", segmentIndex: 0, startIndex: 1, endIndex: 3 });

    // Start drawing at the start coordinate
    state = editorReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 1,
      endIndex: 3,
      firstPoint: { latitude: 47.1, longitude: 11.1 },
    });
    expect(state.drawing).not.toBeNull();
    expect(state.drawing!.isActive).toBe(true);
    expect(state.drawing!.points).toHaveLength(1);

    // Add intermediate points
    state = editorReducer(state, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 47.15, longitude: 11.15 },
    });
    state = editorReducer(state, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 47.2, longitude: 11.2 },
    });
    expect(state.drawing!.points).toHaveLength(3);

    // Commit drawing (marks as not active)
    state = editorReducer(state, { type: "DRAWING_COMMIT" });
    expect(state.drawing!.isActive).toBe(false);

    // Finish drawing (clears drawing state)
    state = editorReducer(state, { type: "DRAWING_FINISH" });
    expect(state.drawing).toBeNull();
  });

  it("DRAWING_CANCEL mid-drawing clears the drawing without affecting geometry or revision", () => {
    let state = createLoadedState();
    const originalGeometry = state.optimisticGeometry;
    const originalRevision = state.revision;

    state = editorReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 1,
      endIndex: 3,
      firstPoint: { latitude: 47.1, longitude: 11.1 },
    });
    state = editorReducer(state, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 47.15, longitude: 11.15 },
    });

    // Cancel
    state = editorReducer(state, { type: "DRAWING_CANCEL" });

    expect(state.drawing).toBeNull();
    expect(state.optimisticGeometry).toEqual(originalGeometry);
    expect(state.revision).toBe(originalRevision);
  });
});

describe("Undo/Redo flow", () => {
  it("OPERATION_SUCCESS updates canUndo and canRedo flags correctly", () => {
    let state = createLoadedState();

    // Apply first operation
    state = editorReducer(state, { type: "OPERATION_START" });
    expect(state.isOperationPending).toBe(true);

    state = editorReducer(state, {
      type: "OPERATION_SUCCESS",
      revision: 1,
      geometry: state.optimisticGeometry!,
      canUndo: true,
      canRedo: false,
    });
    expect(state.revision).toBe(1);
    expect(state.canUndo).toBe(true);
    expect(state.canRedo).toBe(false);
    expect(state.isOperationPending).toBe(false);

    // Undo the operation (via another OPERATION_SUCCESS representing the undo response)
    state = editorReducer(state, {
      type: "OPERATION_SUCCESS",
      revision: 2,
      geometry: state.optimisticGeometry!,
      canUndo: false,
      canRedo: true,
    });
    expect(state.revision).toBe(2);
    expect(state.canUndo).toBe(false);
    expect(state.canRedo).toBe(true);

    // Redo the operation
    state = editorReducer(state, {
      type: "OPERATION_SUCCESS",
      revision: 3,
      geometry: state.optimisticGeometry!,
      canUndo: true,
      canRedo: false,
    });
    expect(state.revision).toBe(3);
    expect(state.canUndo).toBe(true);
    expect(state.canRedo).toBe(false);
  });

  it("OPERATION_FAILURE clears pending state and sets conflict error", () => {
    let state = createLoadedState();

    state = editorReducer(state, { type: "OPERATION_START" });
    state = editorReducer(state, {
      type: "OPERATION_FAILURE",
      error: "Revision conflict",
    });

    expect(state.isOperationPending).toBe(false);
    expect(state.conflictError).toBe("Revision conflict");
  });

  it("SET_CAN_UNDO_REDO can update flags independently", () => {
    let state = createLoadedState();

    state = editorReducer(state, {
      type: "SET_CAN_UNDO_REDO",
      canUndo: true,
      canRedo: true,
    });

    expect(state.canUndo).toBe(true);
    expect(state.canRedo).toBe(true);
  });
});

describe("Reset flow", () => {
  it("OPERATION_SUCCESS with base geometry reverts optimistic geometry to base state", () => {
    let state = createLoadedState();

    // Apply an operation that modifies geometry
    const modifiedGeometry = [
      [
        { latitude: 48.0, longitude: 12.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
      ],
    ];
    state = editorReducer(state, {
      type: "OPERATION_SUCCESS",
      revision: 1,
      geometry: modifiedGeometry,
      canUndo: true,
      canRedo: false,
    });
    expect(state.optimisticGeometry![0]![0]!.latitude).toBe(48.0);

    // Reset: server responds with base geometry
    state = editorReducer(state, {
      type: "OPERATION_SUCCESS",
      revision: 2,
      geometry: state.baseGeometry!,
      canUndo: false,
      canRedo: false,
    });

    expect(state.revision).toBe(2);
    expect(state.canUndo).toBe(false);
    expect(state.canRedo).toBe(false);
    // Geometry is back to base
    expect(state.optimisticGeometry![0]![0]!.latitude).toBe(47.0);
    expect(state.optimisticGeometry![0]![0]!.longitude).toBe(11.0);
  });
});

describe("Conflict resolution journeys", () => {
  it("SET_CONFLICT_STATE sets conflictError message with server revision and local op count", () => {
    let state = createLoadedState();

    const serverDraft: RouteDraftResponse = {
      id: "draft-abc",
      activityId: "activity-1",
      revision: 5,
      state: "active",
      geometry: [
        [
          { latitude: 47.0, longitude: 11.0 },
          { latitude: 47.5, longitude: 11.5 },
        ],
      ],
      canUndo: true,
      canRedo: false,
      createdAt: "2024-01-01T00:00:00Z",
      updatedAt: "2024-01-01T01:00:00Z",
    };

    const localOps: PendingOperation[] = [
      {
        id: "op-1",
        draftId: "draft-abc",
        operation: { type: "movePoint", segmentIndex: 0, pointIndex: 0, newPosition: { latitude: 48.0, longitude: 12.0 } },
        expectedRevision: 3,
        timestamp: Date.now(),
        confirmed: false,
      },
      {
        id: "op-2",
        draftId: "draft-abc",
        operation: { type: "movePoint", segmentIndex: 0, pointIndex: 1, newPosition: { latitude: 48.1, longitude: 12.1 } },
        expectedRevision: 4,
        timestamp: Date.now(),
        confirmed: false,
      },
    ];

    state = editorReducer(state, {
      type: "SET_CONFLICT_STATE",
      serverDraft,
      localOps,
    });

    expect(state.conflictError).toContain("Revision conflict");
    expect(state.conflictError).toContain("revision 5");
    expect(state.conflictError).toContain("2 pending operation(s)");
    expect(state.conflictServerDraft).toBe(serverDraft);
    expect(state.conflictLocalOps).toBe(localOps);
    expect(state.isOperationPending).toBe(false);
  });

  it("RESOLVE_CONFLICT_RELOAD updates state to server draft values and clears conflict", () => {
    let state = createLoadedState();

    const serverDraft: RouteDraftResponse = {
      id: "draft-abc",
      activityId: "activity-1",
      revision: 5,
      state: "active",
      geometry: [
        [
          { latitude: 50.0, longitude: 13.0 },
          { latitude: 50.1, longitude: 13.1 },
        ],
      ],
      canUndo: true,
      canRedo: false,
      createdAt: "2024-01-01T00:00:00Z",
      updatedAt: "2024-01-01T01:00:00Z",
    };

    const localOps: PendingOperation[] = [
      {
        id: "op-1",
        draftId: "draft-abc",
        operation: { type: "deletePoint", segmentIndex: 0, pointIndex: 0 },
        expectedRevision: 3,
        timestamp: Date.now(),
        confirmed: false,
      },
    ];

    state = editorReducer(state, { type: "SET_CONFLICT_STATE", serverDraft, localOps });
    state = editorReducer(state, { type: "RESOLVE_CONFLICT_RELOAD" });

    expect(state.revision).toBe(5);
    expect(state.optimisticGeometry).toEqual(serverDraft.geometry);
    expect(state.canUndo).toBe(true);
    expect(state.canRedo).toBe(false);
    expect(state.conflictServerDraft).toBeNull();
    expect(state.conflictLocalOps).toEqual([]);
    expect(state.conflictError).toBeNull();
  });

  it("RESOLVE_CONFLICT_RETRY updates state to server draft values and clears conflict", () => {
    let state = createLoadedState();

    const serverDraft: RouteDraftResponse = {
      id: "draft-abc",
      activityId: "activity-1",
      revision: 7,
      state: "active",
      geometry: [
        [
          { latitude: 51.0, longitude: 14.0 },
          { latitude: 51.1, longitude: 14.1 },
        ],
      ],
      canUndo: false,
      canRedo: true,
      createdAt: "2024-01-01T00:00:00Z",
      updatedAt: "2024-01-02T00:00:00Z",
    };

    const localOps: PendingOperation[] = [
      {
        id: "op-1",
        draftId: "draft-abc",
        operation: { type: "addPoint", segmentIndex: 0, afterPointIndex: 0, point: { latitude: 47.05, longitude: 11.05 } },
        expectedRevision: 5,
        timestamp: Date.now(),
        confirmed: false,
      },
    ];

    state = editorReducer(state, { type: "SET_CONFLICT_STATE", serverDraft, localOps });
    state = editorReducer(state, { type: "RESOLVE_CONFLICT_RETRY" });

    expect(state.revision).toBe(7);
    expect(state.optimisticGeometry).toEqual(serverDraft.geometry);
    expect(state.canUndo).toBe(false);
    expect(state.canRedo).toBe(true);
    expect(state.conflictServerDraft).toBeNull();
    expect(state.conflictLocalOps).toEqual([]);
    expect(state.conflictError).toBeNull();
  });
});

describe("Offline recovery journey", () => {
  it("SET_ONLINE_STATUS(false) sets isOffline to true", () => {
    let state = createLoadedState();

    state = editorReducer(state, { type: "SET_ONLINE_STATUS", isOnline: false });
    expect(state.isOffline).toBe(true);
  });

  it("operations can still be dispatched optimistically while offline", () => {
    let state = createLoadedState();

    // Go offline
    state = editorReducer(state, { type: "SET_ONLINE_STATUS", isOnline: false });
    expect(state.isOffline).toBe(true);

    // Optimistic operation still works
    state = editorReducer(state, { type: "OPERATION_START" });
    expect(state.isOperationPending).toBe(true);

    const modifiedGeometry = [
      [
        { latitude: 48.0, longitude: 12.0 },
        { latitude: 47.1, longitude: 11.1 },
        { latitude: 47.2, longitude: 11.2 },
        { latitude: 47.3, longitude: 11.3 },
        { latitude: 47.4, longitude: 11.4 },
      ],
    ];
    state = editorReducer(state, {
      type: "OPERATION_SUCCESS",
      revision: 1,
      geometry: modifiedGeometry,
      canUndo: true,
      canRedo: false,
    });

    expect(state.isOperationPending).toBe(false);
    expect(state.revision).toBe(1);
    expect(state.canUndo).toBe(true);
    // Still offline
    expect(state.isOffline).toBe(true);
  });

  it("SET_ONLINE_STATUS(true) sets isOffline to false", () => {
    let state = createLoadedState();

    state = editorReducer(state, { type: "SET_ONLINE_STATUS", isOnline: false });
    expect(state.isOffline).toBe(true);

    state = editorReducer(state, { type: "SET_ONLINE_STATUS", isOnline: true });
    expect(state.isOffline).toBe(false);
  });
});

describe("Keyboard interaction / tool switching", () => {
  it("SET_TOOL changes the current tool and clears selection and drawing state", () => {
    let state = createLoadedState();

    // Set a selection and drawing state first
    state = editorReducer(state, {
      type: "SET_SELECTION",
      selection: { type: "point", segmentIndex: 0, pointIndex: 1 },
    });
    state = editorReducer(state, {
      type: "DRAWING_START",
      segmentIndex: 0,
      startIndex: 1,
      endIndex: 3,
      firstPoint: { latitude: 47.1, longitude: 11.1 },
    });

    expect(state.selection).not.toBeNull();
    expect(state.drawing).not.toBeNull();

    // Switch tool
    state = editorReducer(state, { type: "SET_TOOL", tool: "move" });

    expect(state.currentTool).toBe("move");
    expect(state.selection).toBeNull();
    expect(state.drawing).toBeNull();
  });

  it("switching between all available tools clears state each time", () => {
    let state = createLoadedState();
    const tools = ["select", "move", "add", "delete", "split", "join", "draw-section"] as const;

    for (const tool of tools) {
      // Add some state before each tool switch
      state = editorReducer(state, {
        type: "SET_SELECTION",
        selection: { type: "point", segmentIndex: 0, pointIndex: 0 },
      });

      state = editorReducer(state, { type: "SET_TOOL", tool });
      expect(state.currentTool).toBe(tool);
      expect(state.selection).toBeNull();
      expect(state.drawing).toBeNull();
    }
  });
});

describe("Edge cases and invariants", () => {
  it("DRAG_PREVIEW without prior DRAG_START is a no-op", () => {
    const state = createLoadedState();

    const afterPreview = editorReducer(state, {
      type: "DRAG_PREVIEW",
      latitude: 48.0,
      longitude: 12.0,
    });

    expect(afterPreview).toBe(state);
  });

  it("DRAG_CANCEL without prior DRAG_START clears drag to null", () => {
    const state = createLoadedState();

    const afterCancel = editorReducer(state, { type: "DRAG_CANCEL" });
    expect(afterCancel.drag).toBeNull();
  });

  it("DRAWING_ADD_POINT without active drawing is a no-op", () => {
    const state = createLoadedState();

    const after = editorReducer(state, {
      type: "DRAWING_ADD_POINT",
      point: { latitude: 48.0, longitude: 12.0 },
    });

    expect(after).toBe(state);
  });

  it("OPERATION_SUCCESS clears conflictError", () => {
    let state = createLoadedState();

    // Set a conflict error
    state = editorReducer(state, { type: "SET_CONFLICT", message: "Some conflict" });
    expect(state.conflictError).toBe("Some conflict");

    // Successful operation clears it
    state = editorReducer(state, {
      type: "OPERATION_SUCCESS",
      revision: 1,
      geometry: state.optimisticGeometry!,
      canUndo: true,
      canRedo: false,
    });

    expect(state.conflictError).toBeNull();
  });

  it("initialState has sensible defaults", () => {
    expect(initialState.currentTool).toBe("select");
    expect(initialState.selection).toBeNull();
    expect(initialState.draftId).toBeNull();
    expect(initialState.revision).toBe(0);
    expect(initialState.optimisticGeometry).toBeNull();
    expect(initialState.baseGeometry).toBeNull();
    expect(initialState.canUndo).toBe(false);
    expect(initialState.canRedo).toBe(false);
    expect(initialState.conflictError).toBeNull();
    expect(initialState.isOperationPending).toBe(false);
    expect(initialState.isOffline).toBe(false);
    expect(initialState.conflictServerDraft).toBeNull();
    expect(initialState.conflictLocalOps).toEqual([]);
    expect(initialState.drag).toBeNull();
    expect(initialState.drawing).toBeNull();
  });
});
