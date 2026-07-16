// @vitest-environment jsdom
import { describe, it, vi, expect, beforeEach } from "vitest";
import { render, axe } from "@/test-utils/a11y";
import { waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createRouter,
  createRootRoute,
  createRoute,
  RouterProvider,
  createMemoryHistory,
} from "@tanstack/react-router";
import { type ReactElement } from "react";

// Mock hooks
vi.mock("./useActivity", () => ({
  useActivity: vi.fn(),
}));

vi.mock("./useRecordedRoute", () => ({
  useRecordedRoute: vi.fn(),
}));

vi.mock("@/hooks/useDocumentTitle", () => ({
  useDocumentTitle: vi.fn(),
}));

// Mock RouteMap since it uses maplibre-gl which requires canvas
vi.mock("./RouteMap", () => ({
  RouteMap: () => <div data-testid="route-map">Route Map</div>,
}));

import { useActivity } from "./useActivity";
import { useRecordedRoute } from "./useRecordedRoute";
import { ActivityDetailPage } from "./ActivityDetail";

const mockActivity = {
  id: "act-123",
  title: "Morning Trail Run",
  activityType: "running",
  startedAt: "2025-01-15T08:00:00Z",
  endedAt: "2025-01-15T09:30:00Z",
  lifecycleState: "completed",
  createdAt: "2025-01-15T08:00:00Z",
  updatedAt: "2025-01-15T09:30:00Z",
};

function createTestRouter(component: () => ReactElement) {
  const rootRoute = createRootRoute({ component });
  const routeTree = rootRoute.addChildren([
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/",
    }),
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/activities/$activityId/edit",
    }),
  ]);
  return createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
}

function renderWithProviders(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const router = createTestRouter(() => (
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>
  ));
  return render(<RouterProvider router={router} />);
}

describe("ActivityDetailPage accessibility", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("has no axe violations with loaded activity", async () => {
    vi.mocked(useActivity).mockReturnValue({
      data: mockActivity,
      isLoading: false,
      isError: false,
      error: null,
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivity>);

    vi.mocked(useRecordedRoute).mockReturnValue({
      data: {
        type: "FeatureCollection" as const,
        bbox: [11.0, 47.0, 11.5, 47.5],
        features: [],
        properties: null,
      },
      isLoading: false,
      isError: false,
    } as unknown as ReturnType<typeof useRecordedRoute>);

    const { container } = renderWithProviders(
      <ActivityDetailPage activityId="act-123" />,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });

  it("has proper heading hierarchy", async () => {
    vi.mocked(useActivity).mockReturnValue({
      data: mockActivity,
      isLoading: false,
      isError: false,
      error: null,
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivity>);

    vi.mocked(useRecordedRoute).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
    } as unknown as ReturnType<typeof useRecordedRoute>);

    const { container } = renderWithProviders(
      <ActivityDetailPage activityId="act-123" />,
    );

    // Wait for router to render the component
    await waitFor(() => {
      expect(container.querySelector("h1")).not.toBeNull();
    });

    // h1 for activity title
    const h1 = container.querySelector("h1");
    expect(h1?.textContent).toBe("Morning Trail Run");

    // h2 for details section
    const h2 = container.querySelector("h2");
    expect(h2).not.toBeNull();
    expect(h2?.textContent).toBe("Details");
  });

  it("has proper landmark structure", async () => {
    vi.mocked(useActivity).mockReturnValue({
      data: mockActivity,
      isLoading: false,
      isError: false,
      error: null,
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivity>);

    vi.mocked(useRecordedRoute).mockReturnValue({
      data: {
        type: "FeatureCollection" as const,
        bbox: [11.0, 47.0, 11.5, 47.5],
        features: [],
        properties: null,
      },
      isLoading: false,
      isError: false,
    } as unknown as ReturnType<typeof useRecordedRoute>);

    const { container } = renderWithProviders(
      <ActivityDetailPage activityId="act-123" />,
    );

    // Wait for router to render the component
    await waitFor(() => {
      expect(
        container.querySelectorAll("section[aria-label]").length,
      ).toBeGreaterThanOrEqual(2);
    });

    const sections = container.querySelectorAll("section[aria-label]");
    const labels = Array.from(sections).map((s) =>
      s.getAttribute("aria-label"),
    );
    expect(labels).toContain("Route map");
    expect(labels).toContain("Activity details");
  });

  it("has no axe violations in error state", async () => {
    vi.mocked(useActivity).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: true,
      error: new Error("Activity not found"),
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivity>);

    vi.mocked(useRecordedRoute).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
    } as unknown as ReturnType<typeof useRecordedRoute>);

    const { container } = renderWithProviders(
      <ActivityDetailPage activityId="act-123" />,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});
