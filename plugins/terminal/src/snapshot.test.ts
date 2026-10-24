import { describe, expect, it } from "vitest";
import { snapshotChunks } from "./snapshot";

describe("terminal migration snapshots", () => {
  it("preserves Unicode and surrogate pairs at chunk boundaries", () => {
    const snapshot = { schema: 1, screen: `${"a".repeat(65500)}终端🙂${"中🙂\u001b[0m".repeat(40000)}` };
    const chunks = snapshotChunks(snapshot);
    expect(chunks.length).toBeGreaterThan(1);
    expect(JSON.parse(chunks.join(""))).toEqual(snapshot);
    for (const chunk of chunks) {
      expect(new TextEncoder().encode(chunk).byteLength).toBeLessThanOrEqual(256 * 1024);
      expect(chunk.charCodeAt(chunk.length - 1) >= 0xd800 && chunk.charCodeAt(chunk.length - 1) <= 0xdbff).toBe(false);
      expect(new TextEncoder().encode(JSON.stringify({ operationId: "move", index: 0, total: chunks.length, data: chunk })).byteLength).toBeLessThan(1024 * 1024);
    }
  });
  it("rejects snapshots over the shared 4 MiB budget before sending", () => {
    expect(() => snapshotChunks({ screen: "界".repeat(1_400_000) })).toThrow("4 MiB");
  });
  it("keeps a near-limit ASCII snapshot within 64 chunks", () => {
    expect(snapshotChunks({ screen: "a".repeat(4 * 1024 * 1024 - 40) }).length).toBeLessThanOrEqual(64);
  });
});
