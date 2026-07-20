import { useState, useCallback } from "react";
import { useRouteVersions, useRouteVersionGeometry } from "./useRouteVersions";
import { LoadingSpinner } from "@/components/LoadingSpinner";
import type { RouteVersionSummary } from "@/api/client";

function formatDateTime(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatDistanceKm(meters: number): string {
  return `${(meters / 1000).toFixed(1)} km`;
}

function versionLabel(version: RouteVersionSummary): string {
  if (version.versionNumber === 1) {
    return "Original";
  }
  return `Corrected v${version.versionNumber}`;
}

interface RouteHistoryProps {
  activityId: string;
  currentRouteVersionId?: string | null;
}

export function RouteHistory({
  activityId,
  currentRouteVersionId,
}: RouteHistoryProps) {
  const [selectedVersionId, setSelectedVersionId] = useState<string | null>(
    null,
  );

  const {
    data: versionsData,
    isLoading,
    isError,
    error,
    refetch,
  } = useRouteVersions(activityId);

  const {
    data: geometryData,
    isLoading: geometryLoading,
  } = useRouteVersionGeometry(selectedVersionId);

  const handleVersionSelect = useCallback((versionId: string) => {
    setSelectedVersionId((prev) => (prev === versionId ? null : versionId));
  }, []);

  if (isLoading) {
    return (
      <section aria-label="Route version history">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">
          Version History
        </h2>
        <LoadingSpinner className="py-8" />
      </section>
    );
  }

  if (isError) {
    return (
      <section aria-label="Route version history">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">
          Version History
        </h2>
        <div className="flex flex-col items-center justify-center rounded-lg border border-gray-200 bg-gray-50 py-8 text-center">
          <p className="text-sm text-gray-500">
            {error instanceof Error
              ? error.message
              : "Failed to load version history"}
          </p>
          <button
            type="button"
            className="mt-3 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
            onClick={() => void refetch()}
          >
            Retry
          </button>
        </div>
      </section>
    );
  }

  const versions = versionsData?.items ?? [];

  if (versions.length === 0) {
    return (
      <section aria-label="Route version history">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">
          Version History
        </h2>
        <p className="text-sm text-gray-500">No published versions yet.</p>
      </section>
    );
  }

  return (
    <section aria-label="Route version history">
      <h2 className="mb-3 text-lg font-semibold text-gray-900">
        Version History
      </h2>
      <p className="mb-4 text-xs text-gray-500">
        {versions.length} version{versions.length !== 1 ? "s" : ""} published
      </p>

      <ol className="space-y-2" aria-label="Route versions">
        {versions.map((version) => (
          <VersionItem
            key={version.id}
            version={version}
            isCurrent={version.id === currentRouteVersionId}
            isSelected={version.id === selectedVersionId}
            onSelect={handleVersionSelect}
          />
        ))}
      </ol>

      {/* Selected version geometry preview */}
      {selectedVersionId && (
        <div className="mt-4 rounded-lg border border-blue-200 bg-blue-50 p-3">
          <h3 className="text-sm font-medium text-blue-900">
            Geometry Preview
          </h3>
          {geometryLoading ? (
            <LoadingSpinner className="py-4" />
          ) : geometryData ? (
            <dl className="mt-2 grid grid-cols-2 gap-2 text-xs text-blue-800">
              <div>
                <dt className="font-medium">Points</dt>
                <dd>{geometryData.features[0]?.properties.pointCount ?? 0}</dd>
              </div>
              <div>
                <dt className="font-medium">Distance</dt>
                <dd>
                  {geometryData.features[0]?.properties.distanceMeters != null
                    ? formatDistanceKm(
                        geometryData.features[0].properties.distanceMeters,
                      )
                    : "N/A"}
                </dd>
              </div>
              <div>
                <dt className="font-medium">Bounding Box</dt>
                <dd>
                  [{geometryData.bbox[0]?.toFixed(3)},{" "}
                  {geometryData.bbox[1]?.toFixed(3)}] to [
                  {geometryData.bbox[2]?.toFixed(3)},{" "}
                  {geometryData.bbox[3]?.toFixed(3)}]
                </dd>
              </div>
            </dl>
          ) : null}
        </div>
      )}
    </section>
  );
}

interface VersionItemProps {
  version: RouteVersionSummary;
  isCurrent: boolean;
  isSelected: boolean;
  onSelect: (versionId: string) => void;
}

function VersionItem({
  version,
  isCurrent,
  isSelected,
  onSelect,
}: VersionItemProps) {
  return (
    <li>
      <button
        type="button"
        className={`w-full rounded-lg border p-3 text-left transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1 ${
          isSelected
            ? "border-blue-400 bg-blue-50"
            : "border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50"
        }`}
        onClick={() => onSelect(version.id)}
        aria-pressed={isSelected}
        aria-label={`${versionLabel(version)}${isCurrent ? " (current)" : ""}`}
      >
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-gray-900">
              {versionLabel(version)}
            </span>
            {isCurrent && (
              <span className="inline-flex items-center rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-800">
                Current
              </span>
            )}
          </div>
          <time
            className="text-xs text-gray-500"
            dateTime={version.createdAt}
          >
            {formatDateTime(version.createdAt)}
          </time>
        </div>

        {version.editSummary && (
          <p className="mt-1 text-xs text-gray-600">{version.editSummary}</p>
        )}

        <div className="mt-2 flex gap-4 text-xs text-gray-500">
          <span>
            {formatDistanceKm(version.correctedStatistics.distanceMeters)}
          </span>
          <span>{version.correctedStatistics.pointCount} points</span>
          {version.parentVersionId && (
            <span className="text-gray-400">
              from v
              {version.versionNumber - 1}
            </span>
          )}
        </div>
      </button>
    </li>
  );
}
