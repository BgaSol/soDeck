import { describe, expect, it } from "vitest";
import {
  defaultWorkspaceView,
  fitWorkspaceLayout,
  normalizeWorkspaceView,
  workspaceVisibility,
} from "./workspaceSizing";
describe("workspace sizing", () => {
  it("uses the available workspace width and reserves 320px for a visible terminal", () => {
    for (const width of [320, 650, 759, 760, 860, 1199, 1200, 1440]) {
      for (const preferred of [
        undefined,
        { fileWidth: 520, serviceWidth: 560 },
        { fileWidth: 280, serviceWidth: 330 },
      ]) {
        const result = fitWorkspaceLayout(width, preferred);
        expect(result.mode).toBe(
          width >= 1200 ? "three" : width >= 760 ? "two" : "single",
        );
        if (result.mode === "three")
          expect(
            width - 20 - result.fileWidth - result.serviceWidth,
          ).toBeGreaterThanOrEqual(320);
        if (result.mode === "two") {
          expect(width - 10 - result.serviceWidth).toBeGreaterThanOrEqual(320);
          expect(width - 10 - result.fileWidth).toBeGreaterThanOrEqual(320);
        }
        expect(result.serviceWidth).toBeLessThanOrEqual(480);
      }
    }
  });
  it("fits without overwriting user widths and compresses files first", () => {
    const preferred = { fileWidth: 520, serviceWidth: 480 };
    expect(fitWorkspaceLayout(1200, preferred)).toMatchObject({
      fileWidth: 380,
      serviceWidth: 480,
    });
    expect(fitWorkspaceLayout(760, preferred).serviceWidth).toBe(430);
    expect(fitWorkspaceLayout(1440, preferred)).toMatchObject(preferred);
    expect(preferred).toEqual({ fileWidth: 520, serviceWidth: 480 });
  });
  it("selects one narrow pane and isolates focus mode at every width", () => {
    expect(workspaceVisibility("single", defaultWorkspaceView)).toEqual({
      files: false,
      terminal: true,
      services: false,
    });
    expect(
      workspaceVisibility("two", { expanded: false, pane: "files" }),
    ).toEqual({ files: true, terminal: true, services: false });
    for (const mode of ["single", "two", "three"] as const)
      expect(
        workspaceVisibility(mode, { expanded: true, pane: "files" }),
      ).toEqual({ files: false, terminal: false, services: true });
    expect(normalizeWorkspaceView()).toEqual(defaultWorkspaceView);
  });
});
