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
  hasSelection: boolean;
  isOperationPending: boolean;
  selectionDescription: string | null;
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
  hasSelection,
  isOperationPending,
  selectionDescription,
}: EditorToolbarProps) {
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const toolbarRef = useRef<HTMLElement>(null);

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
        if (tool && !isOperationPending) {
          e.preventDefault();
          onToolChange(tool.id);
        }
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onToolChange, isOperationPending]);

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
            disabled={isOperationPending}
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
          disabled={!hasSelection || isOperationPending}
        >
          <span className="sm:hidden">Del</span>
          <span className="hidden sm:inline">Delete</span>
        </button>
        <button
          type="button"
          aria-label="Split segment at selected point"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onSplit}
          disabled={!hasSelection || isOperationPending}
        >
          Split
        </button>
        <button
          type="button"
          aria-label="Join adjacent segments"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onJoin}
          disabled={isOperationPending}
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
          disabled={!canUndo || isOperationPending}
        >
          Undo
        </button>
        <button
          type="button"
          aria-label="Redo last undone operation (Ctrl+Shift+Z)"
          className="min-w-[44px] min-h-[44px] rounded px-3 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 disabled:opacity-40 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1"
          onClick={onRedo}
          disabled={!canRedo || isOperationPending}
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
    </nav>
  );
}
