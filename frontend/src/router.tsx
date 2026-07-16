import { useEffect, useRef, useState } from "react";
import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  useLocation,
} from "@tanstack/react-router";
import { z } from "zod";
import { ActivityLibrary } from "@/features/activity-library/ActivityLibrary";
import { ActivityDetailPage } from "@/features/activity-detail/ActivityDetail";
import { RouteEditor } from "@/features/route-editor/RouteEditor";
import { ImportActivity } from "@/features/import-activity";
import { ExportRoute } from "@/features/export-route";
import { ErrorBoundary } from "@/components/ErrorBoundary";

function getPageName(pathname: string): string {
  if (pathname === "/") return "Activities page";
  if (pathname === "/import") return "Import Activity page";
  if (/^\/activities\/[^/]+\/edit$/.test(pathname)) return "Route Editor page";
  if (/^\/activities\/[^/]+\/export$/.test(pathname)) return "Export Route page";
  if (/^\/activities\/[^/]+$/.test(pathname)) return "Activity Detail page";
  return "Page";
}

function RootLayout() {
  const location = useLocation();
  const [announcement, setAnnouncement] = useState("");
  const isFirstRender = useRef(true);

  useEffect(() => {
    if (isFirstRender.current) {
      isFirstRender.current = false;
      return;
    }
    setAnnouncement(getPageName(location.pathname));
  }, [location.pathname]);

  return (
    <ErrorBoundary>
      {/* Skip to content link */}
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-[100] focus:rounded-md focus:bg-blue-600 focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-white focus:shadow-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
      >
        Skip to content
      </a>
      {/* Route announcements for screen readers */}
      <div aria-live="assertive" aria-atomic="true" className="sr-only">
        {announcement}
      </div>
      <main id="main-content" className="min-h-screen bg-white">
        <Outlet />
      </main>
    </ErrorBoundary>
  );
}

const rootRoute = createRootRoute({
  component: RootLayout,
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
