import { describe, it, expect } from "vitest";
import { Explorer } from "./state";
import { root } from "./remote-path";
import type { Bridge, Attributes } from "./remote";
const directory: Attributes = {
  type: "directory",
  size: null,
  mtime: null,
  atime: null,
  permissions: null,
  uid: null,
  gid: null,
};
const file: Attributes = { ...directory, type: "file", size: "1" };
function fixture(total = 1) {
  let cursor = 0,
    sequence = 0,
    removed = false;
  const bridge: Bridge = {
    async call<T>(method: string, p: Record<string, unknown> = {}) {
      let result: unknown;
      switch (method) {
        case "files.openDirectory":
          cursor = 0;
          result = { directoryId: "directory-" + ++sequence, cursor: 0 };
          break;
        case "files.readDirectory": {
          const count = Math.min(256, total - cursor);
          result = {
            entries: Array.from({ length: count }, (_, i) => ({
              name: { bytesBase64: btoa("file-" + (cursor + i)) },
              attributes: file,
            })),
            nextCursor: Number(p.cursor) + 1,
            eof: cursor + count === total,
          };
          cursor += count;
          break;
        }
        case "files.stat":
          if (removed) throw { code: "NOT_FOUND", message: "已不存在" };
          result = file;
          break;
        case "files.closeDirectory":
        case "files.close":
        case "exec.close":
          result = { closed: true };
          break;
        case "remote.operations.forget":
          result = { forgotten: true };
          break;
        case "operations.cancel":
          result = { canceled: true };
          break;
        case "connections.release":
          result = { released: true };
          break;
        default:
          throw Error(method);
      }
      return result as T;
    },
  };
  let explorer: Explorer;
  explorer = new Explorer(
    bridge,
    { workspaceId: "w", workspaceGeneration: "g" },
    "s",
    "v",
    () => explorer.cacheBytes(),
  );
  explorer.visible = true;
  explorer.remote.lease = {
    leaseId: "lease",
    connectionId: "connection",
    generation: 1,
  };
  explorer.nodes.get(root.bytesBase64)!.expanded = true;
  return {
    explorer,
    remove() {
      removed = true;
      total = 0;
    },
  };
}
describe("file worker lifecycle and budgets", () => {
  it("refresh removes disappeared descendants and selects the existing parent", async () => {
    const f = fixture();
    await f.explorer.load(root.bytesBase64);
    const child = f.explorer.nodes.get(root.bytesBase64)!.children[0];
    await f.explorer.select(child);
    f.remove();
    await f.explorer.load(root.bytesBase64, true);
    expect(f.explorer.selected).toBe(root.bytesBase64);
    expect(f.explorer.nodes.has(child)).toBe(false);
    expect(f.explorer.nodes.size).toBe(1);
    await f.explorer.dispose();
  });
  it("enforces the node budget with an explicit incomplete listing", async () => {
    const { explorer: e } = fixture(22000);
    for (let i = 0; i < 80; i++) await e.load(root.bytesBase64);
    expect(e.nodes.size).toBeLessThanOrEqual(20000);
    expect(e.nodes.get(root.bytesBase64)!.error).toMatch(/上限|刷新/);
    expect(e.cacheBytes()).toBeLessThanOrEqual(12 * 1024 * 1024);
    await e.dispose();
  });
  it("bounds long-path replies without losing viewport rows across pages", async () => {
    const { explorer: e } = fixture(256);
    await e.load(root.bytesBase64);
    for (const n of e.nodes.values())
      if (n.path !== root) n.name = "长".repeat(4000);
    e.changed();
    let start = 0;
    const ids: string[] = [];
    for (;;) {
      const page = e.snapshot(start, 300);
      expect(
        new TextEncoder().encode(JSON.stringify(page)).length,
      ).toBeLessThanOrEqual(256 * 1024);
      ids.push(...page.rows.map((r) => r.id));
      start += page.rows.length;
      if (start >= page.total) break;
      expect(page.rows.length).toBeGreaterThan(0);
    }
    expect(new Set(ids).size).toBe(e.flat().length);
    await e.dispose();
  });
});
