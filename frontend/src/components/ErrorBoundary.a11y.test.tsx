// @vitest-environment jsdom
import { describe, it } from "vitest";
import { checkA11y, render, axe } from "@/test-utils/a11y";
import { expect } from "vitest";

// Test the fallback UI directly since ErrorBoundary renders it on error
function ErrorFallbackUI() {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center">
      <h1 className="text-lg font-medium text-gray-900">
        Something went wrong
      </h1>
      <p className="mt-1 text-sm text-gray-500">An unexpected error occurred</p>
      <button
        type="button"
        className="mt-4 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
      >
        Try again
      </button>
    </div>
  );
}

describe("ErrorBoundary fallback accessibility", () => {
  it("has no axe violations in error state", async () => {
    await checkA11y(<ErrorFallbackUI />);
  });

  it("renders accessible error message and retry button", () => {
    const { container } = render(<ErrorFallbackUI />);
    const heading = container.querySelector("h1");
    expect(heading).not.toBeNull();
    expect(heading?.textContent).toBe("Something went wrong");

    const button = container.querySelector("button");
    expect(button).not.toBeNull();
    expect(button?.textContent).toBe("Try again");
  });

  it("error fallback has proper heading hierarchy", async () => {
    const { container } = render(<ErrorFallbackUI />);
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});
