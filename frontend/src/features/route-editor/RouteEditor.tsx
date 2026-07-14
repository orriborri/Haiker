import { useEffect, useCallback, useRef, useMemo } from "react";
import { useNavigate } from "@tanstack/react-router";
import { EditorMap } from "./EditorMap";
import { EditorToolbar } from "./EditorToolbar";
import { ConflictDialog } from "./ConflictDialog";
import { RecoveryDialog } from "./RecoveryDialog";
import { useEditorState } from "./useEditorState";
import { useAutosave } from "./useAutosave";
import { useOnlineStatus } from "./useOnlineStatus";
import {
  useGetRouteDraft,
  useCreateRouteDraft,
  useApplyOperation,
  useUndoOperation,
  useRedoOperation,
  useResetDraft,
} from "./useRouteDraft";
import { getRouteDraft } from "@/api/client";
import { useRecordedRoute } from "@/features/activity-detail/useRecordedRoute";
import { LoadingSpinner } from "@/components/LoadingSpinner";
import { ApiError } from "@/api/client";
import type { RoutePointDto } from "@/api/client";
import type { Selection, RouteOperation, PendingOperation } from "./types";

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
  const { isOnline } = useOnlineStatus();
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
    setConflictState,
    resolveConflictReload,
    resolveConflictRetry,
    setOnlineStatus,
  } = useEditorState();

  // Track base geometry for reset operations
  const baseGeometryRef = useRef<RoutePointDto[][] | null>(null);

  // Sync online status to reducer state
  const prevOnlineRef = useRef(isOnline);
  useEffect(() => {
    setOnlineStatus(isOnline);
  }, [isOnline, setOnlineStatus]);

  // Fetch recorded route as base geometry for creating draft
  const { data: recordedRoute, isLoading: routeLoading } =
    useRecordedRoute(activityId);

  // Draft query
  const { data: draft, refetch: refetchDraft } = useGetRouteDraft(
    state.draftId,
  );

  // Mutations
  const createDraft = useCreateRouteDraft();

  // Use a ref for getUnconfirmedOps to avoid circular dependency with handleConflict
  const getUnconfirmedOpsRef = useRef<() => Promise<PendingOperation[]>>(async () => []);

  const handleConflict = useCallback(
    async (error: ApiError) => {
      void error;
      // Fetch fresh server state and local pending ops
      const { data: serverDraft } = await refetchDraft();
      const localOps = await getUnconfirmedOpsRef.current();
      if (serverDraft) {
        setConflictState(serverDraft, localOps);
      } else {
        setConflict(
          `Revision conflict: the draft was modified elsewhere. Current revision is stale. Please reload the draft.`,
        );
      }
    },
    [refetchDraft, setConflictState, setConflict],
  );

  const applyOp = useApplyOperation(handleConflict);
  const undoOp = useUndoOperation(handleConflict);
  const redoOp = useRedoOperation(handleConflict);
  const resetOp = useResetDraft(handleConflict);

  // Autosave
  const { saveOperation, confirmSaved, hasRecovery, recoveryOps, dismissRecovery, getUnconfirmedOps, clearRecovery, saveBaseRevision, getBaseRevision } =
    useAutosave({
      draftId: state.draftId,
    });

  // Keep the ref in sync
  getUnconfirmedOpsRef.current = getUnconfirmedOps;

  // On reconnection (offline -> online): revalidate server revision
  useEffect(() => {
    const wasOffline = !prevOnlineRef.current;
    prevOnlineRef.current = isOnline;

    if (isOnline && wasOffline && state.draftId) {
      // Refetch draft bypassing service worker cache, with retry
      const draftId = state.draftId;
      void (async () => {
        const savedRevision = await getBaseRevision();
        let serverDraft = null;
        const maxRetries = 3;
        for (let attempt = 0; attempt < maxRetries; attempt++) {
          try {
            serverDraft = await getRouteDraft(draftId, { cache: "no-store" });
            break;
          } catch {
            if (attempt < maxRetries - 1) {
              await new Promise((r) => setTimeout(r, 1000 * (attempt + 1)));
            } else {
              console.warn(
                "[RouteEditor] Reconnection refetch failed after retries. Reconciliation skipped.",
              );
              return;
            }
          }
        }
        if (serverDraft) {
          // If server revision differs from what we last saved, trigger conflict flow
          const localRevision = savedRevision ?? state.revision;
          if (serverDraft.revision !== localRevision) {
            const localOps = await getUnconfirmedOpsRef.current();
            if (localOps.length > 0) {
              setConflictState(serverDraft, localOps);
            } else {
              // No pending ops - just accept server state
              operationSuccess(
                serverDraft.revision,
                serverDraft.geometry,
                serverDraft.canUndo,
                serverDraft.canRedo,
              );
            }
          }
        }
      })();
    }
  }, [isOnline, state.draftId, state.revision, getBaseRevision, setConflictState, operationSuccess]);

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
          void saveBaseRevision(data.revision);
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
    saveBaseRevision,
  ]);

  // Sync draft data to state when it changes
  useEffect(() => {
    if (draft && state.draftId) {
      setCanUndoRedo(draft.canUndo, draft.canRedo);
    }
  }, [draft, state.draftId, setCanUndoRedo]);

  // Operation handlers
  const dispatchOperation = useCallback(
    async (operation: RouteOperation) => {
      if (!state.draftId || state.isOffline) return;

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
            await saveBaseRevision(result.revision);
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
      state.isOffline,
      operationStart,
      saveOperation,
      confirmSaved,
      saveBaseRevision,
      applyOp,
      refetchDraft,
      operationSuccess,
      operationFailure,
    ],
  );

  const handleUndo = useCallback(() => {
    if (!state.draftId || state.isOffline) return;
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
    state.isOffline,
    operationStart,
    undoOp,
    refetchDraft,
    operationSuccess,
    operationFailure,
  ]);

  const handleRedo = useCallback(() => {
    if (!state.draftId || state.isOffline) return;
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
    state.isOffline,
    operationStart,
    redoOp,
    refetchDraft,
    operationSuccess,
    operationFailure,
  ]);

  const handleReset = useCallback(() => {
    if (!state.draftId || state.isOffline) return;
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
          await clearRecovery();
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
    state.isOffline,
    operationStart,
    resetOp,
    refetchDraft,
    operationSuccess,
    operationFailure,
    clearSelection,
    clearRecovery,
  ]);

  // Compute a human-readable description of the current selection for accessibility
  const selectionDescription = useMemo((): string | null => {
    const selection = state.selection;
    if (!selection) return null;
    if (selection.type === "point") {
      return `Point ${selection.pointIndex + 1} on segment ${selection.segmentIndex + 1} selected`;
    }
    if (selection.type === "section") {
      return `Section from point ${selection.startIndex + 1} to point ${selection.endIndex + 1} on segment ${selection.segmentIndex + 1} selected`;
    }
    return null;
  }, [state.selection]);

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

  // Keyboard shortcuts
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Ignore if focus is inside an input, textarea, contenteditable, or textbox role
      const target = e.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable ||
        target.getAttribute("role") === "textbox"
      ) return;

      // Ctrl+Z or Cmd+Z for undo
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && !e.shiftKey) {
        e.preventDefault();
        if (state.canUndo && !state.isOperationPending) {
          handleUndo();
        }
        return;
      }
      // Ctrl+Shift+Z or Cmd+Shift+Z for redo
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && e.shiftKey) {
        e.preventDefault();
        if (state.canRedo && !state.isOperationPending) {
          handleRedo();
        }
        return;
      }

      // Escape clears selection
      if (e.key === "Escape") {
        e.preventDefault();
        clearSelection();
        return;
      }

      // Delete/Backspace triggers delete on selection
      if ((e.key === "Delete" || e.key === "Backspace") && !e.ctrlKey && !e.metaKey) {
        if (state.selection && !state.isOperationPending) {
          e.preventDefault();
          handleDelete();
        }
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [state.canUndo, state.canRedo, state.isOperationPending, state.selection, handleUndo, handleRedo, handleDelete, clearSelection]);

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

  const handleAcceptServerState = useCallback(async () => {
    await clearRecovery();
    resolveConflictReload();
  }, [clearRecovery, resolveConflictReload]);

  const handleRetryOperations = useCallback(async () => {
    const opsToRetry: PendingOperation[] = [...state.conflictLocalOps];
    const serverRevision = state.conflictServerDraft?.revision ?? state.revision;
    resolveConflictRetry();
    await clearRecovery();

    // Re-apply each pending operation sequentially against the new server state
    let currentRevision = serverRevision;
    let successCount = 0;
    for (const pendingOp of opsToRetry) {
      if (!state.draftId) break;
      try {
        const result = await applyOp.mutateAsync({
          draftId: state.draftId,
          operation: pendingOp.operation as unknown as Record<string, unknown>,
          expectedRevision: currentRevision,
        });
        currentRevision = result.revision;
        successCount++;
      } catch {
        // If a retry fails, surface which operations were lost
        setConflict(
          `Retry failed: ${successCount} of ${opsToRetry.length} operations were re-applied successfully. The remaining ${opsToRetry.length - successCount} operation(s) could not be applied.`,
        );
        break;
      }
    }

    // Refresh to get the latest server state after retries
    const { data: updatedDraft } = await refetchDraft();
    if (updatedDraft) {
      operationSuccess(
        updatedDraft.revision,
        updatedDraft.geometry,
        updatedDraft.canUndo,
        updatedDraft.canRedo,
      );
      await saveBaseRevision(updatedDraft.revision);
    }
  }, [
    state.conflictLocalOps,
    state.conflictServerDraft,
    state.revision,
    state.draftId,
    resolveConflictRetry,
    clearRecovery,
    applyOp,
    refetchDraft,
    operationSuccess,
    setConflict,
    saveBaseRevision,
  ]);

  const handleDismissConflict = useCallback(() => {
    clearConflict();
    resolveConflictReload();
  }, [clearConflict, resolveConflictReload]);

  const handleReplayRecovery = useCallback(async () => {
    if (!state.draftId) return;

    // Fetch fresh server draft bypassing service worker cache
    let serverDraft = null;
    try {
      serverDraft = await getRouteDraft(state.draftId, { cache: "no-store" });
    } catch {
      setConflict("Failed to fetch server state for recovery replay. Please try again.");
      return;
    }
    if (!serverDraft) return;

    // Replay all pending operations against the current server revision
    const opsToReplay = [...recoveryOps];
    let currentRevision = serverDraft.revision;
    let successCount = 0;

    for (const pendingOp of opsToReplay) {
      try {
        const result = await applyOp.mutateAsync({
          draftId: state.draftId,
          operation: pendingOp.operation as unknown as Record<string, unknown>,
          expectedRevision: currentRevision,
        });
        currentRevision = result.revision;
        successCount++;
      } catch {
        setConflict(
          `Recovery replay failed: ${successCount} of ${opsToReplay.length} operations were re-applied. The remaining ${opsToReplay.length - successCount} could not be applied due to a conflict.`,
        );
        break;
      }
    }

    // Clear recovery data after replay attempt
    await clearRecovery();

    // Refresh to get the latest server state
    const { data: updatedDraft } = await refetchDraft();
    if (updatedDraft) {
      operationSuccess(
        updatedDraft.revision,
        updatedDraft.geometry,
        updatedDraft.canUndo,
        updatedDraft.canRedo,
      );
      await saveBaseRevision(updatedDraft.revision);
    }
  }, [
    state.draftId,
    recoveryOps,
    refetchDraft,
    applyOp,
    clearRecovery,
    operationSuccess,
    saveBaseRevision,
    setConflict,
  ]);

  const handleDiscardRecovery = useCallback(() => {
    void dismissRecovery();
  }, [dismissRecovery]);

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

      {/* Conflict dialog */}
      {state.conflictServerDraft && (
        <ConflictDialog
          serverDraft={state.conflictServerDraft}
          localPendingOps={state.conflictLocalOps}
          onReloadServerState={() => void handleAcceptServerState()}
          onRetryOperations={() => void handleRetryOperations()}
          onDismiss={handleDismissConflict}
        />
      )}

      {/* Conflict banner (fallback when no server draft available) */}
      {state.conflictError && !state.conflictServerDraft && (
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

      {/* Recovery dialog */}
      {hasRecovery && (
        <RecoveryDialog
          pendingOperations={recoveryOps}
          isOffline={state.isOffline}
          onReplay={() => void handleReplayRecovery()}
          onDiscard={handleDiscardRecovery}
        />
      )}

      {/* Offline banner */}
      {state.isOffline && (
        <div
          className="flex items-center gap-3 bg-gray-100 border-b border-gray-300 px-4 py-2"
          role="alert"
          aria-live="assertive"
        >
          <svg
            className="h-5 w-5 text-gray-600 flex-shrink-0"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            aria-hidden="true"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M18.364 5.636a9 9 0 010 12.728M5.636 5.636a9 9 0 000 12.728M12 12h.01"
            />
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M3 3l18 18"
            />
          </svg>
          <p className="text-sm text-gray-700">
            You are offline. Edits are paused until connectivity is restored.
          </p>
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
        isOffline={state.isOffline}
        selectionDescription={selectionDescription}
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
