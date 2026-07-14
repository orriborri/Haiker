import { useNavigate } from "@tanstack/react-router";
import { useCallback } from "react";

interface ImportResultProps {
  phase: "completed" | "failed" | "duplicate";
  activityId: string | null;
  duplicateActivityId: string | null;
  error: string | null;
  onRetry: () => void;
}

function SuccessResult({ activityId }: { activityId: string | null }) {
  const navigate = useNavigate();

  const handleViewActivity = useCallback(() => {
    if (activityId) {
      void navigate({
        to: "/activities/$activityId",
        params: { activityId },
      });
    }
  }, [navigate, activityId]);

  return (
    <div className="flex flex-col items-center space-y-4 py-8 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-green-100">
        <svg
          className="h-6 w-6 text-green-600"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M5 13l4 4L19 7"
          />
        </svg>
      </div>
      <h2 className="text-lg font-medium text-gray-900">
        Import completed successfully
      </h2>
      <p className="text-sm text-gray-500">
        Your activity has been imported and is ready to view.
      </p>
      {activityId && (
        <button
          type="button"
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
          onClick={handleViewActivity}
        >
          View Activity
        </button>
      )}
    </div>
  );
}

function FailureResult({
  error,
  onRetry,
}: {
  error: string | null;
  onRetry: () => void;
}) {
  return (
    <div className="flex flex-col items-center space-y-4 py-8 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-red-100">
        <svg
          className="h-6 w-6 text-red-600"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M6 18L18 6M6 6l12 12"
          />
        </svg>
      </div>
      <h2 className="text-lg font-medium text-gray-900">Import failed</h2>
      {error && <p className="text-sm text-gray-500">{error}</p>}
      <button
        type="button"
        className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
        onClick={onRetry}
      >
        Try Again
      </button>
    </div>
  );
}

function DuplicateResult({
  duplicateActivityId,
  onRetry,
}: {
  duplicateActivityId: string | null;
  onRetry: () => void;
}) {
  const navigate = useNavigate();

  const handleViewExisting = useCallback(() => {
    if (duplicateActivityId) {
      void navigate({
        to: "/activities/$activityId",
        params: { activityId: duplicateActivityId },
      });
    }
  }, [navigate, duplicateActivityId]);

  return (
    <div className="flex flex-col items-center space-y-4 py-8 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-yellow-100">
        <svg
          className="h-6 w-6 text-yellow-600"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z"
          />
        </svg>
      </div>
      <h2 className="text-lg font-medium text-gray-900">
        Duplicate file detected
      </h2>
      <p className="text-sm text-gray-500">
        This file has already been imported.
      </p>
      <div className="flex gap-3">
        {duplicateActivityId && (
          <button
            type="button"
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
            onClick={handleViewExisting}
          >
            View Existing Activity
          </button>
        )}
        <button
          type="button"
          className="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
          onClick={onRetry}
        >
          Import Different File
        </button>
      </div>
    </div>
  );
}

export function ImportResult({
  phase,
  activityId,
  duplicateActivityId,
  error,
  onRetry,
}: ImportResultProps) {
  switch (phase) {
    case "completed":
      return <SuccessResult activityId={activityId} />;
    case "failed":
      return <FailureResult error={error} onRetry={onRetry} />;
    case "duplicate":
      return (
        <DuplicateResult
          duplicateActivityId={duplicateActivityId}
          onRetry={onRetry}
        />
      );
  }
}
