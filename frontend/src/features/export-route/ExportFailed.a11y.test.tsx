// @vitest-environment jsdom
import { describe, it, vi } from "vitest";
import { checkA11y } from "@/test-utils/a11y";
import { ExportFailed } from "./ExportFailed";

describe("ExportFailed accessibility", () => {
  it("has no axe violations with failure reason", async () => {
    await checkA11y(
      <ExportFailed
        failureReason="Server error during file generation"
        onRetry={vi.fn()}
      />,
    );
  });

  it("has no axe violations without failure reason", async () => {
    await checkA11y(<ExportFailed failureReason={null} onRetry={vi.fn()} />);
  });
});
