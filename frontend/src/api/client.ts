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
  startedAt: z.string(),
  finishedAt: z.string().nullable(),
  distanceMeters: z.number().nullable(),
  durationSeconds: z.number().nullable(),
  elevationGainMeters: z.number().nullable(),
});

const ActivitiesPageSchema = z.object({
  items: z.array(ActivitySummarySchema),
  cursor: z.string().nullable(),
});

const ActivityDetailSchema = z.object({
  id: z.string(),
  title: z.string(),
  activityType: z.string(),
  startedAt: z.string(),
  finishedAt: z.string().nullable(),
  distanceMeters: z.number().nullable(),
  durationSeconds: z.number().nullable(),
  elevationGainMeters: z.number().nullable(),
  elevationLossMeters: z.number().nullable(),
  averageSpeedMps: z.number().nullable(),
  maxSpeedMps: z.number().nullable(),
  averageHeartRate: z.number().nullable(),
  maxHeartRate: z.number().nullable(),
  createdAt: z.string(),
});

const RecordedRouteSchema = z.object({
  type: z.literal("Feature"),
  geometry: z.object({
    type: z.string(),
    coordinates: z.array(z.unknown()),
  }),
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

export { ApiError };
