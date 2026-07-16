// @vitest-environment jsdom
import { describe, it, vi } from "vitest";
import { checkA11y } from "@/test-utils/a11y";
import { ExportReady } from "./ExportReady";

describe("ExportReady accessibility", () => {
  it("has no axe violations", async () => {
    await checkA11y(
      <ExportReady
        exportStatus={{
          exportId: "export-1",
          activityId: "activity-1",
          routeVersionId: "rv-1",
          format: "gpx",
          status: "ready",
          failureReason: null,
          downloadAvailableUntil: "2025-12-31T23:59:59Z",
          createdAt: "2025-01-01T00:00:00Z",
          updatedAt: "2025-01-01T00:00:00Z",
        }}
        getDownloadUrl={vi.fn()}
      />,
    );
  });

  it("has no axe violations without expiry date", async () => {
    await checkA11y(
      <ExportReady
        exportStatus={null}
        getDownloadUrl={vi.fn()}
      />,
    );
  });
});
