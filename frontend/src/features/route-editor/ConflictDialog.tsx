import type { RouteDraftResponse } from "@/api/client";
import type { PendingOperation } from "./types";

interface ConflictDialogProps {
  serverDraft: RouteDraftResponse;
  localPendingOps: PendingOperation[];
  onReloadServerState: () => void;
  onRetryOperations: () => void;
  onDismiss: () => void;
}

export function ConflictDialog({
  serverDraft,
  localPendingOps,
  onReloadServerState,
  onRetryOperations,
  onDismiss,
}: ConflictDialogProps) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      role="dialog"
      aria-modal="true"
      aria-labelledby="conflict-dialog-title"
    >
      <div className="mx-4 w-full max-w-md rounded-lg border border-gray-200 bg-white p-6 shadow-xl">
        {/* Header */}
        <div className="mb-4 flex items-start gap-3">
          <svg
            className="mt-0.5 h-6 w-6 flex-shrink-0 text-amber-500"
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
          <div>
            <h2
              id="conflict-dialog-title"
              className="text-lg font-semibold text-gray-900"
            >
              Revision Conflict
            </h2>
            <p className="mt-1 text-sm text-gray-600">
              The draft was modified elsewhere. Your local changes conflict with
              the current server state.
            </p>
          </div>
        </div>

        {/* Server state info */}
        <div className="mb-4 rounded-md bg-gray-50 p-3">
          <dl className="space-y-1 text-sm">
            <div className="flex justify-between">
              <dt className="text-gray-500">Server revision</dt>
              <dd className="font-medium text-gray-900">
                {serverDraft.revision}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-gray-500">Server state</dt>
              <dd className="font-medium text-gray-900">
                {serverDraft.state}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-gray-500">Your pending operations</dt>
              <dd className="font-medium text-gray-900">
                {localPendingOps.length}
              </dd>
            </div>
          </dl>
        </div>

        {/* Actions */}
        <div className="flex flex-col gap-2">
          <button
            type="button"
            className="w-full rounded bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
            onClick={onReloadServerState}
          >
            Accept Server State
          </button>
          <button
            type="button"
            className="w-full rounded bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 focus:outline-none focus:ring-2 focus:ring-amber-500 focus:ring-offset-1"
            onClick={onRetryOperations}
          >
            Retry My Changes
          </button>
          <button
            type="button"
            className="w-full rounded bg-gray-100 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-gray-500 focus:ring-offset-1"
            onClick={onDismiss}
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
}
