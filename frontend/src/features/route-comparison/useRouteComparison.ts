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
    staleTime: Infinity, // Both recorded and published versions are immutable
  });
}
