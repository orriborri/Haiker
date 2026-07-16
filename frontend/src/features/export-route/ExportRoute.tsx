import { useCallback } from "react";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useExportRoute } from "./useExportRoute";
import { ExportProgress } from "./ExportProgress";
import { ExportReady } from "./ExportReady";
import { ExportFailed } from "./ExportFailed";
import { ExportExpired } from "./ExportExpired";
import { useDocumentTitle } from "@/hooks/useDocumentTitle";

interface ExportRouteProps {
  activityId: string;
}

function getAnnouncementText(
  phase: string,
  error: string | null,
): string {
  switch (phase) {
    case "idle":
      return "Ready to export";
    case "requesting":
      return "Requesting export";
    case "polling":
      return "Export in progress";
    case "ready":
      return "Export ready for download";
    case "failed":
      return `Export failed: ${error ?? "Unknown error"}`;
    case "expired":
      return "Export has expired";
    case "offline":
      return "You are offline. Export is unavailable.";
    case "unauthorized":
      return "Unauthorized. Please sign in to export.";
    default:
      return "";
  }
}

export function ExportRoute({ activityId }: ExportRouteProps) {
  useDocumentTitle("Export Route");
  const navigate = useNavigate();
  const { exportId: exportIdParam, routeVersionId: routeVersionIdParam } =
    useSearch({ from: "/activities/$activityId/export" });

  const initialExportId = exportIdParam ?? null;
  const routeVersionId = routeVersionIdParam;

  const handleExportIdChange = useCallback(
    (id: string | null) => {
      void navigate({
        to: "/activities/$activityId/export",
        params: { activityId },
        search: {
          ...(id ? { exportId: id } : {}),
          ...(routeVersionId ? { routeVersionId } : {}),
        },
        replace: true,
      });
    },
    [navigate, activityId, routeVersionId],
  );

  const {
    phase,
    error,
    failureReason,
    polledStatus,
    isOnline,
    startExport,
    retry,
    getDownloadUrl,
    exportStatus,
  } = useExportRoute({
    activityId,
    routeVersionId,
    initialExportId,
    onExportIdChange: handleExportIdChange,
  });

  const handleBack = useCallback(() => {
    void navigate({
      to: "/activities/$activityId",
      params: { activityId },
    });
  }, [navigate, activityId]);

  return (
    <div className="mx-auto max-w-2xl">
      <header className="flex items-center gap-3 border-b border-gray-200 px-4 py-4">
        <button
          type="button"
          className="rounded-md p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700 focus:outline-none focus:ring-2 focus:ring-blue-500"
          onClick={handleBack}
          aria-label="Back to activity"
        >
          <svg
            className="h-5 w-5"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M15 19l-7-7 7-7"
            />
          </svg>
        </button>
        <h1 className="text-xl font-semibold text-gray-900">Export Route</h1>
      </header>

      <div className="px-4 py-6">
        {/* Route version info - always visible */}
        {routeVersionId && (
          <div className="mb-4 rounded-md bg-gray-50 px-3 py-2">
            <p className="text-sm text-gray-600">
              <span className="font-medium">Version:</span>{" "}
              <span data-testid="route-version-id">{routeVersionId}</span>
            </p>
          </div>
        )}

        {/* Accessible announcement region */}
        <div aria-live="polite" aria-atomic="true" className="sr-only">
          {getAnnouncementText(phase, error)}
        </div>

        {/* Offline banner */}
        {!isOnline && (
          <div
            className="mb-4 rounded-md bg-yellow-50 border border-yellow-200 px-4 py-3"
            role="alert"
          >
            <p className="text-sm text-yellow-800">
              You are offline. Export is unavailable until your connection is
              restored.
            </p>
          </div>
        )}

        {/* Phase-conditional rendering */}
        {(phase === "idle" || phase === "offline") && (
          <div className="flex flex-col items-center space-y-4 py-8 text-center">
            <p className="text-sm text-gray-600">
              Export your route as a GPX file for use in other applications.
            </p>
            <button
              type="button"
              className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={startExport}
              disabled={!isOnline || !routeVersionId}
              aria-label="Export as GPX"
            >
              Export as GPX
            </button>
          </div>
        )}

        {phase === "requesting" && (
          <div className="flex flex-col items-center space-y-4 py-8">
            <div className="h-10 w-10 animate-spin rounded-full border-4 border-gray-200 border-t-blue-600" />
            <p className="text-sm font-medium text-gray-700">
              Requesting export...
            </p>
          </div>
        )}

        {phase === "polling" && <ExportProgress status={polledStatus} />}

        {phase === "ready" && (
          <ExportReady
            exportStatus={exportStatus}
            getDownloadUrl={getDownloadUrl}
          />
        )}

        {phase === "failed" && (
          <ExportFailed failureReason={failureReason ?? error} onRetry={retry} />
        )}

        {phase === "expired" && <ExportExpired onRetry={retry} />}

        {phase === "unauthorized" && (
          <div className="flex flex-col items-center space-y-4 py-8 text-center">
            <div className="flex h-12 w-12 items-center justify-center rounded-full bg-gray-100">
              <svg
                className="h-6 w-6 text-gray-600"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                aria-hidden="true"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"
                />
              </svg>
            </div>
            <h2 className="text-lg font-medium text-gray-900">
              Unauthorized
            </h2>
            <p className="text-sm text-gray-500">
              {error ?? "You are not authorized to export this route. Please sign in and try again."}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
