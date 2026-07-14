import { useCallback, useEffect, useRef } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useActivities } from "./useActivities";
import { LoadingSpinner } from "@/components/LoadingSpinner";
import { EmptyState } from "@/components/EmptyState";
import type { ActivitySummary } from "@/api/client";

function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

function ActivityRow({ activity }: { activity: ActivitySummary }) {
  const navigate = useNavigate();

  const handleSelect = useCallback(() => {
    void navigate({
      to: "/activities/$activityId",
      params: { activityId: activity.id },
    });
  }, [navigate, activity.id]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        handleSelect();
      }
    },
    [handleSelect],
  );

  return (
    <li
      className="flex cursor-pointer items-center gap-4 border-b border-gray-100 px-4 py-3 transition-colors hover:bg-gray-50 focus:bg-blue-50 focus:outline-none focus:ring-2 focus:ring-inset focus:ring-blue-500"
      role="button"
      tabIndex={0}
      onClick={handleSelect}
      onKeyDown={handleKeyDown}
      aria-label={`View activity: ${activity.title}`}
    >
      <div className="min-w-0 flex-1">
        <h3 className="truncate text-sm font-medium text-gray-900">
          {activity.title}
        </h3>
        <p className="mt-0.5 text-xs text-gray-500">
          <span className="capitalize">{activity.activityType}</span>
          {activity.startedAt && (
            <>
              {" \u00B7 "}
              {formatDate(activity.startedAt)}
            </>
          )}
        </p>
      </div>
    </li>
  );
}

function LoadingSkeleton() {
  return (
    <div className="animate-pulse" role="status" aria-label="Loading activities">
      {Array.from({ length: 6 }).map((_, i) => (
        <div
          key={i}
          className="flex items-center gap-4 border-b border-gray-100 px-4 py-3"
        >
          <div className="min-w-0 flex-1">
            <div className="h-4 w-3/4 rounded bg-gray-200" />
            <div className="mt-1.5 h-3 w-1/2 rounded bg-gray-100" />
          </div>
          <div className="flex flex-col items-end gap-1">
            <div className="h-3 w-12 rounded bg-gray-100" />
            <div className="h-3 w-10 rounded bg-gray-100" />
          </div>
        </div>
      ))}
    </div>
  );
}

export function ActivityLibrary() {
  const {
    data,
    isLoading,
    isError,
    error,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    refetch,
  } = useActivities();

  const navigate = useNavigate();
  const loadMoreRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!hasNextPage || isFetchingNextPage) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0];
        if (entry?.isIntersecting) {
          void fetchNextPage();
        }
      },
      { threshold: 0.1 },
    );

    const el = loadMoreRef.current;
    if (el) observer.observe(el);

    return () => {
      if (el) observer.unobserve(el);
    };
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  if (isLoading) {
    return (
      <div className="mx-auto max-w-2xl">
        <header className="border-b border-gray-200 px-4 py-4">
          <h1 className="text-xl font-semibold text-gray-900">Activities</h1>
        </header>
        <LoadingSkeleton />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="mx-auto max-w-2xl">
        <header className="border-b border-gray-200 px-4 py-4">
          <h1 className="text-xl font-semibold text-gray-900">Activities</h1>
        </header>
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <h2 className="text-lg font-medium text-gray-900">
            Failed to load activities
          </h2>
          <p className="mt-1 text-sm text-gray-500">
            {error instanceof Error ? error.message : "An unexpected error occurred"}
          </p>
          <button
            type="button"
            className="mt-4 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
            onClick={() => void refetch()}
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  const allActivities = data?.pages.flatMap((page) => page.items) ?? [];

  const handleNavigateToImport = useCallback(() => {
    void navigate({ to: "/import" });
  }, [navigate]);

  return (
    <div className="mx-auto max-w-2xl">
      <header className="flex items-center justify-between border-b border-gray-200 px-4 py-4">
        <h1 className="text-xl font-semibold text-gray-900">Activities</h1>
        <button
          type="button"
          className="rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
          onClick={handleNavigateToImport}
        >
          Import GPX
        </button>
      </header>

      {allActivities.length === 0 ? (
        <EmptyState
          title="No activities yet"
          description="Your activities will appear here after you import them."
        />
      ) : (
        <>
          <ul className="divide-y-0" role="list">
            {allActivities.map((activity) => (
              <ActivityRow key={activity.id} activity={activity} />
            ))}
          </ul>

          <div ref={loadMoreRef} className="px-4 py-4">
            {isFetchingNextPage && <LoadingSpinner className="py-4" />}
          </div>
        </>
      )}
    </div>
  );
}
