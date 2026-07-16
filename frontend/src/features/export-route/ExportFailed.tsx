interface ExportFailedProps {
  failureReason: string | null;
  onRetry: () => void;
}

export function ExportFailed({ failureReason, onRetry }: ExportFailedProps) {
  return (
    <div className="flex flex-col items-center space-y-4 py-8 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-red-100">
        <svg
          className="h-6 w-6 text-red-600"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M6 18L18 6M6 6l12 12"
          />
        </svg>
      </div>
      <h2 className="text-lg font-medium text-gray-900">Export failed</h2>
      {failureReason && (
        <p className="text-sm text-gray-500">{failureReason}</p>
      )}
      <button
        type="button"
        className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
        onClick={onRetry}
        aria-label="Try again"
      >
        Try Again
      </button>
    </div>
  );
}
