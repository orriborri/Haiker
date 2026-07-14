import type { ImportStatusResponse } from "@/api/client";

interface ImportProgressProps {
  uploadProgress: number;
  isUploading: boolean;
  importStatus: ImportStatusResponse | null;
}

function getStatusLabel(status: string): string {
  switch (status) {
    case "requested":
      return "Preparing your import...";
    case "uploading":
      return "Uploading your file...";
    case "uploaded":
      return "Upload complete, starting validation...";
    case "validating":
      return "Validating your file...";
    case "queued":
      return "Queued for processing...";
    case "parsing":
      return "Parsing GPS data...";
    case "committing":
      return "Almost done...";
    default:
      return "Processing...";
  }
}

export function ImportProgress({
  uploadProgress,
  isUploading,
  importStatus,
}: ImportProgressProps) {
  if (isUploading) {
    return (
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-gray-700">
            Uploading file...
          </span>
          <span className="text-sm text-gray-500">{uploadProgress}%</span>
        </div>
        <div
          className="h-2.5 w-full overflow-hidden rounded-full bg-gray-200"
          role="progressbar"
          aria-valuenow={uploadProgress}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label="Upload progress"
        >
          <div
            className="h-full rounded-full bg-blue-600 transition-all duration-300"
            style={{ width: `${uploadProgress}%` }}
          />
        </div>
        <p className="text-xs text-gray-500">
          Please do not close this page while uploading.
        </p>
      </div>
    );
  }

  const statusText = importStatus
    ? getStatusLabel(importStatus.status)
    : "Processing...";

  return (
    <div className="flex flex-col items-center space-y-4 py-8">
      <div className="h-10 w-10 animate-spin rounded-full border-4 border-gray-200 border-t-blue-600" />
      <p
        className="text-sm font-medium text-gray-700"
        aria-live="polite"
        aria-atomic="true"
      >
        {statusText}
      </p>
    </div>
  );
}
