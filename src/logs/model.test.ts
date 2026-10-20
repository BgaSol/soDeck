import { describe, it, expect } from "vitest";
import { readReducer, collectionLabel, validRange } from "./model";
describe("log reading intent", () => {
  it("only a browsing pause resumes on user-driven arrival at bottom", () => {
    for (const reason of ["browse", "manual", "selection"] as const) {
      const next = readReducer(
        { mode: "paused", reason },
        {
          type: "bottom",
          userInitiated: true,
          hasSelection: false,
          live: true,
        },
      );
      expect(next.mode).toBe(reason === "browse" ? "follow" : "paused");
    }
  });
  it("layout changes and historical snapshots never resume scrolling", () => {
    for (const input of [
      { userInitiated: false, hasSelection: false, live: true },
      { userInitiated: true, hasSelection: true, live: true },
      { userInitiated: true, hasSelection: false, live: false },
    ])
      expect(
        readReducer(
          { mode: "paused", reason: "browse" },
          { type: "bottom", ...input },
        ),
      ).toEqual({ mode: "paused", reason: "browse" });
  });
  it("browsing cannot override an explicit pause", () =>
    expect(
      readReducer(
        { mode: "paused", reason: "manual" },
        { type: "pause", reason: "browse" },
      ),
    ).toEqual({ mode: "paused", reason: "manual" }));
  it("does not equate a missing container with a transport outage", () => {
    expect(
      collectionLabel({ phase: "failed", reason: "containerMissing" }),
    ).toBe("原容器已不存在");
    expect(
      collectionLabel({ phase: "reconnecting", retryAt: 9000 }, 1000),
    ).toBe("正在重连 · 8 秒后重试");
  });
  it("rejects fractional or reversed sequence ranges", () => {
    expect(validRange("1.5", "2")).toBe(false);
    expect(validRange("4", "2")).toBe(false);
    expect(validRange("1", "2")).toBe(true);
  });
});
