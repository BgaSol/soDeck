import { expect, it, vi } from "vitest";
vi.mock("../api", () => ({ desktop: false }));
import { createCopyQueue } from "./clipboardService";
it("orders writes by action, not historical producer completion", async () => {
  const writes: string[] = [];
  const queue = createCopyQueue({
    readText: vi.fn(),
    writeText: async (text) => {
      writes.push(text);
    },
  });
  let release!: (value: string) => void;
  const pending = new Promise<string>((resolve) => {
    release = resolve;
  });
  const first = queue(() => pending);
  const second = queue("new");
  release("old");
  await Promise.all([first, second]);
  expect(writes).toEqual(["old", "new"]);
});
it("skips invalidated copies and continues after failure", async () => {
  const writeText = vi.fn(async () => {});
  const queue = createCopyQueue({ readText: vi.fn(), writeText });
  const failed = queue(async () => {
    throw new Error("history unavailable");
  });
  const stale = queue("stale", () => false);
  const good = queue("valid");
  await expect(failed).rejects.toThrow("history unavailable");
  expect(await stale).toBe(false);
  expect(await good).toBe(true);
  expect(writeText).toHaveBeenCalledExactlyOnceWith("valid");
});
