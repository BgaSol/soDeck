import { describe, expect, it } from "vitest";
import { fitWorkspaceLayout } from "./workspaceSizing";

describe("workspace sizing", () => {
  it("keeps both sidebars and the terminal inside a small detached window", () => {
    for (const width of [320, 626, 828, 1048, 1408]) {
      for (const preferred of [
        undefined,
        { fileWidth: 520, serviceWidth: 560 },
        { fileWidth: 280, serviceWidth: 330 },
      ]) {
        const result = fitWorkspaceLayout(width, preferred);
        expect(result.fileWidth).toBeGreaterThanOrEqual(result.fileMin - 0.01);
        expect(result.serviceWidth).toBeGreaterThanOrEqual(
          result.serviceMin - 0.01,
        );
        expect(result.fileWidth + result.serviceWidth).toBeLessThanOrEqual(
          result.budget + 0.01,
        );
        expect(
          width - 20 - result.fileWidth - result.serviceWidth,
        ).toBeGreaterThanOrEqual(Math.min(280, (width - 20) * 0.4) - 0.01);
      }
    }
  });
  it("preserves user widths when returning from a narrower window", () => {
    const preferred = { fileWidth: 310, serviceWidth: 350 };
    const narrow = fitWorkspaceLayout(626, preferred);
    expect(narrow.fileWidth).toBeLessThan(preferred.fileWidth);
    expect(fitWorkspaceLayout(1408, preferred)).toMatchObject(preferred);
    expect(preferred).toEqual({ fileWidth: 310, serviceWidth: 350 });
  });
});
