import { useState, useCallback } from "react";
import type { EditorTool } from "./types";

interface EditorToolbarProps {
  currentTool: EditorTool;
  onToolChange: (tool: EditorTool) => void;
  canUndo: boolean;
  canRedo: boolean;
  onUndo: () => void;
  onRedo: () => void;
  onReset: () => void;
  onDelete: () => void;
  onSplit: () => void;
  onJoin: () => void;
  hasSelection: boolean;
  isOperationPending: boolean;
}

const TOOLS: Array<{ id: EditorTool; label: string; shortLabel: string }> = [
  { id: "select", label: "Select point or section", shortLabel: "Select" },
  { id: "move", label: "Move selected point", shortLabel: "Move" },
  { id: "add", label: "Add point on segment", shortLabel: "Add Point" },
  { id: "delete", label: "Delete selected element", shortLabel: "Delete" },
  { id: "split", label: "Split segment at point", shortLabel: "Split" },
  { id: "join", label: "Join adjacent segments", shortLabel: "Join" },
  { id: "draw-section", label: "Draw replacement section", shortLabel: "Draw Section" },
];

export function EditorToolbar({
  currentTool,
  onToolChange,
  canUndo,
  canRedo,
  onUndo,
  onRedo,
  onReset,
  onDelete,
  onSplit,
  onJoin,
  hasSelection,
  isOperationPending,
}: EditorToolbarProps) {
  const [showResetConfirm, setShowResetConfirm] = useState(false);

  const handleReset = useCallback(() => {
    setShowResetConfirm(true);
  }, []);

  const confirmReset = useCallback(() => {
    setShowResetConfirm(false);
    onReset();
  }, [onReset]);

  const cancelReset = useCallback(() => {
    setShowResetConfirm(false);
  }, []);

  return (
    <nav
      className="flex flex-wrap items-center gap-2 border-b border-gray-200 bg-white px-4 py-2"
      role="toolbar"
      aria-label="Route editing tools"
    >
      {/* Tool mode buttons */}
      <div className="flex gap-1" role="radiogroup" aria-label="Editing mode">
        {TOOLS.map((tool) => (
          <button
            key={tool.id}
            type="button"
            role="radio"
            aria-checked={currentTool === tool.id}
            aria-label={tool.label}
            className={`rounded px-3 py-1.5 text-sm font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1 ${
              currentTool === tool.id
                ? "bg-blue-600 text-white"
                : "bg-gray-100 text-gray-700 hover:bg-gray-200"
            }`}
            onClick={() => onToolChange(tool.id)}
            disabled={isOperationPending}
          >
            {tool.shortLabel}
          </button>
        ))}
      </div>

      {/* Separator */}
      <div className="mx-2 h-6 w-px bg-gray-200" aria-hidden="true" />

      {/* Action buttons */}
      <div className="flex gap-1">
        <button
          type="button"
          aria-label="Delete selected element"
          className="rounded px-3 py-1.5 text-sm font-medium text-red-700 bg-red-50 hover:bg-red-100 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-red-500 focus:ring-offset-1"
          onClick={onDelete}
          disabled={!hasSelection || isOperationPending}
        >
          Delete
        </button>
        <button
          type="button"
          aria-label="Split segment at selected point"
          className="rounded px-3 py-1.5 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onSplit}
          disabled={!hasSelection || isOperationPending}
        >
          Split
        </button>
        <button
          type="button"
          aria-label="Join adjacent segments"
          className="rounded px-3 py-1.5 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onJoin}
          disabled={isOperationPending}
        >
          Join
        </button>
      </div>

      {/* Separator */}
      <div className="mx-2 h-6 w-px bg-gray-200" aria-hidden="true" />

      {/* Undo/Redo */}
      <div className="flex gap-1">
        <button
          type="button"
          aria-label="Undo last operation (Ctrl+Z)"
          className="rounded px-3 py-1.5 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onUndo}
          disabled={!canUndo || isOperationPending}
        >
          Undo
        </button>
        <button
          type="button"
          aria-label="Redo last undone operation (Ctrl+Shift+Z)"
          className="rounded px-3 py-1.5 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onRedo}
          disabled={!canRedo || isOperationPending}
        >
          Redo
        </button>
      </div>

      {/* Separator */}
      <div className="mx-2 h-6 w-px bg-gray-200" aria-hidden="true" />

      {/* Reset */}
      <div className="relative">
        <button
          type="button"
          aria-label="Reset route to original"
          className="rounded px-3 py-1.5 text-sm font-medium text-orange-700 bg-orange-50 hover:bg-orange-100 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-1"
          onClick={handleReset}
          disabled={isOperationPending}
        >
          Reset
        </button>

        {showResetConfirm && (
          <div
            className="absolute left-0 top-full z-10 mt-1 rounded-md border border-gray-200 bg-white p-3 shadow-lg"
            role="alertdialog"
            aria-label="Confirm reset"
          >
            <p className="mb-2 text-sm text-gray-700">
              Reset all changes? This cannot be undone.
            </p>
            <div className="flex gap-2">
              <button
                type="button"
                className="rounded bg-orange-600 px-3 py-1 text-sm font-medium text-white hover:bg-orange-700 focus:outline-none focus:ring-2 focus:ring-orange-500"
                onClick={confirmReset}
              >
                Confirm Reset
              </button>
              <button
                type="button"
                className="rounded bg-gray-100 px-3 py-1 text-sm font-medium text-gray-700 hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-gray-500"
                onClick={cancelReset}
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>
    </nav>
  );
}
