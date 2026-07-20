interface ComparisonLegendProps {
  correctedVersionNumber: number;
}

export function ComparisonLegend({
  correctedVersionNumber,
}: ComparisonLegendProps) {
  return (
    <div
      className="rounded-lg border border-gray-200 bg-white p-3"
      role="region"
      aria-label="Map legend"
    >
      <p className="mb-2 text-sm font-semibold text-gray-900">Legend</p>
      <ul aria-label="Route line styles" className="space-y-2">
        <li className="flex items-center gap-2">
          <svg
            className="h-3 w-8 flex-shrink-0"
            aria-hidden="true"
            role="img"
          >
            <line
              x1="0"
              y1="6"
              x2="32"
              y2="6"
              stroke="#3b82f6"
              strokeWidth="3"
            />
          </svg>
          <span className="text-xs text-gray-700">
            Recorded (solid blue line)
          </span>
        </li>
        <li className="flex items-center gap-2">
          <svg
            className="h-3 w-8 flex-shrink-0"
            aria-hidden="true"
            role="img"
          >
            <line
              x1="0"
              y1="6"
              x2="32"
              y2="6"
              stroke="#f97316"
              strokeWidth="3"
              strokeDasharray="6 3"
            />
          </svg>
          <span className="text-xs text-gray-700">
            Corrected v{correctedVersionNumber} (dashed orange line)
          </span>
        </li>
      </ul>
    </div>
  );
}
