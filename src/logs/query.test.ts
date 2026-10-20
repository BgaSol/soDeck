import { it, expect } from "vitest";
import { bounded, mergeRows, MAX_ROWS, MAX_BYTES } from "./model";
import type { LogRow } from "../logApi";
const row = (seq: number, raw = "row"): LogRow => ({
  seq,
  raw,
  text: raw,
  prefix: "",
  stream: "stdout",
  timestamp: null,
  level: "",
  complete: true,
  continuation: false,
});
it("replayed updates replace a record without inventing a new sequence", () => {
  const next = mergeRows(
    [row(1, "partial"), row(2)],
    [row(1, "completed"), row(3)],
  );
  expect(next.map((r) => r.seq)).toEqual([1, 2, 3]);
  expect(next[0].raw).toBe("completed");
});
it("keeps the requested edge within both row and byte budgets", () => {
  const many = Array.from({ length: MAX_ROWS + 50 }, (_, i) => row(i + 1));
  expect(bounded(many)[0].seq).toBe(51);
  expect(bounded(many, false).at(-1)?.seq).toBe(MAX_ROWS);
  const large = Array.from({ length: 30 }, (_, i) =>
    row(i + 1, "x".repeat(400000)),
  );
  const kept = bounded(large);
  expect(kept.reduce((n, r) => n + 4 * r.raw.length, 0)).toBeLessThanOrEqual(
    MAX_BYTES,
  );
  expect(kept.at(-1)?.seq).toBe(30);
});
