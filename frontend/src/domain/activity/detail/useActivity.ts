import { useQuery } from "@tanstack/react-query";
import { getActivity, type ActivityDetail } from "@/api/client";

export function useActivity(activityId: string) {
  return useQuery<ActivityDetail>({
    queryKey: ["activity", activityId],
    queryFn: () => getActivity(activityId),
    staleTime: 60_000,
  });
}
