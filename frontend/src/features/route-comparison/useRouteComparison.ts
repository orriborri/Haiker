import { useQuery } from "@tanstack/react-query";
import {
  getRouteComparison,
  type RouteComparisonResponse,
} from "@/api/client";

export function useRouteComparison(
  activityId: string,
  routeVersionId: string | null,
) {
  return useQuery<RouteComparisonResponse>({
    queryKey: ["routeComparison", activityId, routeVersionId],
    queryFn: () => getRouteComparison(activityId, routeVersionId!),
    enabled: !!routeVersionId,
    // staleTime: Infinity is safe here because:
    // - Published route versions are immutable by domain invariant (enforced by DB triggers).
    // - The recorded route is effectively immutable: re-importing creates a new activity rather
    //   than mutating the existing one, so a given (activityId, routeVersionId) pair always
    //   resolves to the same geometry. If this assumption changes, switch to a finite staleTime.
    staleTime: Infinity,
  });
}
