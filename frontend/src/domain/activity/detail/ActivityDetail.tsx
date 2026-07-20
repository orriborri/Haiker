import { useCallback, useEffect, useRef } from "react";
import { useNavigate, Link } from "@tanstack/react-router";
import { useActivity } from "./useActivity";
import { useRecordedRoute } from "./useRecordedRoute";
import { RouteMap } from "./RouteMap";
import { LoadingSpinner } from "@/common/components/LoadingSpinner";
import { useDocumentTitle } from "@/common/hooks/useDocumentTitle";
import { formatDateTime, formatDistanceKm } from "@/common/utils";
import type { ActivityDetail } from "@/api/client";

interface ActivityDetailPageProps {
  activityId: string;
}

export function ActivityDetailPage({ activityId }: ActivityDetailPageProps) {
  const navigate = useNavigate();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const {
    data: activity,
    isLoading: activityLoading,
    isError: activityError,
    error: activityErr,
    refetch: refetchActivity,
  } = useActivity(activityId);
  const {
    data: route,
    isLoading: routeLoading,
    isError: routeError,
  } = useRecordedRoute(activityId);

  useDocumentTitle(activity?.title ?? "Activity Detail");

  useEffect(() => {
    if (activity && headingRef.current) {
      headingRef.current.focus();
    }
  }, [activity]);

  const handleBack = useCallback(() => {
    void navigate({ to: "/" });
  }, [navigate]);

  if (activityLoading) {
    return (
      <div className="mx-auto max-w-2xl px-4 py-8">
        <LoadingSpinner className="py-16" />
      </div>
    );
  }

  if (activityError) {
    return (
      <div className="mx-auto max-w-2xl px-4 py-8">
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <h1 className="text-lg font-medium text-gray-900">
            Failed to load activity
          </h1>
          <p className="mt-1 text-sm text-gray-500">
            {activityErr instanceof Error
              ? activityErr.message
              : "An unexpected error occurred"}
          </p>
          <div className="mt-4 flex gap-3">
            <button
              type="button"
              className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
              onClick={handleBack}
            >
              Back to list
            </button>
            <button
              type="button"
              className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
              onClick={() => void refetchActivity()}
            >
              Retry
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (!activity) return null;

  return (
    <div className="mx-auto max-w-2xl px-4 py-4">
      {/* Back navigation */}
      <button
        type="button"
        className="mb-4 flex items-center gap-1 text-sm text-gray-600 hover:text-gray-900 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 rounded"
        onClick={handleBack}
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
        Back to activities
      </button>

      {/* Activity header */}
      <header className="mb-6">
        <h1
          ref={headingRef}
          className="text-2xl font-bold text-gray-900 focus:outline-none"
          tabIndex={-1}
        >
          {activity.title}
        </h1>
        <p className="mt-1 text-sm capitalize text-gray-500">
          {activity.activityType}
        </p>
      </header>

      {/* Route map */}
      <section className="mb-6" aria-label="Route map">
        {routeLoading && <LoadingSpinner className="h-64 sm:h-96" />}
        {routeError && (
          <div className="flex h-64 items-center justify-center rounded-lg border border-gray-200 bg-gray-50 sm:h-96">
            <p className="text-sm text-gray-500">Unable to load route map</p>
          </div>
        )}
        {route && <RouteMap route={route} />}
        {route && (
          <div className="mt-3">
            <Link
              to="/activities/$activityId/edit"
              params={{ activityId }}
              className="inline-flex items-center rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
            >
              Edit Route
            </Link>
          </div>
        )}
      </section>

      {/* Activity metadata */}
      <section aria-label="Activity details">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Details</h2>
        <dl className="grid grid-cols-2 gap-4 sm:grid-cols-3">
          {activity.startedAt && (
            <MetadataItem label="Started" value={formatDateTime(activity.startedAt)} />
          )}
          {activity.endedAt && (
            <MetadataItem
              label="Ended"
              value={formatDateTime(activity.endedAt)}
            />
          )}
          <MetadataItem label="State" value={activity.lifecycleState} />
          <MetadataItem label="Created" value={formatDateTime(activity.createdAt)} />
          <MetadataItem label="Updated" value={formatDateTime(activity.updatedAt)} />
        </dl>
      </section>

      {/* Statistics */}
      <StatisticsSection activity={activity} />

      {/* Route version history */}
      <div className="mt-6">
        <RouteHistory
          activityId={activityId}
          currentRouteVersionId={activity.currentRouteVersionId}
        />
      </div>
    </div>
  );
}

function MetadataItem({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-xs font-medium text-gray-500">{label}</dt>
      <dd className="mt-0.5 text-sm text-gray-900">{value}</dd>
    </div>
  );
}

function StatisticsSection({ activity }: { activity: ActivityDetail }) {
  const recorded = activity.recordedSummary;
  const corrected = activity.correctedSummary;

  if (!recorded && !corrected) return null;

  return (
    <section aria-label="Statistics" className="mt-6">
      <h2 className="mb-3 text-lg font-semibold text-gray-900">Statistics</h2>
      <dl className="grid grid-cols-2 gap-4 sm:grid-cols-3">
        {recorded?.distance_meters != null && (
          <MetadataItem
            label="Recorded distance"
            value={formatDistanceKm(recorded.distance_meters)}
          />
        )}
        {corrected?.distance_meters != null && (
          <MetadataItem
            label="Corrected distance"
            value={formatDistanceKm(corrected.distance_meters)}
          />
        )}
        {recorded?.point_count != null && (
          <MetadataItem
            label="Recorded points"
            value={String(recorded.point_count)}
          />
        )}
        {corrected?.point_count != null && (
          <MetadataItem
            label="Corrected points"
            value={String(corrected.point_count)}
          />
        )}
        {recorded?.elevation_gain_meters != null && (
          <MetadataItem
            label="Elevation gain"
            value={`${Math.round(recorded.elevation_gain_meters)} m`}
          />
        )}
        {recorded?.elevation_loss_meters != null && (
          <MetadataItem
            label="Elevation loss"
            value={`${Math.round(recorded.elevation_loss_meters)} m`}
          />
        )}
      </dl>
    </section>
  );
}
