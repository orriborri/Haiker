import { useCallback } from "react";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useImportActivity } from "./useImportActivity";
import { FilePickerDropZone } from "./FilePickerDropZone";
import { ImportProgress } from "./ImportProgress";
import { ImportResult } from "./ImportResult";

export function ImportActivity() {
  const navigate = useNavigate();
  const searchParams = useSearch({ strict: false }) as Record<string, string | undefined>;
  const initialImportId = searchParams.importId ?? null;

  const handleImportIdChange = useCallback(
    (id: string | null) => {
      void navigate({
        to: "/import",
        search: id ? { importId: id } : {},
        replace: true,
      });
    },
    [navigate],
  );

  const {
    phase,
    uploadProgress,
    importStatus,
    error,
    duplicateActivityId,
    startFileImport,
    reset,
  } = useImportActivity(initialImportId, handleImportIdChange);

  const handleBack = useCallback(() => {
    void navigate({ to: "/" });
  }, [navigate]);

  const activityId = importStatus?.activityId ?? null;

  return (
    <div className="mx-auto max-w-2xl">
      <header className="flex items-center gap-3 border-b border-gray-200 px-4 py-4">
        <button
          type="button"
          className="rounded-md p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700 focus:outline-none focus:ring-2 focus:ring-blue-500"
          onClick={handleBack}
          aria-label="Back to activities"
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
        <h1 className="text-xl font-semibold text-gray-900">Import Activity</h1>
      </header>

      <div className="px-4 py-6">
        <div aria-live="polite" aria-atomic="true" className="sr-only">
          {phase === "uploading" && `Uploading: ${uploadProgress}% complete`}
          {phase === "processing" && "Processing your file"}
          {phase === "completed" && "Import completed successfully"}
          {phase === "failed" && `Import failed: ${error ?? "Unknown error"}`}
          {phase === "duplicate" && "Duplicate file detected"}
        </div>

        {(phase === "idle" || phase === "starting") && (
          <FilePickerDropZone
            onFileSelected={startFileImport}
            disabled={phase === "starting"}
          />
        )}

        {(phase === "uploading" || phase === "completing" || phase === "processing") && (
          <ImportProgress
            uploadProgress={uploadProgress}
            isUploading={phase === "uploading" || phase === "completing"}
            importStatus={importStatus}
          />
        )}

        {(phase === "completed" || phase === "failed" || phase === "duplicate") && (
          <ImportResult
            phase={phase}
            activityId={activityId}
            duplicateActivityId={duplicateActivityId}
            error={error}
            onRetry={reset}
          />
        )}
      </div>
    </div>
  );
}
