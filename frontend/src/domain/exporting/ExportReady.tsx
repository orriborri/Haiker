import { useState, useCallback } from "react";
import type { ExportDownloadResponse, ExportStatusResponse } from "@/api/client";

interface ExportReadyProps {
  exportStatus: ExportStatusResponse | null;
  getDownloadUrl: () => Promise<ExportDownloadResponse>;
}

export function ExportReady({ exportStatus, getDownloadUrl }: ExportReadyProps) {
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);

  const handleDownload = useCallback(async () => {
    setIsDownloading(true);
    setDownloadError(null);
    try {
      const result = await getDownloadUrl();
      window.location.assign(result.downloadUrl);
    } catch {
      setDownloadError("Failed to get download link. Please try again.");
    } finally {
      setIsDownloading(false);
    }
  }, [getDownloadUrl]);

  const expiryDate = exportStatus?.downloadAvailableUntil
    ? new Date(exportStatus.downloadAvailableUntil).toLocaleString()
    : null;

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
      <h2 className="text-lg font-medium text-gray-900">Export ready</h2>
      {expiryDate && (
        <p className="text-sm text-gray-500">
          Available until {expiryDate}
        </p>
      )}
      <button
        type="button"
        className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 disabled:opacity-50"
        onClick={() => void handleDownload()}
        disabled={isDownloading}
        aria-label="Download GPX file"
      >
        {isDownloading ? "Preparing download..." : "Download GPX"}
      </button>
      {downloadError && (
        <p className="text-sm text-red-600" role="alert">
          {downloadError}
        </p>
      )}
    </div>
  );
}
