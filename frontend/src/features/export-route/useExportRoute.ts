import { useState, useCallback, useEffect, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  requestExport,
  getExportStatus,
  getExportDownloadUrl,
  ApiError,
  type ExportStatusResponse,
  type ExportDownloadResponse,
} from "@/api/client";
import { useOnlineStatus } from "@/features/route-editor/useOnlineStatus";

/** Maximum polling duration in milliseconds (2 minutes). */
export const MAX_POLLING_DURATION_MS = 120_000;

// --- Reducer / pure logic (exported for testing) ---

export type ExportPhase =
  | "idle"
  | "requesting"
  | "polling"
  | "ready"
  | "failed"
  | "expired"
  | "offline"
  | "unauthorized";

export type ExportStatus = ExportStatusResponse["status"];

export const TERMINAL_STATUSES: ReadonlySet<ExportStatus> = new Set([
  "ready",
  "failed",
  "expired",
]);

export function isTerminalStatus(status: ExportStatus): boolean {
  return TERMINAL_STATUSES.has(status);
}

export function getProgressLabel(status: ExportStatus): string {
  switch (status) {
    case "queued":
      return "Queued for generation...";
    case "generating":
      return "Generating GPX...";
    default:
      return "Processing...";
  }
}

export type ExportAction =
  | { type: "START_REQUEST" }
  | { type: "REQUEST_SUCCESS"; exportId: string }
  | { type: "REQUEST_DUPLICATE"; exportId: string }
  | { type: "REQUEST_FAILURE"; error: string }
  | { type: "REQUEST_UNAUTHORIZED" }
  | { type: "POLL_UPDATE"; status: ExportStatus; failureReason?: string | null }
  | { type: "SET_OFFLINE"; isOffline: boolean }
  | { type: "RETRY" }
  | { type: "RESUME_POLLING" };

export interface ExportState {
  phase: ExportPhase;
  exportId: string | null;
  error: string | null;
  polledStatus: ExportStatus | null;
  failureReason: string | null;
}

export const initialExportState: ExportState = {
  phase: "idle",
  exportId: null,
  error: null,
  polledStatus: null,
  failureReason: null,
};

export function exportReducer(
  state: ExportState,
  action: ExportAction,
): ExportState {
  switch (action.type) {
    case "START_REQUEST":
      return { ...state, phase: "requesting", error: null, failureReason: null };
    case "REQUEST_SUCCESS":
      return { ...state, phase: "polling", exportId: action.exportId };
    case "REQUEST_DUPLICATE":
      return { ...state, phase: "polling", exportId: action.exportId };
    case "REQUEST_FAILURE":
      return { ...state, phase: "failed", error: action.error };
    case "REQUEST_UNAUTHORIZED":
      return {
        ...state,
        phase: "unauthorized",
        error: "You are not authorized to export this route. Please sign in and try again.",
      };
    case "POLL_UPDATE": {
      if (action.status === "ready") {
        return { ...state, phase: "ready", polledStatus: "ready" };
      }
      if (action.status === "failed") {
        return {
          ...state,
          phase: "failed",
          polledStatus: "failed",
          failureReason: action.failureReason ?? "Export failed unexpectedly",
          error: action.failureReason ?? "Export failed unexpectedly",
        };
      }
      if (action.status === "expired") {
        return { ...state, phase: "expired", polledStatus: "expired" };
      }
      return { ...state, phase: "polling", polledStatus: action.status };
    }
    case "SET_OFFLINE":
      if (action.isOffline && state.phase === "idle") {
        return { ...state, phase: "offline" };
      }
      if (!action.isOffline && state.phase === "offline") {
        return { ...state, phase: "idle" };
      }
      return state;
    case "RETRY":
      return {
        ...initialExportState,
        phase: "idle",
      };
    case "RESUME_POLLING":
      return { ...state, phase: "polling" };
    default:
      return state;
  }
}

// --- Hook ---

export interface UseExportRouteProps {
  activityId: string;
  routeVersionId: string | undefined;
  initialExportId: string | null;
  onExportIdChange: (id: string | null) => void;
}

export interface UseExportRouteResult {
  phase: ExportPhase;
  exportId: string | null;
  error: string | null;
  failureReason: string | null;
  polledStatus: ExportStatus | null;
  isOnline: boolean;
  startExport: () => void;
  retry: () => void;
  getDownloadUrl: () => Promise<ExportDownloadResponse>;
  exportStatus: ExportStatusResponse | null;
}

