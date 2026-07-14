import { useReducer, useCallback } from "react";
import type { RoutePointDto, RouteDraftResponse } from "@/api/client";
import type {
  EditorState,
  EditorAction,
  EditorTool,
  Selection,
  PendingOperation,
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
  conflictServerDraft: null,
  conflictLocalOps: [],
};

function editorReducer(state: EditorState, action: EditorAction): EditorState {
  switch (action.type) {
    case "SET_TOOL":
      return { ...state, currentTool: action.tool, selection: null };
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
  };
}
