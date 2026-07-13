import { useInfiniteQuery } from "@tanstack/react-query";
import { listActivities, type ActivitiesPage } from "@/api/client";

const PAGE_SIZE = 20;

export function useActivities() {
  return useInfiniteQuery<ActivitiesPage>({
    queryKey: ["activities"],
    queryFn: ({ pageParam }) =>
      listActivities(pageParam as string | undefined, PAGE_SIZE),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) => lastPage.cursor ?? undefined,
    staleTime: 30_000,
  });
}
