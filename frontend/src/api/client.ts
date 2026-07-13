import { z } from "zod";

const API_BASE = "/v1";

function getAuthToken(): string | null {
  return localStorage.getItem("auth_token");
}

class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function apiFetch<T>(
  path: string,
  schema: z.ZodType<T>,
  options: RequestInit = {},
): Promise<T> {
  const token = getAuthToken();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...((options.headers as Record<string, string>) ?? {}),
  };

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const response = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new ApiError(
      response.status,
      (body as Record<string, unknown>).code as string ?? "unknown",
      (body as Record<string, unknown>).detail as string ?? response.statusText,
    );
  }

  if (response.status === 204) {
    return undefined as unknown as T;
  }

  const json: unknown = await response.json();
  return schema.parse(json);
}

// Schemas

const ActivitySummarySchema = z.object({
  id: z.string(),
  title: z.string(),
  activityType: z.string(),
  startedAt: z.string().nullable(),
  endedAt: z.string().nullable(),
  recordedSummary: z.unknown().nullable().optional(),
  correctedSummary: z.unknown().nullable().optional(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

const PaginationMetaSchema = z.object({
  cursor: z.string().nullable().optional(),
  hasMore: z.boolean(),
  pageSize: z.number(),
});

const ActivitiesPageSchema = z.object({
  items: z.array(ActivitySummarySchema),
  pagination: PaginationMetaSchema,
});

const ActivityDetailSchema = z.object({
  id: z.string(),
  title: z.string(),
  activityType: z.string(),
  startedAt: z.string().nullable(),
  endedAt: z.string().nullable(),
  lifecycleState: z.string(),
  recordedSummary: z.unknown().nullable().optional(),
  correctedSummary: z.unknown().nullable().optional(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

const GeoJsonGeometrySchema = z.object({
  type: z.string(),
  coordinates: z.array(z.array(z.number())),
});

const GeoJsonFeatureSchema = z.object({
  type: z.literal("Feature"),
  geometry: GeoJsonGeometrySchema,
  properties: z.record(z.unknown()).nullable(),
});

const RecordedRouteSchema = z.object({
  type: z.literal("FeatureCollection"),
  bbox: z.array(z.number()),
  features: z.array(GeoJsonFeatureSchema),
  properties: z.record(z.unknown()).nullable(),
});

const VoidSchema = z.undefined();

// Types

export type ActivitySummary = z.infer<typeof ActivitySummarySchema>;
export type ActivitiesPage = z.infer<typeof ActivitiesPageSchema>;
export type ActivityDetail = z.infer<typeof ActivityDetailSchema>;
export type RecordedRoute = z.infer<typeof RecordedRouteSchema>;

// API Functions

export function listActivities(
  cursor?: string | null,
  pageSize?: number,
): Promise<ActivitiesPage> {
  const params = new URLSearchParams();
  if (cursor) params.set("cursor", cursor);
  if (pageSize) params.set("pageSize", String(pageSize));
  const query = params.toString();
  return apiFetch(`/activities${query ? `?${query}` : ""}`, ActivitiesPageSchema);
}

export function getActivity(activityId: string): Promise<ActivityDetail> {
  return apiFetch(`/activities/${activityId}`, ActivityDetailSchema);
}

export function getRecordedRoute(
  activityId: string,
  detail?: "full" | "preview",
): Promise<RecordedRoute> {
  const params = new URLSearchParams();
  if (detail) params.set("detail", detail);
  const query = params.toString();
  return apiFetch(
    `/activities/${activityId}/recorded-route${query ? `?${query}` : ""}`,
    RecordedRouteSchema,
  );
}

export function updateActivityTitle(
  activityId: string,
  title: string,
): Promise<ActivityDetail> {
  return apiFetch(`/activities/${activityId}/title`, ActivityDetailSchema, {
    method: "PATCH",
    body: JSON.stringify({ title }),
  });
}

export function deleteActivity(activityId: string): Promise<undefined> {
  return apiFetch(`/activities/${activityId}`, VoidSchema, {
    method: "DELETE",
  });
}

// Route Editing Schemas

const RouteDraftGeometrySchema = z.object({
  type: z.literal("MultiLineString"),
  coordinates: z.array(z.array(z.array(z.number()))),
});

const RouteDraftResponseSchema = z.object({
  id: z.string(),
  activityId: z.string(),
  revision: z.number(),
  state: z.string(),
  geometry: RouteDraftGeometrySchema,
  createdAt: z.string(),
  updatedAt: z.string(),
});

const OperationResultResponseSchema = z.object({
  draftId: z.string(),
  revision: z.number(),
});

export type RouteDraftGeometry = z.infer<typeof RouteDraftGeometrySchema>;
export type RouteDraftResponse = z.infer<typeof RouteDraftResponseSchema>;
export type OperationResultResponse = z.infer<typeof OperationResultResponseSchema>;

// Route Editing API Functions

export function createRouteDraft(
  activityId: string,
  geometry: RouteDraftGeometry,
): Promise<RouteDraftResponse> {
  return apiFetch("/route-drafts", RouteDraftResponseSchema, {
    method: "POST",
    body: JSON.stringify({ activityId, geometry }),
  });
}

export function getRouteDraft(draftId: string): Promise<RouteDraftResponse> {
  return apiFetch(`/route-drafts/${draftId}`, RouteDraftResponseSchema);
}

export function applyOperation(
  draftId: string,
  operation: Record<string, unknown>,
  expectedRevision: number,
): Promise<OperationResultResponse> {
  return apiFetch(
    `/route-drafts/${draftId}/operations`,
    OperationResultResponseSchema,
    {
      method: "POST",
      headers: {
        "Idempotency-Key": crypto.randomUUID(),
      },
      body: JSON.stringify({ operation, expectedRevision }),
    },
  );
}

export function undoOperation(
  draftId: string,
  expectedRevision: number,
): Promise<OperationResultResponse> {
  return apiFetch(
    `/route-drafts/${draftId}/undo`,
    OperationResultResponseSchema,
    {
      method: "POST",
      headers: {
        "Idempotency-Key": crypto.randomUUID(),
      },
      body: JSON.stringify({ expectedRevision }),
    },
  );
}

export function redoOperation(
  draftId: string,
  expectedRevision: number,
): Promise<OperationResultResponse> {
  return apiFetch(
    `/route-drafts/${draftId}/redo`,
    OperationResultResponseSchema,
    {
      method: "POST",
      headers: {
        "Idempotency-Key": crypto.randomUUID(),
      },
      body: JSON.stringify({ expectedRevision }),
    },
  );
}

export function resetDraft(
  draftId: string,
  expectedRevision: number,
): Promise<OperationResultResponse> {
  return apiFetch(
    `/route-drafts/${draftId}/reset`,
    OperationResultResponseSchema,
    {
      method: "POST",
      headers: {
        "Idempotency-Key": crypto.randomUUID(),
      },
      body: JSON.stringify({ expectedRevision }),
    },
  );
}

export function discardDraft(draftId: string): Promise<undefined> {
  return apiFetch(`/route-drafts/${draftId}`, VoidSchema, {
    method: "DELETE",
  });
}

export { ApiError };
