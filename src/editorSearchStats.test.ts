import { describe, expect, it } from "vitest";
import { EditorSelection, EditorState } from "@codemirror/state";
import { SearchQuery } from "@codemirror/search";
import {
  createMatchScan,
  currentMatchIndex,
  MATCH_LIMIT,
  matchSummaryText,
  sameMatchQuery,
} from "./editorSearchStats";

function scan(
  text: string,
  config: ConstructorParameters<typeof SearchQuery>[0],
) {
  const task = createMatchScan(
    EditorState.create({ doc: text }),
    new SearchQuery(config),
  );
  let result = task.step(7);
  while (result.kind === "pending") result = task.step(7);
  return result;
}

describe("editor match statistics", () => {
  it("distinguishes empty, invalid and valid queries with no matches", () => {
    expect(scan("abc", { search: "" })).toEqual({ kind: "empty" });
    expect(scan("abc", { search: "[", regexp: true })).toEqual({
      kind: "invalid",
    });
    expect(scan("abc", { search: "z" })).toEqual({
      kind: "complete",
      ranges: [],
    });
  });
  it("shares the engine's case and whole-word semantics", () => {
    expect(scan("cat Cat scatter", { search: "cat", wholeWord: true })).toEqual(
      {
        kind: "complete",
        ranges: [
          { from: 0, to: 3 },
          { from: 4, to: 7 },
        ],
      },
    );
    expect(
      scan("cat Cat scatter", { search: "cat", caseSensitive: true }),
    ).toEqual({
      kind: "complete",
      ranges: [
        { from: 0, to: 3 },
        { from: 9, to: 12 },
      ],
    });
  });
  it("handles Unicode, multiline escapes, literal mode and zero-width regex", () => {
    expect(scan("中文 中文", { search: "中文" })).toEqual({
      kind: "complete",
      ranges: [
        { from: 0, to: 2 },
        { from: 3, to: 5 },
      ],
    });
    expect(scan("one\ntwo", { search: "one\\ntwo" })).toEqual({
      kind: "complete",
      ranges: [{ from: 0, to: 7 }],
    });
    expect(scan("one\ntwo", { search: "one\\ntwo", literal: true })).toEqual({
      kind: "complete",
      ranges: [],
    });
    const zero = scan("aa", { search: "(?=a)", regexp: true });
    expect(zero).toEqual({
      kind: "complete",
      ranges: [
        { from: 0, to: 0 },
        { from: 1, to: 1 },
      ],
    });
  });
  it("distinguishes exactly the cap from more than the cap", () => {
    const exact = scan("x ".repeat(MATCH_LIMIT), { search: "x" });
    expect(exact.kind).toBe("complete");
    const capped = scan("x ".repeat(MATCH_LIMIT + 1), { search: "x" });
    expect(capped.kind).toBe("capped");
    if (capped.kind !== "capped") throw Error("expected capped scan");
    expect(capped.ranges).toHaveLength(MATCH_LIMIT);
    expect(capped.lowerBound).toBe(MATCH_LIMIT + 1);
    expect(matchSummaryText(capped, EditorSelection.range(0, 1))).toBe(
      "1000+ 个匹配",
    );
  });
  it("counts incrementally on an immutable snapshot", () => {
    const old = EditorState.create({ doc: "x ".repeat(300) });
    const task = createMatchScan(old, new SearchQuery({ search: "x" }));
    expect(task.step(2)).toEqual({ kind: "pending", found: 2 });
    const next = old.update({
      changes: { from: 0, to: old.doc.length, insert: "no matches" },
    }).state;
    expect(next.doc).not.toBe(old.doc);
    let result = task.step();
    while (result.kind === "pending") result = task.step();
    expect(result.kind === "complete" && result.ranges.length).toBe(300);
    expect(task.step()).toBe(result);
  });
  it("replacement-only edits preserve match cache identity", () => {
    const base = new SearchQuery({
      search: "a",
      replace: "old",
      wholeWord: true,
    });
    expect(
      sameMatchQuery(
        base,
        new SearchQuery({ search: "a", replace: "new", wholeWord: true }),
      ),
    ).toBe(true);
    for (const change of [
      { search: "b" },
      { caseSensitive: true },
      { regexp: true },
      { wholeWord: false },
      { literal: true },
    ])
      expect(
        sameMatchQuery(
          base,
          new SearchQuery({ search: "a", wholeWord: true, ...change }),
        ),
      ).toBe(false);
  });
  it("only labels an exact current selection, including backward selections", () => {
    const ranges = [
      { from: 2, to: 4 },
      { from: 6, to: 8 },
    ];
    expect(currentMatchIndex(ranges, EditorSelection.range(8, 6))).toBe(2);
    expect(currentMatchIndex(ranges, EditorSelection.cursor(7))).toBe(0);
    expect(currentMatchIndex(ranges, EditorSelection.range(2, 3))).toBe(0);
    expect(
      matchSummaryText(
        { kind: "complete", ranges },
        EditorSelection.range(6, 8),
      ),
    ).toBe("2 / 2");
    expect(
      matchSummaryText({ kind: "complete", ranges }, EditorSelection.cursor(0)),
    ).toBe("2 个匹配");
    expect(matchSummaryText({ kind: "complete", ranges: [] })).toBe(
      "无匹配结果",
    );
  });
});
