import { formatDistance } from "./geo-utils";
import { MAX_REPLACEMENT_POINTS } from "./geo-utils";

interface DrawingSectionPanelProps {
  currentDistance: number;
  pointCount: number;
  onUndoLastPoint: () => void;
  onFinish: () => void;
  onCancel: () => void;
}

export function DrawingSectionPanel({
  currentDistance,
  pointCount,
  onUndoLastPoint,
  onFinish,
  onCancel,
}: DrawingSectionPanelProps) {
  const canFinish = pointCount >= 2;
  const isNearLimit = pointCount >= MAX_REPLACEMENT_POINTS - 50;
  const isAtLimit = pointCount >= MAX_REPLACEMENT_POINTS - 1;

  return (
    <div
      className="flex flex-wrap items-center gap-3 border-b border-green-200 bg-green-50 px-4 py-2"
      role="region"
      aria-label="Drawing replacement section controls"
    >
      {/* Instructions */}
      <p className="text-sm text-green-800">
        Click on the map to draw replacement points. Press Enter or click Finish to complete.
      </p>

      {/* Live distance */}
      <div aria-live="polite" aria-atomic="true" className="text-sm font-medium text-green-900">
        {formatDistance(currentDistance)}
      </div>

      {/* Point count */}
      <div
        className={`text-sm font-medium ${isAtLimit ? "text-red-700" : isNearLimit ? "text-amber-700" : "text-green-700"}`}
        aria-label={`${pointCount} of ${MAX_REPLACEMENT_POINTS} points`}
      >
        {pointCount} / {MAX_REPLACEMENT_POINTS} points
      </div>

      {/* Warning when near limit */}
      {isNearLimit && !isAtLimit && (
        <span className="text-xs text-amber-600" role="alert">
          Approaching point limit
        </span>
      )}
      {isAtLimit && (
        <span className="text-xs text-red-600" role="alert">
          Point limit reached
        </span>
      )}

      {/* Action buttons */}
      <div className="ml-auto flex gap-2">
        <button
          type="button"
          className="min-h-[36px] rounded bg-green-100 px-3 py-1.5 text-sm font-medium text-green-800 hover:bg-green-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-green-500 focus:ring-offset-1"
          onClick={onUndoLastPoint}
          disabled={pointCount <= 1}
          aria-label="Undo last point"
        >
          Undo Last Point
        </button>
        <button
          type="button"
          className="min-h-[36px] rounded bg-green-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-green-700 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-green-500 focus:ring-offset-1"
          onClick={onFinish}
          disabled={!canFinish}
          aria-label="Finish drawing replacement section"
        >
          Finish
        </button>
        <button
          type="button"
          className="min-h-[36px] rounded bg-gray-100 px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-gray-500 focus:ring-offset-1"
          onClick={onCancel}
          aria-label="Cancel drawing"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
