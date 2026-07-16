// @vitest-environment jsdom
import { describe, it, vi } from "vitest";
import { checkA11y } from "@/test-utils/a11y";
import { ExportExpired } from "./ExportExpired";

describe("ExportExpired accessibility", () => {
  it("has no axe violations", async () => {
    await checkA11y(<ExportExpired onRetry={vi.fn()} />);
  });
});
