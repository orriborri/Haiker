import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  selectCurrentRouteVersion,
  type ActivityDetail,
} from "@/api/client";

interface SelectCurrentRouteVersionParams {
  activityId: string;
  routeVersionId: string;
}

export function useSelectCurrentRouteVersion(activityId: string) {
  const queryClient = useQueryClient();

  return useMutation<ActivityDetail, Error, SelectCurrentRouteVersionParams>({
    mutationFn: ({ activityId, routeVersionId }) =>
      selectCurrentRouteVersion(activityId, routeVersionId),
    onSuccess: () => {
      // Invalidate activity detail cache so UI reflects new currentRouteVersionId
      void queryClient.invalidateQueries({
        queryKey: ["activity", activityId],
      });
      // Invalidate route versions list to refresh any stale data
      void queryClient.invalidateQueries({
        queryKey: ["routeVersions", activityId],
      });
    },
  });
}
