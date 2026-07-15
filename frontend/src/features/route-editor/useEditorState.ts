import { useReducer, useCallback } from "react";
import type { RoutePointDto, RouteDraftResponse } from "@/api/client";
import type {
  EditorState,
  EditorAction,
  EditorTool,
  Selection,
  PendingOperation,
  DragState,
  DrawingState,
} from "./types";

const initialState: EditorState = {
  currentTool: "select",
  selection: null,
  draftId: null,
  revision: 0,
  optimisticGeometry: null,
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

function editorReducer(state: EditorState, action: EditorAction): EditorState {
  switch (action.type) {
    case "SET_TOOL":
      return { ...state, currentTool: action.tool, selection: null, drawing: null };
    case "SET_SELECTION":
      return { ...state, selection: action.selection };
    case "CLEAR_SELECTION":
      return { ...state, selection: null };
    case "SET_DRAFT":
      return {
        ...state,
        draftId: action.draftId,
        revision: action.revision,
        optimisticGeometry: action.geometry,
        baseGeometry: action.baseGeometry,
        conflictError: null,
      };
    case "OPERATION_START":
      return { ...state, isOperationPending: true };
    case "OPERATION_SUCCESS":
      return {
        ...state,
        revision: action.revision,
        optimisticGeometry: action.geometry,
        canUndo: action.canUndo,
        canRedo: action.canRedo,
        isOperationPending: false,
        conflictError: null,
      };
    case "OPERATION_FAILURE":
      return {
        ...state,
        isOperationPending: false,
        conflictError: action.error,
      };
    case "SET_CONFLICT":
      return { ...state, conflictError: action.message };
    case "CLEAR_CONFLICT":
      return { ...state, conflictError: null };
    case "SET_OPTIMISTIC_GEOMETRY":
      return { ...state, optimisticGeometry: action.geometry };
    case "SET_CAN_UNDO_REDO":
      return {
        ...state,
        canUndo: action.canUndo,
        canRedo: action.canRedo,
      };
    case "SET_CONFLICT_STATE":
      return {
        ...state,
        conflictServerDraft: action.serverDraft,
        conflictLocalOps: action.localOps,
        conflictError: `Revision conflict: server is at revision ${action.serverDraft.revision}. You have ${action.localOps.length} pending operation(s).`,
        isOperationPending: false,
      };
    case "RESOLVE_CONFLICT_RELOAD":
      return {
        ...state,
        revision: state.conflictServerDraft?.revision ?? state.revision,
        optimisticGeometry: state.conflictServerDraft?.geometry ?? state.optimisticGeometry,
        canUndo: state.conflictServerDraft?.canUndo ?? state.canUndo,
        canRedo: state.conflictServerDraft?.canRedo ?? state.canRedo,
        conflictServerDraft: null,
        conflictLocalOps: [],
        conflictError: null,
      };
    case "RESOLVE_CONFLICT_RETRY":
      return {
        ...state,
        revision: state.conflictServerDraft?.revision ?? state.revision,
        optimisticGeometry: state.conflictServerDraft?.geometry ?? state.optimisticGeometry,
        canUndo: state.conflictServerDraft?.canUndo ?? state.canUndo,
        canRedo: state.conflictServerDraft?.canRedo ?? state.canRedo,
        conflictServerDraft: null,
        conflictLocalOps: [],
        conflictError: null,
      };
    case "SET_ONLINE_STATUS":
      return { ...state, isOffline: !action.isOnline };
    case "DRAG_START": {
      const drag: DragState = {
        segmentIndex: action.segmentIndex,
        pointIndex: action.pointIndex,
        originalLatitude: action.latitude,
        originalLongitude: action.longitude,
        previewLatitude: action.latitude,
        previewLongitude: action.longitude,
      };
      return { ...state, drag };
    }
    case "DRAG_PREVIEW": {
      if (!state.drag) return state;
      const updatedDrag: DragState = {
        ...state.drag,
        previewLatitude: action.latitude,
        previewLongitude: action.longitude,
      };
      // Update geometry optimistically for live preview
      if (!state.optimisticGeometry) return { ...state, drag: updatedDrag };
      const previewGeometry = state.optimisticGeometry.map((segment, sIdx) => {
        if (sIdx !== updatedDrag.segmentIndex) return segment;
        return segment.map((pt, pIdx) => {
          if (pIdx !== updatedDrag.pointIndex) return pt;
          return { ...pt, latitude: action.latitude, longitude: action.longitude };
        });
      });
      return { ...state, drag: updatedDrag, optimisticGeometry: previewGeometry };
    }
    case "DRAG_END":
      return { ...state, drag: null };
    case "DRAG_CANCEL": {
      // Restore original geometry from before drag
      if (!state.drag || !state.optimisticGeometry) return { ...state, drag: null };
      const restored = state.optimisticGeometry.map((segment, sIdx) => {
        if (sIdx !== state.drag!.segmentIndex) return segment;
        return segment.map((pt, pIdx) => {
          if (pIdx !== state.drag!.pointIndex) return pt;
          return { ...pt, latitude: state.drag!.originalLatitude, longitude: state.drag!.originalLongitude };
        });
      });
      return { ...state, drag: null, optimisticGeometry: restored };
    }
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
      // Never remove the first point (endpoint continuity anchor)
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
    case "DRAWING_FINISH": {
      return { ...state, drawing: null };
    }
  }
}

export function useEditorState() {
  const [state, dispatch] = useReducer(editorReducer, initialState);

  const setTool = useCallback((tool: EditorTool) => {
    dispatch({ type: "SET_TOOL", tool });
  }, []);

  const setSelection = useCallback((selection: Selection) => {
    dispatch({ type: "SET_SELECTION", selection });
  }, []);

  const clearSelection = useCallback(() => {
    dispatch({ type: "CLEAR_SELECTION" });
  }, []);

  const setDraft = useCallback(
    (draftId: string, revision: number, geometry: RoutePointDto[][], baseGeometry: RoutePointDto[][]) => {
      dispatch({ type: "SET_DRAFT", draftId, revision, geometry, baseGeometry });
    },
    [],
  );

  const operationStart = useCallback(() => {
    dispatch({ type: "OPERATION_START" });
  }, []);

  const operationSuccess = useCallback(
    (revision: number, geometry: RoutePointDto[][], canUndo: boolean, canRedo: boolean) => {
      dispatch({ type: "OPERATION_SUCCESS", revision, geometry, canUndo, canRedo });
    },
    [],
  );

  const operationFailure = useCallback((error: string) => {
    dispatch({ type: "OPERATION_FAILURE", error });
  }, []);

  const setConflict = useCallback((message: string) => {
    dispatch({ type: "SET_CONFLICT", message });
  }, []);

  const clearConflict = useCallback(() => {
    dispatch({ type: "CLEAR_CONFLICT" });
  }, []);

  const setOptimisticGeometry = useCallback((geometry: RoutePointDto[][]) => {
    dispatch({ type: "SET_OPTIMISTIC_GEOMETRY", geometry });
  }, []);

  const setCanUndoRedo = useCallback((canUndo: boolean, canRedo: boolean) => {
    dispatch({ type: "SET_CAN_UNDO_REDO", canUndo, canRedo });
  }, []);

  const setConflictState = useCallback(
    (serverDraft: RouteDraftResponse, localOps: PendingOperation[]) => {
      dispatch({ type: "SET_CONFLICT_STATE", serverDraft, localOps });
    },
    [],
  );

  const resolveConflictReload = useCallback(() => {
    dispatch({ type: "RESOLVE_CONFLICT_RELOAD" });
  }, []);

  const resolveConflictRetry = useCallback(() => {
    dispatch({ type: "RESOLVE_CONFLICT_RETRY" });
  }, []);

  const setOnlineStatus = useCallback((isOnline: boolean) => {
    dispatch({ type: "SET_ONLINE_STATUS", isOnline });
  }, []);

  const dragStart = useCallback(
    (segmentIndex: number, pointIndex: number, latitude: number, longitude: number) => {
      dispatch({ type: "DRAG_START", segmentIndex, pointIndex, latitude, longitude });
    },
    [],
  );

  const dragPreview = useCallback((latitude: number, longitude: number) => {
    dispatch({ type: "DRAG_PREVIEW", latitude, longitude });
  }, []);

  const dragEnd = useCallback(() => {
    dispatch({ type: "DRAG_END" });
  }, []);

  const dragCancel = useCallback(() => {
    dispatch({ type: "DRAG_CANCEL" });
  }, []);

  return {
    state,
    dispatch,
    setTool,
    setSelection,
    clearSelection,
    setDraft,
    operationStart,
    operationSuccess,
    operationFailure,
    setConflict,
    clearConflict,
    setOptimisticGeometry,
    setCanUndoRedo,
    setConflictState,
    resolveConflictReload,
    resolveConflictRetry,
    setOnlineStatus,
    dragStart,
    dragPreview,
    dragEnd,
    dragCancel,
  };
}
