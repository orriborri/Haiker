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
import { RouteComparison } from "@/features/route-comparison";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { AuthGuard, AuthCallback, LoginPage } from "@/auth";

function getPageName(pathname: string): string {
  if (pathname === "/") return "Activities page";
  if (pathname === "/login") return "Sign in page";
  if (pathname === "/auth/callback") return "Completing sign in";
  if (pathname === "/import") return "Import Activity page";
  if (/^\/activities\/[^/]+\/edit$/.test(pathname)) return "Route Editor page";
  if (/^\/activities\/[^/]+\/export$/.test(pathname)) return "Export Route page";
  if (/^\/activities\/[^/]+\/compare$/.test(pathname)) return "Route Comparison page";
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
      <div aria-live="polite" aria-atomic="true" className="sr-only">
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

// --- Public routes (no auth required) ---

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: LoginPage,
});

const authCallbackRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/auth/callback",
  component: AuthCallback,
});

// --- Protected routes (wrapped in AuthGuard) ---

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: function ProtectedActivityLibrary() {
    return (
      <AuthGuard>
        <ActivityLibrary />
      </AuthGuard>
    );
  },
});

const activityDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activities/$activityId",
  component: function ProtectedActivityDetail() {
    const { activityId } = activityDetailRoute.useParams();
    return (
      <AuthGuard>
        <ActivityDetailPage activityId={activityId} />
      </AuthGuard>
    );
  },
});

const activityEditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activities/$activityId/edit",
  component: function ProtectedActivityEdit() {
    const { activityId } = activityEditRoute.useParams();
    return (
      <AuthGuard>
        <RouteEditor activityId={activityId} />
      </AuthGuard>
    );
  },
});

const importSearchSchema = z.object({
  importId: z.string().optional(),
});

const importRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/import",
  validateSearch: importSearchSchema,
  component: function ProtectedImportActivity() {
    return (
      <AuthGuard>
        <ImportActivity />
      </AuthGuard>
    );
  },
});

const exportSearchSchema = z.object({
  exportId: z.string().optional(),
  routeVersionId: z.string().optional(),
});

const exportRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activities/$activityId/export",
  validateSearch: exportSearchSchema,
  component: function ProtectedExportRoute() {
    const { activityId } = exportRoute.useParams();
    return (
      <AuthGuard>
        <ExportRoute activityId={activityId} />
      </AuthGuard>
    );
  },
});

const compareRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activities/$activityId/compare",
  component: function ProtectedRouteComparison() {
    const { activityId } = compareRoute.useParams();
    return (
      <AuthGuard>
        <RouteComparison activityId={activityId} />
      </AuthGuard>
    );
  },
});

const routeTree = rootRoute.addChildren([
  loginRoute,
  authCallbackRoute,
  indexRoute,
  activityDetailRoute,
  activityEditRoute,
  importRoute,
  exportRoute,
  compareRoute,
]);

export const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
