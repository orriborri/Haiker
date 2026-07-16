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

// Mock the useActivities hook
vi.mock("./useActivities", () => ({
  useActivities: vi.fn(),
}));

// Mock useDocumentTitle to avoid side effects
vi.mock("@/hooks/useDocumentTitle", () => ({
  useDocumentTitle: vi.fn(),
}));

import { useActivities } from "./useActivities";
import { ActivityLibrary } from "./ActivityLibrary";

const mockActivities = [
  {
    id: "1",
    title: "Morning Run",
    activityType: "running",
    startedAt: "2025-01-15T08:00:00Z",
    endedAt: "2025-01-15T09:00:00Z",
    createdAt: "2025-01-15T08:00:00Z",
    updatedAt: "2025-01-15T09:00:00Z",
  },
  {
    id: "2",
    title: "Evening Ride",
    activityType: "cycling",
    startedAt: "2025-01-14T17:00:00Z",
    endedAt: "2025-01-14T18:30:00Z",
    createdAt: "2025-01-14T17:00:00Z",
    updatedAt: "2025-01-14T18:30:00Z",
  },
  {
    id: "3",
    title: "Weekend Hike",
    activityType: "hiking",
    startedAt: "2025-01-12T10:00:00Z",
    endedAt: "2025-01-12T14:00:00Z",
    createdAt: "2025-01-12T10:00:00Z",
    updatedAt: "2025-01-12T14:00:00Z",
  },
];

function createTestRouter(component: () => ReactElement) {
  const rootRoute = createRootRoute({ component });
  const routeTree = rootRoute.addChildren([
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/activities/$activityId",
    }),
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/import",
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

describe("ActivityLibrary accessibility", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("has no axe violations with activity list", async () => {
    vi.mocked(useActivities).mockReturnValue({
      data: {
        pages: [
          {
            items: mockActivities,
            pagination: { hasMore: false, pageSize: 20, cursor: null },
          },
        ],
        pageParams: [undefined],
      },
      isLoading: false,
      isError: false,
      error: null,
      fetchNextPage: vi.fn(),
      hasNextPage: false,
      isFetchingNextPage: false,
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivities>);

    const { container } = renderWithProviders(<ActivityLibrary />);
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });

  it("has proper heading hierarchy", async () => {
    vi.mocked(useActivities).mockReturnValue({
      data: {
        pages: [
          {
            items: mockActivities,
            pagination: { hasMore: false, pageSize: 20, cursor: null },
          },
        ],
        pageParams: [undefined],
      },
      isLoading: false,
      isError: false,
      error: null,
      fetchNextPage: vi.fn(),
      hasNextPage: false,
      isFetchingNextPage: false,
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivities>);

    const { container } = renderWithProviders(<ActivityLibrary />);

    // Wait for router to render the component
    await waitFor(() => {
      expect(container.querySelector("h1")).not.toBeNull();
    });

    const h1 = container.querySelector("h1");
    expect(h1?.textContent).toBe("Activities");

    // Verify list structure
    const list = container.querySelector('[role="list"]');
    expect(list).not.toBeNull();
  });

  it("has no axe violations in loading state", async () => {
    vi.mocked(useActivities).mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
      error: null,
      fetchNextPage: vi.fn(),
      hasNextPage: false,
      isFetchingNextPage: false,
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivities>);

    const { container } = renderWithProviders(<ActivityLibrary />);
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });

  it("has no axe violations in error state", async () => {
    vi.mocked(useActivities).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: true,
      error: new Error("Network error"),
      fetchNextPage: vi.fn(),
      hasNextPage: false,
      isFetchingNextPage: false,
      refetch: vi.fn(),
    } as unknown as ReturnType<typeof useActivities>);

    const { container } = renderWithProviders(<ActivityLibrary />);
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});
