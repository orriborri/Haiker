import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  createRouteDraft,
  getRouteDraft,
  applyOperation,
  undoOperation,
  redoOperation,
  resetDraft,
  discardDraft,
  type RouteDraftResponse,
  type RouteGeometryPayload,
  type OperationResultResponse,
  ApiError,
} from "@/api/client";

export function useGetRouteDraft(draftId: string | null) {
  return useQuery<RouteDraftResponse>({
    queryKey: ["routeDraft", draftId],
    queryFn: () => getRouteDraft(draftId!),
    enabled: !!draftId,
    staleTime: 30_000,
  });
}

export function useCreateRouteDraft() {
  const queryClient = useQueryClient();
  return useMutation<
    RouteDraftResponse,
    Error,
    { activityId: string; geometry: RouteGeometryPayload }
  >({
    mutationFn: ({ activityId, geometry }) =>
      createRouteDraft(activityId, geometry),
    onSuccess: (data) => {
      queryClient.setQueryData(["routeDraft", data.id], data);
    },
  });
}

export function useApplyOperation(
  onConflict?: (error: ApiError) => void,
) {
  const queryClient = useQueryClient();
  return useMutation<
    OperationResultResponse,
    Error,
    {
      draftId: string;
      operation: Record<string, unknown>;
      expectedRevision: number;
    }
  >({
    mutationFn: ({ draftId, operation, expectedRevision }) =>
      applyOperation(draftId, operation, expectedRevision),
    onSuccess: (_data, variables) => {
      void queryClient.invalidateQueries({
        queryKey: ["routeDraft", variables.draftId],
      });
    },
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409 && onConflict) {
        onConflict(error);
      }
    },
  });
}

export function useUndoOperation(
  onConflict?: (error: ApiError) => void,
) {
  const queryClient = useQueryClient();
  return useMutation<
    OperationResultResponse,
    Error,
    { draftId: string; expectedRevision: number }
  >({
    mutationFn: ({ draftId, expectedRevision }) =>
      undoOperation(draftId, expectedRevision),
    onSuccess: (_data, variables) => {
      void queryClient.invalidateQueries({
        queryKey: ["routeDraft", variables.draftId],
      });
    },
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409 && onConflict) {
        onConflict(error);
      }
    },
  });
}

export function useRedoOperation(
  onConflict?: (error: ApiError) => void,
) {
  const queryClient = useQueryClient();
  return useMutation<
    OperationResultResponse,
    Error,
    { draftId: string; expectedRevision: number }
  >({
    mutationFn: ({ draftId, expectedRevision }) =>
      redoOperation(draftId, expectedRevision),
    onSuccess: (_data, variables) => {
      void queryClient.invalidateQueries({
        queryKey: ["routeDraft", variables.draftId],
      });
    },
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409 && onConflict) {
        onConflict(error);
      }
    },
  });
}

export function useResetDraft(
  onConflict?: (error: ApiError) => void,
) {
  const queryClient = useQueryClient();
  return useMutation<
    OperationResultResponse,
    Error,
    { draftId: string; expectedRevision: number }
  >({
    mutationFn: ({ draftId, expectedRevision }) =>
      resetDraft(draftId, expectedRevision),
    onSuccess: (_data, variables) => {
      void queryClient.invalidateQueries({
        queryKey: ["routeDraft", variables.draftId],
      });
    },
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409 && onConflict) {
        onConflict(error);
      }
    },
  });
}

export function useDiscardDraft() {
  const queryClient = useQueryClient();
  return useMutation<undefined, Error, { draftId: string }>({
    mutationFn: ({ draftId }) => discardDraft(draftId),
    onSuccess: (_data, variables) => {
      void queryClient.invalidateQueries({
        queryKey: ["routeDraft", variables.draftId],
      });
    },
  });
}
