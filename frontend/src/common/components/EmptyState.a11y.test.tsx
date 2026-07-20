// @vitest-environment jsdom
import { describe, it } from "vitest";
import { checkA11y } from "@/test-utils/a11y";
import { EmptyState } from "./EmptyState";

describe("EmptyState accessibility", () => {
  it("has no axe violations with title only", async () => {
    await checkA11y(<EmptyState title="No activities yet" />);
  });

  it("has no axe violations with title and description", async () => {
    await checkA11y(
      <EmptyState
        title="No activities yet"
        description="Your activities will appear here after you import them."
      />,
    );
  });
});
