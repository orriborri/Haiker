// @vitest-environment jsdom
import { describe, it } from "vitest";
import { checkA11y } from "@/test-utils/a11y";
import { LoadingSpinner } from "./LoadingSpinner";

describe("LoadingSpinner accessibility", () => {
  it("has no axe violations", async () => {
    await checkA11y(<LoadingSpinner />);
  });

  it("has no axe violations with custom class", async () => {
    await checkA11y(<LoadingSpinner className="py-16" />);
  });
});
