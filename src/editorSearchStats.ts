import type { EditorState, SelectionRange } from "@codemirror/state";
import type { SearchQuery } from "@codemirror/search";

export const MATCH_LIMIT = 1000;
export type MatchRange = { from: number; to: number };
export type MatchSummary =
  | { kind: "empty" | "invalid" }
  | { kind: "pending"; found: number }
  | { kind: "complete"; ranges: readonly MatchRange[] }
  | { kind: "capped"; ranges: readonly MatchRange[]; lowerBound: number };

// Replacement text doesn't affect matching, so typing it must not rescan the file.
export function sameMatchQuery(a: SearchQuery, b: SearchQuery) {
  return (
    a.search === b.search &&
    a.caseSensitive === b.caseSensitive &&
    a.literal === b.literal &&
    a.regexp === b.regexp &&
    a.wholeWord === b.wholeWord &&
    a.test === b.test
  );
}

export function createMatchScan(state: EditorState, query: SearchQuery) {
  let summary: MatchSummary = !query.search
    ? { kind: "empty" }
    : !query.valid
      ? { kind: "invalid" }
      : { kind: "pending", found: 0 };
  const cursor = query.valid ? query.getCursor(state) : null;
  const ranges: MatchRange[] = [];
  return {
    step(batchSize = 100): MatchSummary {
      if (summary.kind !== "pending" || !cursor) return summary;
      const deadline = performance.now() + 4;
      for (let i = 0; i < batchSize; i++) {
        const next = cursor.next();
        if (next.done) return (summary = { kind: "complete", ranges });
        if (ranges.length === MATCH_LIMIT)
          return (summary = {
            kind: "capped",
            ranges,
            lowerBound: MATCH_LIMIT + 1,
          });
        ranges.push({ from: next.value.from, to: next.value.to });
        // A single RegExp cursor call is synchronous; this bounds work BETWEEN calls.
        if (performance.now() >= deadline) break;
      }
      return (summary = { kind: "pending", found: ranges.length });
    },
  };
}

export function currentMatchIndex(
  ranges: readonly MatchRange[],
  selection: SelectionRange,
) {
  let low = 0,
    high = ranges.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (ranges[middle].from < selection.from) low = middle + 1;
    else high = middle;
  }
  return ranges[low]?.from === selection.from &&
    ranges[low]?.to === selection.to
    ? low + 1
    : 0;
}

export function matchSummaryText(
  summary: MatchSummary,
  selection?: SelectionRange,
) {
  switch (summary.kind) {
    case "empty":
      return "输入查找内容";
    case "invalid":
      return "正则表达式无效";
    case "pending":
      return "正在统计…";
    case "capped":
      return `${MATCH_LIMIT}+ 个匹配`;
    case "complete": {
      if (!summary.ranges.length) return "无匹配结果";
      const index = selection
        ? currentMatchIndex(summary.ranges, selection)
        : 0;
      return index
        ? `${index} / ${summary.ranges.length}`
        : `${summary.ranges.length} 个匹配`;
    }
  }
}
