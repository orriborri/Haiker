import type { SectionSelection } from "./types";

interface DeleteSectionConfirmDialogProps {
  selection: SectionSelection;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteSectionConfirmDialog({
  selection,
  onConfirm,
  onCancel,
}: DeleteSectionConfirmDialogProps) {
  const segmentDisplay = selection.segmentIndex + 1;
  const startDisplay = selection.startIndex + 1;
  const endDisplay = selection.endIndex + 1;
  const pointCount = selection.endIndex - selection.startIndex + 1;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="delete-section-dialog-title"
      aria-describedby="delete-section-dialog-description"
      onClick={onCancel}
    >
      <div
        className="mx-4 w-full max-w-sm rounded-md border border-gray-200 bg-white p-3 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <h2
          id="delete-section-dialog-title"
          className="text-base font-semibold text-gray-900"
        >
          Delete Section
        </h2>
        <p
          id="delete-section-dialog-description"
          className="mt-2 text-sm text-gray-600"
        >
          Delete {pointCount} {pointCount === 1 ? "point" : "points"} from
          segment {segmentDisplay} (points {startDisplay} to {endDisplay})?
        </p>

        <div className="mt-4 flex gap-2">
          <button
            type="button"
            className="min-h-[44px] min-w-[44px] flex-1 rounded bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 focus:ring-offset-1"
            onClick={onConfirm}
            autoFocus
          >
            Confirm Delete
          </button>
          <button
            type="button"
            className="min-h-[44px] min-w-[44px] flex-1 rounded bg-gray-100 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-gray-500 focus:ring-offset-1"
            onClick={onCancel}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
