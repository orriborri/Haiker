import { render, type RenderOptions } from "@testing-library/react";
import type { ReactElement } from "react";
import { axe } from "vitest-axe";
import { expect } from "vitest";

// vitest-axe/matchers.d.ts incorrectly uses `export type *` so TypeScript
// cannot see the runtime value of toHaveNoViolations. We work around this
// by importing the module and casting it.
const matchers: { toHaveNoViolations: unknown } = await import(
  "vitest-axe/matchers" as string
);

expect.extend(matchers as Parameters<typeof expect.extend>[0]);

declare module "vitest" {
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface Assertion<T> {
    toHaveNoViolations(): void;
  }
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface AsymmetricMatchersContaining {
    toHaveNoViolations(): void;
  }
}

/**
 * Renders a component and runs axe-core accessibility checks against it.
 * Throws if any violations are detected.
 */
export async function checkA11y(
  ui: ReactElement,
  options?: RenderOptions,
): Promise<void> {
  const { container } = render(ui, options);
  const results = await axe(container);
  expect(results).toHaveNoViolations();
}

export { axe, render };
