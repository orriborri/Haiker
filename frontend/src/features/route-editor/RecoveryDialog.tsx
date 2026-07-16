import { FocusTrap } from "@/components/FocusTrap";
import type { PendingOperation } from "./types";

interface RecoveryDialogProps {
  pendingOperations: PendingOperation[];
  isOffline: boolean;
  onReplay: () => void;
  onDiscard: () => void;
}

/**
 * Modal dialog shown when unconfirmed operations are found after reload.
 * Offers the user a choice to replay pending operations or discard them.
 */
export function RecoveryDialog({
  pendingOperations,
  isOffline,
  onReplay,
  onDiscard,
}: RecoveryDialogProps) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      role="dialog"
      aria-modal="true"
      aria-labelledby="recovery-dialog-title"
    >
      <FocusTrap onEscape={onDiscard}>
        <div className="mx-4 w-full max-w-md rounded-lg border border-gray-200 bg-white p-6 shadow-xl">
          {/* Header */}
          <div className="mb-4 flex items-start gap-3">
            <svg
              className="mt-0.5 h-6 w-6 flex-shrink-0 text-blue-500"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              aria-hidden="true"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
              />
            </svg>
            <div>
              <h2
                id="recovery-dialog-title"
                className="text-lg font-semibold text-gray-900"
              >
                Recover Unsaved Changes
              </h2>
              <p className="mt-1 text-sm text-gray-600">
                {pendingOperations.length} unsaved operation
                {pendingOperations.length === 1 ? " was" : "s were"} found from a
                previous session. Would you like to replay them or discard?
              </p>
            </div>
          </div>

          {/* Details */}
          <div className="mb-4 rounded-md bg-gray-50 p-3">
            <dl className="space-y-1 text-sm">
              <div className="flex justify-between">
                <dt className="text-gray-500">Pending operations</dt>
                <dd className="font-medium text-gray-900">
                  {pendingOperations.length}
                </dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-gray-500">Last operation at</dt>
                <dd className="font-medium text-gray-900">
                  {pendingOperations.length > 0
                    ? new Date(
                        pendingOperations[pendingOperations.length - 1]!.timestamp,
                      ).toLocaleString()
                    : "N/A"}
                </dd>
              </div>
            </dl>
          </div>

          {/* Actions */}
          <div className="flex flex-col gap-2">
            <button
              type="button"
              className="w-full rounded bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1 disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={onReplay}
              disabled={isOffline}
              aria-label={isOffline ? "Replay unavailable while offline" : "Replay pending operations"}
              title={isOffline ? "Replay is unavailable while offline" : undefined}
            >
              Replay Operations
            </button>
            <button
              type="button"
              className="w-full rounded bg-gray-100 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-gray-500 focus:ring-offset-1"
              onClick={onDiscard}
              aria-label="Discard unsaved changes"
            >
              Discard Changes
            </button>
          </div>
        </div>
      </FocusTrap>
    </div>
  );
}
