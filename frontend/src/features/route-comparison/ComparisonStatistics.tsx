import type {
  RecordedRouteComparisonStatistics,
  CorrectedRouteComparisonStatistics,
} from "@/api/client";

function formatDistanceKm(meters: number): string {
  return `${(meters / 1000).toFixed(1)} km`;
}

interface ComparisonStatisticsProps {
  recorded: RecordedRouteComparisonStatistics;
  corrected: CorrectedRouteComparisonStatistics;
  correctedVersionNumber: number;
}

export function ComparisonStatistics({
  recorded,
  corrected,
  correctedVersionNumber,
}: ComparisonStatisticsProps) {
  return (
    <section aria-label="Route comparison statistics" className="mt-4">
      <h2 className="mb-3 text-lg font-semibold text-gray-900">Statistics</h2>
      <div className="grid grid-cols-2 gap-4">
        {/* Recorded column */}
        <div className="rounded-lg border border-blue-200 bg-blue-50 p-3">
          <h3 className="mb-2 text-sm font-semibold text-blue-900">
            Recorded
          </h3>
          <dl className="space-y-1 text-xs text-blue-800">
            <div className="flex justify-between">
              <dt>Distance</dt>
              <dd>{formatDistanceKm(recorded.distanceMeters)}</dd>
            </div>
            <div className="flex justify-between">
              <dt>Points</dt>
              <dd>{recorded.pointCount}</dd>
            </div>
            <div className="flex justify-between">
              <dt>Segments</dt>
              <dd>{recorded.segmentCount}</dd>
            </div>
            {recorded.elevationGainMeters != null && (
              <div className="flex justify-between">
                <dt>Elevation gain</dt>
                <dd>{Math.round(recorded.elevationGainMeters)} m</dd>
              </div>
            )}
            {recorded.elevationLossMeters != null && (
              <div className="flex justify-between">
                <dt>Elevation loss</dt>
                <dd>{Math.round(recorded.elevationLossMeters)} m</dd>
              </div>
            )}
          </dl>
        </div>

        {/* Corrected column */}
        <div className="rounded-lg border border-orange-200 bg-orange-50 p-3">
          <h3 className="mb-2 text-sm font-semibold text-orange-900">
            Corrected v{correctedVersionNumber}
          </h3>
          <dl className="space-y-1 text-xs text-orange-800">
            <div className="flex justify-between">
              <dt>Distance</dt>
              <dd>{formatDistanceKm(corrected.distanceMeters)}</dd>
            </div>
            <div className="flex justify-between">
              <dt>Points</dt>
              <dd>{corrected.pointCount}</dd>
            </div>
          </dl>
        </div>
      </div>
    </section>
  );
}
