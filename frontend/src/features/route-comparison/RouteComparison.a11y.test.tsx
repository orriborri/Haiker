// @vitest-environment jsdom
import { describe, it, vi, expect, beforeEach } from "vitest";
import { render, axe } from "@/test-utils/a11y";
import { screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createRouter,
  createRootRoute,
  createRoute,
  RouterProvider,
  createMemoryHistory,
} from "@tanstack/react-router";
import { type ReactElement } from "react";

// Mock the map component since it uses maplibre-gl which requires canvas
vi.mock("./RouteComparisonMap", () => ({
  RouteComparisonMap: () => (
    <div data-testid="route-comparison-map">Route Comparison Map</div>
  ),
}));

vi.mock("./useRouteComparison", () => ({
  useRouteComparison: vi.fn(),
}));

vi.mock("@/features/route-history", () => ({
  useRouteVersions: vi.fn(),
}));

import { useRouteComparison } from "./useRouteComparison";
import { useRouteVersions } from "@/features/route-history";
import { RouteComparison } from "./RouteComparison";

const mockVersions = {
  items: [
    {
      id: "ver-1",
      activityId: "act-123",
      parentVersionId: null,
      versionNumber: 1,
      editSummary: null,
      correctedStatistics: {
        distanceMeters: 5000,
        pointCount: 100,
        calculationVersion: "1.0",
      },
      calculationVersion: "1.0",
      createdBy: "user-1",
      createdAt: "2025-01-15T08:00:00Z",
    },
    {
      id: "ver-2",
      activityId: "act-123",
      parentVersionId: "ver-1",
      versionNumber: 2,
      editSummary: "Fixed a detour",
      correctedStatistics: {
        distanceMeters: 4800,
        pointCount: 95,
        calculationVersion: "1.0",
      },
      calculationVersion: "1.0",
      createdBy: "user-1",
      createdAt: "2025-01-16T10:00:00Z",
    },
  ],
  pagination: { cursor: null, hasMore: false, pageSize: 20 },
};

const mockComparison = {
  recorded: {
    geometry: {
      type: "FeatureCollection" as const,
      bbox: [11.0, 47.0, 11.5, 47.5],
      features: [
        {
          type: "Feature" as const,
          geometry: {
            type: "LineString",
            coordinates: [
              [11.0, 47.0],
              [11.5, 47.5],
            ],
          },
          properties: { segmentIndex: 0, pointCount: 2 },
        },
      ],
    },
    statistics: {
      distanceMeters: 5000,
      elevationGainMeters: 200,
      elevationLossMeters: 150,
      pointCount: 100,
      segmentCount: 1,
    },
  },
  corrected: {
    geometry: {
      type: "FeatureCollection" as const,
      bbox: [11.0, 47.0, 11.5, 47.5],
      features: [
        {
          type: "Feature" as const,
          geometry: {
            type: "LineString",
            coordinates: [
              [11.0, 47.0],
              [11.5, 47.5],
            ],
          },
          properties: { pointCount: 95, distanceMeters: 4800 },
        },
      ],
    },
    statistics: {
      distanceMeters: 4800,
      pointCount: 95,
      calculationVersion: "1.0",
    },
    versionNumber: 2,
    editSummary: "Fixed a detour",
  },
  sharedBbox: [11.0, 47.0, 11.5, 47.5],
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
      path: "/activities/$activityId",
    }),
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/activities/$activityId/compare",
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

describe("RouteComparison accessibility", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("has no axe violations with loaded comparison", async () => {
    vi.mocked(useRouteVersions).mockReturnValue({
      data: mockVersions,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteVersions>);

    vi.mocked(useRouteComparison).mockReturnValue({
      data: mockComparison,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteComparison>);

    const { container } = renderWithProviders(
      <RouteComparison activityId="act-123" />,
    );

    await waitFor(() => {
      expect(container.querySelector("h1")).not.toBeNull();
    });

    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });

  it("legend is accessible with aria-label", async () => {
    vi.mocked(useRouteVersions).mockReturnValue({
      data: mockVersions,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteVersions>);

    vi.mocked(useRouteComparison).mockReturnValue({
      data: mockComparison,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteComparison>);

    const { container } = renderWithProviders(
      <RouteComparison activityId="act-123" />,
    );

    await waitFor(() => {
      expect(
        container.querySelector('[aria-label="Map legend"]'),
      ).not.toBeNull();
    });

    // Legend region exists
    const legendRegion = container.querySelector('[aria-label="Map legend"]');
    expect(legendRegion).not.toBeNull();

    // Legend contains labeled items
    const legendList = container.querySelector(
      '[aria-label="Route line styles"]',
    );
    expect(legendList).not.toBeNull();
    expect(legendList?.querySelectorAll("li").length).toBe(2);
  });

  it("recorded and corrected routes are labeled distinctly", async () => {
    vi.mocked(useRouteVersions).mockReturnValue({
      data: mockVersions,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteVersions>);

    vi.mocked(useRouteComparison).mockReturnValue({
      data: mockComparison,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteComparison>);

    const { container } = renderWithProviders(
      <RouteComparison activityId="act-123" />,
    );

    await waitFor(() => {
      expect(
        screen.getAllByText(/Recorded \(solid blue line\)/).length,
      ).toBeGreaterThan(0);
    });

    expect(
      screen.getAllByText(/Corrected v2 \(dashed orange line\)/).length,
    ).toBeGreaterThan(0);

    // Statistics headings are distinct
    const recordedHeadings = container.querySelectorAll("h3");
    const headingTexts = Array.from(recordedHeadings).map(
      (h) => h.textContent,
    );
    expect(headingTexts).toContain("Recorded");
    expect(headingTexts).toContain("Corrected v2");
  });

  it("version selector is present and labeled", async () => {
    vi.mocked(useRouteVersions).mockReturnValue({
      data: mockVersions,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteVersions>);

    vi.mocked(useRouteComparison).mockReturnValue({
      data: mockComparison,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteComparison>);

    renderWithProviders(<RouteComparison activityId="act-123" />);

    await waitFor(() => {
      expect(screen.getByLabelText("Compare with:")).toBeDefined();
    });

    const select = screen.getByLabelText("Compare with:");
    expect(select.tagName).toBe("SELECT");
  });

  it("has no axe violations in loading state", async () => {
    vi.mocked(useRouteVersions).mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteVersions>);

    vi.mocked(useRouteComparison).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useRouteComparison>);

    const { container } = renderWithProviders(
      <RouteComparison activityId="act-123" />,
    );

    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});
