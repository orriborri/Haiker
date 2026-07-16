import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
} from "@tanstack/react-router";
import { z } from "zod";
import { ActivityLibrary } from "@/features/activity-library/ActivityLibrary";
import { ActivityDetailPage } from "@/features/activity-detail/ActivityDetail";
import { RouteEditor } from "@/features/route-editor/RouteEditor";
import { ImportActivity } from "@/features/import-activity";
import { ExportRoute } from "@/features/export-route";
import { ErrorBoundary } from "@/components/ErrorBoundary";

const rootRoute = createRootRoute({
  component: () => (
    <ErrorBoundary>
      <main className="min-h-screen bg-white">
        <Outlet />
      </main>
    </ErrorBoundary>
  ),
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: ActivityLibrary,
});

const activityDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activities/$activityId",
  component: function ActivityDetailWrapper() {
    const { activityId } = activityDetailRoute.useParams();
    return <ActivityDetailPage activityId={activityId} />;
  },
});

const activityEditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activities/$activityId/edit",
  component: function ActivityEditWrapper() {
    const { activityId } = activityEditRoute.useParams();
    return <RouteEditor activityId={activityId} />;
  },
});

const importSearchSchema = z.object({
  importId: z.string().optional(),
});

const importRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/import",
  validateSearch: importSearchSchema,
  component: ImportActivity,
});

const exportSearchSchema = z.object({
  exportId: z.string().optional(),
  routeVersionId: z.string().optional(),
});

const exportRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activities/$activityId/export",
  validateSearch: exportSearchSchema,
  component: function ExportRouteWrapper() {
    const { activityId } = exportRoute.useParams();
    return <ExportRoute activityId={activityId} />;
  },
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  activityDetailRoute,
  activityEditRoute,
  importRoute,
  exportRoute,
]);

export const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
