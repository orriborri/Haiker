import { useState, useCallback, useEffect, useRef } from "react";
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
  onValidate: () => void;
  hasSelection: boolean;
  isOperationPending: boolean;
  isOffline?: boolean;
  selectionDescription: string | null;
  validationResult: { valid: boolean; errors: Array<{ code: string; detail: string }> } | null;
  isValidating?: boolean;
}

const TOOLS: Array<{
  id: EditorTool;
  label: string;
  shortLabel: string;
  abbreviation: string;
  key: string;
}> = [
  { id: "select", label: "Select point or section", shortLabel: "Select", abbreviation: "S", key: "1" },
  { id: "move", label: "Move selected point", shortLabel: "Move", abbreviation: "M", key: "2" },
  { id: "add", label: "Add point on segment", shortLabel: "Add Point", abbreviation: "A", key: "3" },
  { id: "delete", label: "Delete selected element", shortLabel: "Delete", abbreviation: "D", key: "4" },
  { id: "split", label: "Split segment at point", shortLabel: "Split", abbreviation: "Sp", key: "5" },
  { id: "join", label: "Join adjacent segments", shortLabel: "Join", abbreviation: "J", key: "6" },
  { id: "draw-section", label: "Draw replacement section", shortLabel: "Draw Section", abbreviation: "Dr", key: "7" },
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
  onValidate,
  hasSelection,
  isOperationPending,
  isOffline = false,
  selectionDescription,
  validationResult,
  isValidating = false,
}: EditorToolbarProps) {
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const toolbarRef = useRef<HTMLElement>(null);
  const isDisabled = isOperationPending || isOffline;

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

  // Keyboard shortcuts: only number keys 1-7 for tool switching.
  // Escape, Delete/Backspace, and undo/redo are handled by RouteEditor.tsx.
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Ignore if focus is inside an input/textarea/contenteditable
      const target = e.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable ||
        target.getAttribute("role") === "textbox"
      ) return;

      // Number keys 1-7 to switch tools
      const keyNum = parseInt(e.key, 10);
      if (keyNum >= 1 && keyNum <= 7 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const tool = TOOLS[keyNum - 1];
        if (tool && !isDisabled) {
          e.preventDefault();
          onToolChange(tool.id);
        }
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onToolChange, isDisabled]);

  // Build the status announcement text
  const currentToolDef = TOOLS.find((t) => t.id === currentTool);
  const statusText = [
    currentToolDef ? `Tool: ${currentToolDef.shortLabel}` : "",
    selectionDescription ? `${selectionDescription}` : "No selection",
  ]
    .filter(Boolean)
    .join(". ");

  return (
    <nav
      ref={toolbarRef}
      className="flex min-h-[3.5rem] flex-wrap items-center gap-2 overflow-x-auto border-b border-gray-200 bg-white px-4 py-2"
      role="toolbar"
      aria-label="Route editing tools"
    >
      {/* Visually hidden aria-live status region */}
      <div className="sr-only" aria-live="polite" aria-atomic="true">
        {statusText}
      </div>

      {/* Tool mode buttons */}
      <div className="flex flex-wrap gap-1" role="radiogroup" aria-label="Editing mode">
        {TOOLS.map((tool) => (
          <button
            key={tool.id}
            type="button"
            role="radio"
            aria-checked={currentTool === tool.id}
            aria-label={`${tool.label} (${tool.key})`}
            className={`min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1 ${
              currentTool === tool.id
                ? "bg-blue-600 text-white"
                : "bg-gray-100 text-gray-700 hover:bg-gray-200"
            }`}
            onClick={() => onToolChange(tool.id)}
            disabled={isDisabled}
          >
            {/* Mobile: abbreviation, Desktop: short label */}
            <span className="sm:hidden">{tool.abbreviation}</span>
            <span className="hidden sm:inline">{tool.shortLabel}</span>
          </button>
        ))}
      </div>

      {/* Separator */}
      <div className="mx-1 hidden h-6 w-px bg-gray-200 sm:mx-2 sm:block" aria-hidden="true" />

      {/* Action buttons - wraps to second row on small screens */}
      <div className="flex flex-wrap gap-1">
        <button
          type="button"
          aria-label="Delete selected element"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-red-700 bg-red-50 hover:bg-red-100 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-red-500 focus:ring-offset-1"
          onClick={onDelete}
          disabled={!hasSelection || isDisabled}
        >
          <span className="sm:hidden">Del</span>
          <span className="hidden sm:inline">Delete</span>
        </button>
        <button
          type="button"
          aria-label="Split segment at selected point"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onSplit}
          disabled={!hasSelection || isDisabled}
        >
          Split
        </button>
        <button
          type="button"
          aria-label="Join adjacent segments"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onJoin}
          disabled={isDisabled}
        >
          Join
        </button>
      </div>

      {/* Separator */}
      <div className="mx-1 hidden h-6 w-px bg-gray-200 sm:mx-2 sm:block" aria-hidden="true" />

      {/* Undo/Redo */}
      <div className="flex gap-1">
        <button
          type="button"
          aria-label="Undo last operation (Ctrl+Z)"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onUndo}
          disabled={!canUndo || isDisabled}
        >
          Undo
        </button>
        <button
          type="button"
          aria-label="Redo last undone operation (Ctrl+Shift+Z)"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onRedo}
          disabled={!canRedo || isDisabled}
        >
          Redo
        </button>
      </div>

      {/* Separator */}
      <div className="mx-1 hidden h-6 w-px bg-gray-200 sm:mx-2 sm:block" aria-hidden="true" />

      {/* Reset */}
      <div className="relative">
        <button
          type="button"
          aria-label="Reset route to original"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-orange-700 bg-orange-50 hover:bg-orange-100 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-1"
          onClick={handleReset}
          disabled={isDisabled}
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
                className="min-h-[44px] rounded bg-orange-600 px-3 py-2 text-sm font-medium text-white hover:bg-orange-700 focus:outline-none focus:ring-2 focus:ring-orange-500"
                onClick={confirmReset}
              >
                Confirm Reset
              </button>
              <button
                type="button"
                className="min-h-[44px] rounded bg-gray-100 px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-gray-500"
                onClick={cancelReset}
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Offline indicator */}
      {isOffline && (
        <div className="ml-auto flex items-center gap-1 text-sm text-gray-500" aria-hidden="true">
          <svg
            className="h-4 w-4"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
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
          <span>Offline</span>
        </div>
      )}

      {/* Separator before Validate */}
      <div className="mx-1 hidden h-6 w-px bg-gray-200 sm:mx-2 sm:block" aria-hidden="true" />

      {/* Validate */}
      <button
        type="button"
        aria-label="Validate draft for publication"
        className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-green-700 bg-green-50 hover:bg-green-100 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-green-500 focus:ring-offset-1"
        onClick={onValidate}
        disabled={isDisabled || isValidating}
      >
        {isValidating ? "Validating..." : "Validate"}
      </button>

      {/* Validation result feedback */}
      {validationResult && (
        <div
          className={`ml-2 flex items-center gap-2 rounded px-3 py-1 text-sm ${
            validationResult.valid
              ? "bg-green-50 text-green-800"
              : "bg-red-50 text-red-800"
          }`}
          role="alert"
          aria-live="polite"
        >
          {validationResult.valid ? (
            <span>Valid for publication</span>
          ) : (
            <div>
              <span className="font-medium">Validation failed:</span>
              <ul className="mt-1 list-inside list-disc">
                {validationResult.errors.map((err, idx) => (
                  <li key={idx}>{err.detail}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </nav>
  );
}
