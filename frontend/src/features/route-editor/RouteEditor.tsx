import { useEffect, useCallback, useRef } from "react";
import { useNavigate } from "@tanstack/react-router";
import { EditorMap } from "./EditorMap";
import { EditorToolbar } from "./EditorToolbar";
import { useEditorState } from "./useEditorState";
import { useAutosave } from "./useAutosave";
import {
  useGetRouteDraft,
  useCreateRouteDraft,
  useApplyOperation,
  useUndoOperation,
  useRedoOperation,
  useResetDraft,
} from "./useRouteDraft";
import { useRecordedRoute } from "@/features/activity-detail/useRecordedRoute";
import { LoadingSpinner } from "@/components/LoadingSpinner";
import { ApiError } from "@/api/client";
import type { RoutePointDto } from "@/api/client";
import type { Selection, RouteOperation } from "./types";

interface RouteEditorProps {
  activityId: string;
}

/**
 * Convert a GeoJSON FeatureCollection (from recorded route) to the backend
 * geometry format: array of segments, each segment is array of {latitude, longitude, elevation?}.
 */
function recordedRouteToGeometry(
  recordedRoute: { features: Array<{ geometry: { type: string; coordinates: number[][] } }> },
): RoutePointDto[][] {
  const segments: RoutePointDto[][] = [];
  for (const feature of recordedRoute.features) {
    if (feature.geometry.type === "LineString") {
      const segment: RoutePointDto[] = feature.geometry.coordinates
        .filter((coord): coord is number[] => coord.length >= 2)
        .map((coord) => ({
          latitude: coord[1]!,
          longitude: coord[0]!,
          ...(coord[2] != null ? { elevation: coord[2] } : {}),
        }));
      if (segment.length >= 2) {
        segments.push(segment);
      }
    }
  }
  // Ensure at least one valid segment
  if (segments.length === 0) {
    segments.push([
      { latitude: 0, longitude: 0 },
      { latitude: 1, longitude: 1 },
    ]);
  }
  return segments;
}

