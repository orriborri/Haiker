import { useQuery } from "@tanstack/react-query";
import { getRecordedRoute, type RecordedRoute } from "@/api/client";

export function useRecordedRoute(
  activityId: string,
  detail: "full" | "preview" = "full",
) {
  return useQuery<RecordedRoute>({
    queryKey: ["recordedRoute", activityId, detail],
    queryFn: () => getRecordedRoute(activityId, detail),
    staleTime: 300_000,
  });
}
