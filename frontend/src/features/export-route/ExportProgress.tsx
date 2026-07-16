import { getProgressLabel, type ExportStatus } from "./useExportRoute";

interface ExportProgressProps {
  status: ExportStatus | null;
}

export function ExportProgress({ status }: ExportProgressProps) {
  const statusText = status ? getProgressLabel(status) : "Processing...";

  return (
    <div className="flex flex-col items-center space-y-4 py-8">
      <div className="h-10 w-10 animate-spin rounded-full border-4 border-gray-200 border-t-blue-600" />
      <p className="text-sm font-medium text-gray-700">
        {statusText}
      </p>
    </div>
  );
}