export function RouteEditor({ activityId }: RouteEditorProps) {
  const navigate = useNavigate();
  const {
    state,
    setTool,
    setSelection,
    clearSelection,
    setDraft,
    operationStart,
    operationSuccess,
    operationFailure,
    setConflict,
    clearConflict,
    setCanUndoRedo,
  } = useEditorState();

  // Track base geometry for reset operations
  const baseGeometryRef = useRef<RoutePointDto[][] | null>(null);

  // Fetch recorded route as base geometry for creating draft
  const { data: recordedRoute, isLoading: routeLoading } =
    useRecordedRoute(activityId);

  // Draft query
  const { data: draft, refetch: refetchDraft } = useGetRouteDraft(
    state.draftId,
  );

  // Mutations
  const createDraft = useCreateRouteDraft();
  const handleConflict = useCallback(
    (error: ApiError) => {
      setConflict(
        `Revision conflict: the draft was modified elsewhere. Current revision is stale. Please reload the draft.`,
      );
      void error;
    },
    [setConflict],
  );

  const applyOp = useApplyOperation(handleConflict);
  const undoOp = useUndoOperation(handleConflict);
  const redoOp = useRedoOperation(handleConflict);
  const resetOp = useResetDraft(handleConflict);

  // Autosave
  const { saveOperation, confirmSaved, hasRecovery, dismissRecovery } =
    useAutosave({
      draftId: state.draftId,
    });

  // Create draft from recorded route when we have the route data
  useEffect(() => {
    if (state.draftId || !recordedRoute || createDraft.isPending) return;

    // Convert FeatureCollection to domain geometry format
    const geometry = recordedRouteToGeometry(recordedRoute);
    baseGeometryRef.current = geometry;

    createDraft.mutate(
      { activityId, geometry },
      {
        onSuccess: (data) => {
          setDraft(data.id, data.revision, data.geometry, geometry);
          setCanUndoRedo(data.canUndo, data.canRedo);
        },
      },
    );
  }, [
    activityId,
    recordedRoute,
    state.draftId,
    createDraft,
    setDraft,
    setCanUndoRedo,
  ]);

  // Sync draft data to state when it changes
  useEffect(() => {
    if (draft && state.draftId) {
      setCanUndoRedo(draft.canUndo, draft.canRedo);
    }
  }, [draft, state.draftId, setCanUndoRedo]);

  // Keyboard shortcuts
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Ctrl+Z or Cmd+Z for undo
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && !e.shiftKey) {
        e.preventDefault();
        if (state.canUndo && !state.isOperationPending) {
          handleUndo();
        }
      }
      // Ctrl+Shift+Z or Cmd+Shift+Z for redo
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && e.shiftKey) {
        e.preventDefault();
        if (state.canRedo && !state.isOperationPending) {
          handleRedo();
        }
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.canUndo, state.canRedo, state.isOperationPending, state.draftId, state.revision]);

  // Operation handlers
  const dispatchOperation = useCallback(
    async (operation: RouteOperation) => {
      if (!state.draftId) return;

      operationStart();
      const opId = await saveOperation(operation, state.revision);

      applyOp.mutate(
        {
          draftId: state.draftId,
          operation: operation as unknown as Record<string, unknown>,
          expectedRevision: state.revision,
        },
        {
          onSuccess: async (result) => {
            await confirmSaved(opId);
            // Refetch draft to get updated geometry
            const { data: updatedDraft } = await refetchDraft();
            if (updatedDraft) {
              operationSuccess(
                result.revision,
                updatedDraft.geometry,
                result.canUndo,
                result.canRedo,
              );
            } else {
              operationSuccess(result.revision, state.optimisticGeometry ?? [], result.canUndo, result.canRedo);
            }
          },
          onError: (error) => {
            if (error instanceof ApiError && error.status === 409) {
              operationFailure(
                "Revision conflict. The draft was modified elsewhere.",
              );
            } else {
              operationFailure(error.message);
            }
          },
        },
      );
    },
    [
      state.draftId,
      state.revision,
      state.optimisticGeometry,
      operationStart,
      saveOperation,
      confirmSaved,
      applyOp,
      refetchDraft,
      operationSuccess,
      operationFailure,
    ],
  );

  const handleUndo = useCallback(() => {
    if (!state.draftId) return;
    operationStart();
    undoOp.mutate(
      { draftId: state.draftId, expectedRevision: state.revision },
      {
        onSuccess: async (result) => {
          const { data: updatedDraft } = await refetchDraft();
          if (updatedDraft) {
            operationSuccess(
              result.revision,
              updatedDraft.geometry,
              result.canUndo,
              result.canRedo,
            );
          } else {
            operationSuccess(result.revision, state.optimisticGeometry ?? [], result.canUndo, result.canRedo);
          }
        },
        onError: (error) => {
          operationFailure(error.message);
        },
      },
    );
  }, [
    state.draftId,
    state.revision,
    state.optimisticGeometry,
    operationStart,
    undoOp,
    refetchDraft,
    operationSuccess,
    operationFailure,
  ]);

  const handleRedo = useCallback(() => {
    if (!state.draftId) return;
    operationStart();
    redoOp.mutate(
      { draftId: state.draftId, expectedRevision: state.revision },
      {
        onSuccess: async (result) => {
          const { data: updatedDraft } = await refetchDraft();
          if (updatedDraft) {
            operationSuccess(
              result.revision,
              updatedDraft.geometry,
              result.canUndo,
              result.canRedo,
            );
          } else {
            operationSuccess(result.revision, state.optimisticGeometry ?? [], result.canUndo, result.canRedo);
          }
        },
        onError: (error) => {
          operationFailure(error.message);
        },
      },
    );
  }, [
    state.draftId,
    state.revision,
    state.optimisticGeometry,
    operationStart,
    redoOp,
    refetchDraft,
    operationSuccess,
    operationFailure,
  ]);

  const handleReset = useCallback(() => {
    if (!state.draftId) return;
    const geometry = state.baseGeometry ?? baseGeometryRef.current;
    if (!geometry) return;

    operationStart();
    resetOp.mutate(
      { draftId: state.draftId, expectedRevision: state.revision, geometry },
      {
        onSuccess: async (result) => {
          const { data: updatedDraft } = await refetchDraft();
          if (updatedDraft) {
            operationSuccess(
              result.revision,
              updatedDraft.geometry,
              result.canUndo,
              result.canRedo,
            );
          } else {
            operationSuccess(result.revision, state.optimisticGeometry ?? [], result.canUndo, result.canRedo);
          }
          clearSelection();
        },
        onError: (error) => {
          operationFailure(error.message);
        },
      },
    );
  }, [
    state.draftId,
    state.revision,
    state.optimisticGeometry,
    state.baseGeometry,
    operationStart,
    resetOp,
    refetchDraft,
    operationSuccess,
    operationFailure,
    clearSelection,
  ]);

  const handleSelectionChange = useCallback(
    (selection: Selection) => {
      setSelection(selection);
    },
    [setSelection],
  );

  const handleMovePoint = useCallback(
    (segmentIndex: number, pointIndex: number, newLng: number, newLat: number) => {
      void dispatchOperation({
        type: "movePoint",
        segmentIndex,
        pointIndex,
        newPosition: { latitude: newLat, longitude: newLng },
      });
    },
    [dispatchOperation],
  );

  const handleAddPoint = useCallback(
    (segmentIndex: number, afterPointIndex: number, lng: number, lat: number) => {
      void dispatchOperation({
        type: "addPoint",
        segmentIndex,
        afterPointIndex,
        point: { latitude: lat, longitude: lng },
      });
    },
    [dispatchOperation],
  );

  const handleDelete = useCallback(() => {
    if (!state.selection) return;
    if (state.selection.type === "point") {
      void dispatchOperation({
        type: "deletePoint",
        segmentIndex: state.selection.segmentIndex,
        pointIndex: state.selection.pointIndex,
      });
    } else if (state.selection.type === "section") {
      void dispatchOperation({
        type: "deleteSection",
        segmentIndex: state.selection.segmentIndex,
        startIndex: state.selection.startIndex,
        endIndex: state.selection.endIndex,
      });
    }
    clearSelection();
  }, [state.selection, dispatchOperation, clearSelection]);

  const handleSplit = useCallback(() => {
    if (!state.selection || state.selection.type !== "point") return;
    void dispatchOperation({
      type: "splitSegment",
      segmentIndex: state.selection.segmentIndex,
      atPointIndex: state.selection.pointIndex,
    });
    clearSelection();
  }, [state.selection, dispatchOperation, clearSelection]);

  const handleJoin = useCallback(() => {
    // Join the first two segments (simplified - in a full implementation
    // the user would select which segments to join)
    void dispatchOperation({
      type: "joinSegments",
      firstSegmentIndex: 0,
      secondSegmentIndex: 1,
    });
  }, [dispatchOperation]);

  const handleReloadDraft = useCallback(async () => {
    clearConflict();
    const { data: updatedDraft } = await refetchDraft();
    if (updatedDraft) {
      operationSuccess(
        updatedDraft.revision,
        updatedDraft.geometry,
        updatedDraft.canUndo,
        updatedDraft.canRedo,
      );
    }
  }, [clearConflict, refetchDraft, operationSuccess]);

  const handleBack = useCallback(() => {
    void navigate({ to: "/activities/$activityId", params: { activityId } });
  }, [navigate, activityId]);

  // Loading state
  if (routeLoading || createDraft.isPending) {
    return (
      <div className="flex h-screen items-center justify-center">
        <LoadingSpinner className="py-16" />
      </div>
    );
  }

  return (
    <div className="flex h-screen flex-col" role="region" aria-label="Route editor">
      {/* Header */}
      <header className="flex items-center gap-4 border-b border-gray-200 bg-white px-4 py-2">
        <button
          type="button"
          onClick={handleBack}
          className="rounded text-sm text-gray-600 hover:text-gray-900 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          aria-label="Back to activity detail"
        >
          &larr; Back
        </button>
        <h1 className="text-lg font-semibold text-gray-900">Edit Route</h1>
      </header>

      {/* Conflict banner */}
      {state.conflictError && (
        <div
          className="flex items-center gap-3 bg-amber-50 border-b border-amber-200 px-4 py-2"
          role="alert"
          aria-live="assertive"
        >
          <svg
            className="h-5 w-5 text-amber-600 flex-shrink-0"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            aria-hidden="true"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z"
            />
          </svg>
          <p className="text-sm text-amber-800">{state.conflictError}</p>
          <button
            type="button"
            className="ml-auto rounded bg-amber-600 px-3 py-1 text-sm font-medium text-white hover:bg-amber-700 focus:outline-none focus:ring-2 focus:ring-amber-500"
            onClick={() => void handleReloadDraft()}
          >
            Reload Draft
          </button>
        </div>
      )}

      {/* Recovery banner */}
      {hasRecovery && (
        <div
          className="flex items-center gap-3 bg-blue-50 border-b border-blue-200 px-4 py-2"
          role="alert"
          aria-live="polite"
        >
          <p className="text-sm text-blue-800">
            Unsaved operations were found from a previous session.
          </p>
          <button
            type="button"
            className="ml-auto rounded bg-blue-600 px-3 py-1 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500"
            onClick={dismissRecovery}
          >
            Dismiss
          </button>
        </div>
      )}

      {/* Toolbar */}
      <EditorToolbar
        currentTool={state.currentTool}
        onToolChange={setTool}
        canUndo={state.canUndo}
        canRedo={state.canRedo}
        onUndo={handleUndo}
        onRedo={handleRedo}
        onReset={handleReset}
        onDelete={handleDelete}
        onSplit={handleSplit}
        onJoin={handleJoin}
        hasSelection={state.selection !== null}
        isOperationPending={state.isOperationPending}
      />

      {/* Map area */}
      <div className="flex-1 relative" aria-label="Map editing area">
        <EditorMap
          geometry={state.optimisticGeometry}
          baseGeometry={state.baseGeometry}
          selection={state.selection}
          currentTool={state.currentTool}
          onSelectionChange={handleSelectionChange}
          onMovePoint={handleMovePoint}
          onAddPoint={handleAddPoint}
        />
      </div>
    </div>
  );
}
