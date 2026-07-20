import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { useRouteVersions } from "@/features/route-history";
import { LoadingSpinner } from "@/components/LoadingSpinner";
import { useRouteComparison } from "./useRouteComparison";
import { RouteComparisonMap } from "./RouteComparisonMap";
import { ComparisonStatistics } from "./ComparisonStatistics";
import { ComparisonLegend } from "./ComparisonLegend";
import { VersionSelector } from "./VersionSelector";

interface RouteComparisonProps {
  activityId: string;
}

export function RouteComparison({ activityId }: RouteComparisonProps) {
  const [selectedVersionId, setSelectedVersionId] = useState<string | null>(
    null,
  );

  const {
    data: versionsData,
    isLoading: versionsLoading,
    isError: versionsError,
    error: versionsErr,
  } = useRouteVersions(activityId);

  // Select the latest version by default once versions are loaded
  const versions = versionsData?.items ?? [];
  const effectiveVersionId =
    selectedVersionId ?? (versions.length > 0 ? versions[0]!.id : null);

  const {
    data: comparison,
    isLoading: comparisonLoading,
    isError: comparisonError,
    error: comparisonErr,
  } = useRouteComparison(activityId, effectiveVersionId);

  if (versionsLoading) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-8">
        <LoadingSpinner className="py-16" />
      </div>
    );
  }

  if (versionsError) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-8">
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <h1 className="text-lg font-medium text-gray-900">
            Failed to load route versions
          </h1>
          <p className="mt-1 text-sm text-gray-500">
            {versionsErr instanceof Error
              ? versionsErr.message
              : "An unexpected error occurred"}
          </p>
          <Link
            to="/activities/$activityId"
            params={{ activityId }}
            className="mt-4 rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
          >
            Back to activity
          </Link>
        </div>
      </div>
    );
  }

  if (versions.length === 0) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-8">
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <h1 className="text-lg font-medium text-gray-900">
            No corrected versions
          </h1>
          <p className="mt-1 text-sm text-gray-500">
            There are no published route versions to compare against the
            recorded route.
          </p>
          <Link
            to="/activities/$activityId"
            params={{ activityId }}
            className="mt-4 rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
          >
            Back to activity
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl px-4 py-4">
      {/* Navigation */}
      <Link
        to="/activities/$activityId"
        params={{ activityId }}
        className="mb-4 inline-flex items-center gap-1 text-sm text-gray-600 hover:text-gray-900 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 rounded"
      >
        <svg
          className="h-4 w-4"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M15 19l-7-7 7-7"
          />
        </svg>
        Back to activity
      </Link>

      <header className="mb-4">
        <h1 className="text-2xl font-bold text-gray-900">Route Comparison</h1>
      </header>

      {/* Version selector */}
      {effectiveVersionId && (
        <div className="mb-4">
          <VersionSelector
            versions={versions}
            selectedVersionId={effectiveVersionId}
            onVersionChange={setSelectedVersionId}
          />
        </div>
      )}

      {/* Comparison content */}
      {comparisonLoading && <LoadingSpinner className="py-16" />}

      {comparisonError && (
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <p className="text-sm text-gray-500">
            {comparisonErr instanceof Error
              ? comparisonErr.message
              : "Failed to load comparison data"}
          </p>
        </div>
      )}

      {comparison && (
        <>
          <RouteComparisonMap comparison={comparison} />

          <div className="mt-4">
            <ComparisonLegend
              correctedVersionNumber={comparison.corrected.versionNumber}
            />
          </div>

          <ComparisonStatistics
            recorded={comparison.recorded.statistics}
            corrected={comparison.corrected.statistics}
            correctedVersionNumber={comparison.corrected.versionNumber}
          />
        </>
      )}
    </div>
  );
}
