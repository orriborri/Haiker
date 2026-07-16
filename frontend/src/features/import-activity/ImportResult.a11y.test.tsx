// @vitest-environment jsdom
import { describe, it, vi } from "vitest";
import { checkA11y } from "@/test-utils/a11y";
import { ImportResult } from "./ImportResult";
import {
  createRouter,
  createRootRoute,
  createRoute,
  RouterProvider,
  createMemoryHistory,
} from "@tanstack/react-router";
import { type ReactElement } from "react";

function createTestRouter(component: () => ReactElement) {
  const rootRoute = createRootRoute({ component });
  const routeTree = rootRoute.addChildren([
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/activities/$activityId",
    }),
  ]);
  return createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
}

function renderWithRouter(ui: ReactElement) {
  const router = createTestRouter(() => ui);
  return <RouterProvider router={router} />;
}

describe("ImportResult accessibility", () => {
  it("has no axe violations in completed phase", async () => {
    await checkA11y(
      renderWithRouter(
        <ImportResult
          phase="completed"
          activityId="test-123"
          duplicateActivityId={null}
          error={null}
          onRetry={vi.fn()}
        />,
      ),
    );
  });

  it("has no axe violations in failed phase", async () => {
    await checkA11y(
      renderWithRouter(
        <ImportResult
          phase="failed"
          activityId={null}
          duplicateActivityId={null}
          error="Something went wrong during import"
          onRetry={vi.fn()}
        />,
      ),
    );
  });

  it("has no axe violations in duplicate phase", async () => {
    await checkA11y(
      renderWithRouter(
        <ImportResult
          phase="duplicate"
          activityId={null}
          duplicateActivityId="existing-456"
          error={null}
          onRetry={vi.fn()}
        />,
      ),
    );
  });
});
