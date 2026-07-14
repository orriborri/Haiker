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
    public problemType: string | null = null,
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
    const parsed = body as Record<string, unknown>;
    throw new ApiError(
      response.status,
      (parsed.code as string) ?? "unknown",
      (parsed.detail as string) ?? response.statusText,
      (parsed.type as string) ?? null,
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
  properties: z.record(z.string(), z.unknown()).nullable(),
});

const RecordedRouteSchema = z.object({
  type: z.literal("FeatureCollection"),
  bbox: z.array(z.number()),
  features: z.array(GeoJsonFeatureSchema),
  properties: z.record(z.string(), z.unknown()).nullable(),
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

const RoutePointDtoSchema = z.object({
  latitude: z.number(),
  longitude: z.number(),
  elevation: z.number().optional(),
});

const RouteDraftResponseSchema = z.object({
  id: z.string(),
  activityId: z.string(),
  revision: z.number(),
  state: z.string(),
  geometry: z.array(z.array(RoutePointDtoSchema)),
  canUndo: z.boolean(),
  canRedo: z.boolean(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

const OperationResultResponseSchema = z.object({
  draftId: z.string(),
  revision: z.number(),
  canUndo: z.boolean(),
  canRedo: z.boolean(),
});

export type RoutePointDto = z.infer<typeof RoutePointDtoSchema>;
export type RouteDraftResponse = z.infer<typeof RouteDraftResponseSchema>;
export type OperationResultResponse = z.infer<typeof OperationResultResponseSchema>;

/** Geometry payload for create/reset: array of segments, each segment is array of {latitude, longitude, elevation?} */
export type RouteGeometryPayload = RoutePointDto[][];

// Route Editing API Functions

export function createRouteDraft(
  activityId: string,
  geometry: RouteGeometryPayload,
): Promise<RouteDraftResponse> {
  return apiFetch(`/activities/${activityId}/route-drafts`, RouteDraftResponseSchema, {
    method: "POST",
    headers: {
      "Idempotency-Key": crypto.randomUUID(),
    },
    body: JSON.stringify({ geometry }),
  });
}

export function getRouteDraft(draftId: string, options?: RequestInit & { bypassServiceWorker?: boolean }): Promise<RouteDraftResponse> {
  const { bypassServiceWorker, ...fetchOptions } = options ?? {};
  const query = bypassServiceWorker ? "?_sw-bypass=1" : "";
  return apiFetch(`/route-drafts/${draftId}${query}`, RouteDraftResponseSchema, fetchOptions);
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
  geometry: RouteGeometryPayload,
): Promise<OperationResultResponse> {
  return apiFetch(
    `/route-drafts/${draftId}/reset`,
    OperationResultResponseSchema,
    {
      method: "POST",
      headers: {
        "Idempotency-Key": crypto.randomUUID(),
      },
      body: JSON.stringify({ expectedRevision, geometry }),
    },
  );
}

export function discardDraft(draftId: string): Promise<undefined> {
  return apiFetch(`/route-drafts/${draftId}`, VoidSchema, {
    method: "DELETE",
  });
}

// Import Schemas

const StartImportResponseSchema = z.object({
  importId: z.string(),
  uploadUrl: z.string(),
  status: z.literal("uploading"),
});

const ImportStatusResponseSchema = z.object({
  id: z.string(),
  status: z.enum([
    "requested",
    "uploading",
    "uploaded",
    "validating",
    "queued",
    "parsing",
    "committing",
    "completed",
    "failed",
    "cancelled",
  ]),
  failureReason: z.string().nullable(),
  activityId: z.string().nullable(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

// Import Types

export type StartImportRequest = {
  filename: string;
  contentType: "application/gpx+xml" | "application/xml";
  fileSizeBytes: number;
};

export type StartImportResponse = z.infer<typeof StartImportResponseSchema>;
export type ImportStatusResponse = z.infer<typeof ImportStatusResponseSchema>;

// Import API Functions

export function startImport(
  request: StartImportRequest,
  idempotencyKey: string,
): Promise<StartImportResponse> {
  return apiFetch("/imports", StartImportResponseSchema, {
    method: "POST",
    headers: {
      "Idempotency-Key": idempotencyKey,
    },
    body: JSON.stringify(request),
  });
}

export function completeUpload(
  importId: string,
  checksum: string,
): Promise<ImportStatusResponse> {
  return apiFetch(
    `/imports/${importId}/completion`,
    ImportStatusResponseSchema,
    {
      method: "POST",
      body: JSON.stringify({ checksum }),
    },
  );
}

export function getImportStatus(
  importId: string,
): Promise<ImportStatusResponse> {
  return apiFetch(`/imports/${importId}`, ImportStatusResponseSchema);
}

export { ApiError };