export function useExportRoute({
  activityId,
  routeVersionId,
  initialExportId,
  onExportIdChange,
}: UseExportRouteProps): UseExportRouteResult {
  const queryClient = useQueryClient();
  const { isOnline } = useOnlineStatus();
  const idempotencyKeyRef = useRef<string>(crypto.randomUUID());
  const pollingStartRef = useRef<number | null>(null);

  const [state, setState] = useState<ExportState>(() => {
    if (initialExportId) {
      return { ...initialExportState, phase: "polling", exportId: initialExportId };
    }
    return initialExportState;
  });

  const dispatch = useCallback((action: ExportAction) => {
    setState((prev) => exportReducer(prev, action));
  }, []);

  // Track online/offline
  useEffect(() => {
    dispatch({ type: "SET_OFFLINE", isOffline: !isOnline });
  }, [isOnline, dispatch]);

  // Start polling timer when entering polling phase
  useEffect(() => {
    if (state.phase === "polling") {
      if (pollingStartRef.current === null) {
        pollingStartRef.current = Date.now();
      }
    } else {
      pollingStartRef.current = null;
    }
  }, [state.phase]);

  // Export ID changes - update URL
  const updateExportId = useCallback(
    (id: string | null) => {
      setState((prev) => ({ ...prev, exportId: id }));
      onExportIdChange(id);
    },
    [onExportIdChange],
  );

  // Request export mutation
  const requestMutation = useMutation({
    mutationFn: async () => {
      if (!routeVersionId) {
        throw new Error("Route version ID is required");
      }
      return requestExport(activityId, routeVersionId, idempotencyKeyRef.current);
    },
    onSuccess: (data) => {
      updateExportId(data.exportId);
      dispatch({ type: "REQUEST_SUCCESS", exportId: data.exportId });
    },
    onError: (err) => {
      if (err instanceof ApiError) {
        if (err.status === 401) {
          dispatch({ type: "REQUEST_UNAUTHORIZED" });
          return;
        }
        if (err.status === 409) {
          // Duplicate export - the error body should contain the existing exportId
          // Try to extract from the error message or use a convention
          // The API returns the existing export info in the error response
          const existingExportId = extractExportIdFromError(err);
          if (existingExportId) {
            updateExportId(existingExportId);
            dispatch({ type: "REQUEST_DUPLICATE", exportId: existingExportId });
            return;
          }
        }
      }
      dispatch({
        type: "REQUEST_FAILURE",
        error: err instanceof Error ? err.message : "Failed to request export",
      });
    },
  });

  // Poll export status
  const exportStatusQuery = useQuery<ExportStatusResponse>({
    queryKey: ["exportStatus", state.exportId],
    queryFn: () => getExportStatus(state.exportId!),
    enabled: !!state.exportId && state.phase === "polling",
    refetchInterval: (query) => {
      const status = query.state.data?.status;
      if (status && isTerminalStatus(status)) return false;
      return 2000;
    },
  });

  // Transition phase based on polling result, with timeout guard
  useEffect(() => {
    if (state.phase !== "polling") return;

    // Check polling timeout
    if (pollingStartRef.current !== null) {
      const elapsed = Date.now() - pollingStartRef.current;
      if (elapsed >= MAX_POLLING_DURATION_MS) {
        dispatch({
          type: "REQUEST_FAILURE",
          error: "Export is taking longer than expected. Please try again.",
        });
        return;
      }
    }

    if (!exportStatusQuery.data) return;
    const { status, failureReason } = exportStatusQuery.data;
    dispatch({ type: "POLL_UPDATE", status, failureReason });
  }, [exportStatusQuery.data, dispatch, state.phase]);

  // Handle polling errors (e.g., 401)
  useEffect(() => {
    if (!exportStatusQuery.error) return;
    const err = exportStatusQuery.error;
    if (err instanceof ApiError && err.status === 401) {
      dispatch({ type: "REQUEST_UNAUTHORIZED" });
    }
  }, [exportStatusQuery.error, dispatch]);

  const startExport = useCallback(() => {
    dispatch({ type: "START_REQUEST" });
    requestMutation.mutate();
  }, [dispatch, requestMutation]);

  const retry = useCallback(() => {
    // Reset idempotency key for new request
    idempotencyKeyRef.current = crypto.randomUUID();
    // Invalidate cached status query
    if (state.exportId) {
      void queryClient.invalidateQueries({
        queryKey: ["exportStatus", state.exportId],
      });
    }
    dispatch({ type: "RETRY" });
    onExportIdChange(null);
  }, [dispatch, state.exportId, queryClient, onExportIdChange]);

  const getDownloadUrl = useCallback(async (): Promise<ExportDownloadResponse> => {
    if (!state.exportId) {
      throw new Error("No export ID available for download");
    }
    return getExportDownloadUrl(state.exportId);
  }, [state.exportId]);

  return {
    phase: state.phase,
    exportId: state.exportId,
    error: state.error,
    failureReason: state.failureReason,
    polledStatus: state.polledStatus,
    isOnline,
    startExport,
    retry,
    getDownloadUrl,
    exportStatus: exportStatusQuery.data ?? null,
  };
}

/**
 * Attempts to extract an exportId from a 409 ApiError.
 * The backend includes the existing export ID in the response body as a UUID.
 * Falls back to regex-matching a UUID from the error message.
 */
export function extractExportIdFromError(err: ApiError): string | null {
  // Try the code field first - it may contain the export ID
  if (err.code && err.code !== "unknown") {
    const uuidPattern =
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
    if (uuidPattern.test(err.code)) {
      return err.code;
    }
  }

  // Fall back to extracting from the message text
  const uuidPattern =
    /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i;
  const match = err.message.match(uuidPattern);
  return match ? match[0] : null;
}
