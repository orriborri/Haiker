import { useQuery } from "@tanstack/react-query";
import {
  listRouteVersions,
  getRouteVersion,
  getRouteVersionGeometry,
  type RouteVersionListResponse,
  type RouteVersionDetailResponse,
  type RouteVersionGeometryResponse,
} from "@/api/client";

export function useRouteVersions(activityId: string, pageSize?: number) {
  return useQuery<RouteVersionListResponse>({
    queryKey: ["routeVersions", activityId, pageSize],
    queryFn: () => listRouteVersions(activityId, null, pageSize),
    staleTime: 60_000,
  });
}

export function useRouteVersionDetail(routeVersionId: string | null) {
  return useQuery<RouteVersionDetailResponse>({
    queryKey: ["routeVersion", routeVersionId],
    queryFn: () => getRouteVersion(routeVersionId!),
    enabled: !!routeVersionId,
    staleTime: Infinity, // Immutable data never goes stale
  });
}

export function useRouteVersionGeometry(routeVersionId: string | null) {
  return useQuery<RouteVersionGeometryResponse>({
    queryKey: ["routeVersionGeometry", routeVersionId],
    queryFn: () => getRouteVersionGeometry(routeVersionId!),
    enabled: !!routeVersionId,
    staleTime: Infinity, // Immutable geometry never goes stale
  });
}
