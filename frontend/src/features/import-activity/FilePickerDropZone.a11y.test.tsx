// @vitest-environment jsdom
import { describe, it, vi } from "vitest";
import { checkA11y } from "@/test-utils/a11y";
import { FilePickerDropZone } from "./FilePickerDropZone";

describe("FilePickerDropZone accessibility", () => {
  it("has no axe violations in default state", async () => {
    await checkA11y(<FilePickerDropZone onFileSelected={vi.fn()} />);
  });

  it("has no axe violations when disabled", async () => {
    await checkA11y(
      <FilePickerDropZone onFileSelected={vi.fn()} disabled={true} />,
    );
  });
});
