import { useState, useCallback, useRef } from "react";

const MAX_FILE_SIZE = 52_428_800; // 50 MB

interface FilePickerDropZoneProps {
  onFileSelected: (file: File) => void;
  disabled?: boolean;
}

function validateFile(file: File): string | null {
  const name = file.name.toLowerCase();
  if (!name.endsWith(".gpx")) {
    return "Only GPX files are accepted";
  }
  if (file.size === 0) {
    return "File is empty";
  }
  if (file.size > MAX_FILE_SIZE) {
    return "File must be smaller than 50 MB";
  }
  return null;
}

export function FilePickerDropZone({
  onFileSelected,
  disabled = false,
}: FilePickerDropZoneProps) {
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [validationError, setValidationError] = useState<string | null>(null);
  const [isDragOver, setIsDragOver] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const handleFile = useCallback((file: File) => {
    const error = validateFile(file);
    if (error) {
      setValidationError(error);
      setSelectedFile(null);
      return;
    }
    setValidationError(null);
    setSelectedFile(file);
  }, []);

  const handleInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (file) handleFile(file);
    },
    [handleFile],
  );

  const handleDrop = useCallback(
    (e: React.DragEvent<HTMLDivElement>) => {
      e.preventDefault();
      setIsDragOver(false);
      const file = e.dataTransfer.files[0];
      if (file) handleFile(file);
    },
    [handleFile],
  );

  const handleDragOver = useCallback((e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDragOver(false);
  }, []);

  const handleClick = useCallback(() => {
    inputRef.current?.click();
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        handleClick();
      }
    },
    [handleClick],
  );

  const handleStartImport = useCallback(() => {
    if (selectedFile) {
      onFileSelected(selectedFile);
    }
  }, [selectedFile, onFileSelected]);

  const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <div className="space-y-4">
      <div
        className={`flex cursor-pointer flex-col items-center justify-center rounded-lg border-2 border-dashed p-8 transition-colors ${
          isDragOver
            ? "border-blue-500 bg-blue-50"
            : "border-gray-300 hover:border-gray-400 hover:bg-gray-50"
        } ${disabled ? "pointer-events-none opacity-50" : ""}`}
        onDrop={handleDrop}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onClick={handleClick}
        onKeyDown={handleKeyDown}
        role="button"
        tabIndex={0}
        aria-label="Choose a GPX file or drag and drop"
      >
        <svg
          className="mb-3 h-10 w-10 text-gray-400"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"
          />
        </svg>
        <p className="text-sm font-medium text-gray-700">
          Drop your GPX file here, or click to browse
        </p>
        <p className="mt-1 text-xs text-gray-500">GPX files up to 50 MB</p>
        <label htmlFor="gpx-file-input" className="sr-only">
          Choose a GPX file
        </label>
        <input
          ref={inputRef}
          id="gpx-file-input"
          type="file"
          className="hidden"
          accept=".gpx,application/gpx+xml,application/xml"
          onChange={handleInputChange}
          disabled={disabled}
        />
      </div>

      {validationError && (
        <div
          className="rounded-md bg-red-50 px-4 py-3 text-sm text-red-700"
          role="alert"
        >
          {validationError}
        </div>
      )}

      {selectedFile && !validationError && (
        <div className="flex items-center justify-between rounded-md bg-gray-50 px-4 py-3">
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium text-gray-900">
              {selectedFile.name}
            </p>
            <p className="text-xs text-gray-500">
              {formatFileSize(selectedFile.size)}
            </p>
          </div>
          <button
            type="button"
            className="ml-4 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 disabled:opacity-50"
            onClick={handleStartImport}
            disabled={disabled}
          >
            Start Import
          </button>
        </div>
      )}
    </div>
  );
}
