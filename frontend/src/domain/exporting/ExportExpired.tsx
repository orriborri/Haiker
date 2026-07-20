interface ExportExpiredProps {
  onRetry: () => void;
}

export function ExportExpired({ onRetry }: ExportExpiredProps) {
  return (
    <div className="flex flex-col items-center space-y-4 py-8 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-yellow-100">
        <svg
          className="h-6 w-6 text-yellow-600"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z"
          />
        </svg>
      </div>
      <h2 className="text-lg font-medium text-gray-900">Export expired</h2>
      <p className="text-sm text-gray-500">
        This export has expired and is no longer available for download. You can
        request a new export to generate a fresh file.
      </p>
      <button
        type="button"
        className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
        onClick={onRetry}
        aria-label="Request new export"
      >
        Request New Export
      </button>
    </div>
  );
}
