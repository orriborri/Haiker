import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  selectCurrentRouteVersion,
  type ActivityDetail,
} from "@/api/client";
import { useCallback, useState } from "react";

interface SelectCurrentRouteVersionParams {
  activityId: string;
  routeVersionId: string;
}

export function useSelectCurrentRouteVersion(activityId: string) {
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);

  const mutation = useMutation<ActivityDetail, Error, SelectCurrentRouteVersionParams>({
    mutationFn: ({ activityId, routeVersionId }) =>
      selectCurrentRouteVersion(activityId, routeVersionId),
    onSuccess: () => {
      setError(null);
      // Invalidate activity detail cache so UI reflects new currentRouteVersionId
      void queryClient.invalidateQueries({
        queryKey: ["activity", activityId],
      });
      // Invalidate route versions list to refresh any stale data
      void queryClient.invalidateQueries({
        queryKey: ["routeVersions", activityId],
      });
    },
    onError: (err) => {
      setError(err.message || "Failed to set current version");
    },
  });

  const clearError = useCallback(() => setError(null), []);

  return { ...mutation, error, clearError };
}
