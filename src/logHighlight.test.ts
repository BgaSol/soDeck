import { describe, expect, it } from "vitest";
import { ansiText, highlightLog } from "./logHighlight";
const options = {
  timestamps: true,
  highlight: true,
  keyword: "",
  caseSensitive: false,
};
describe("log display highlighting", () => {
  it("preserves text and highlights JSON fields, timestamps, and search over syntax", () => {
    const raw =
      '2026-10-14T09:00:00Z {"level":"ERROR","message":"中文错误","count":12,"ok":false}';
    const spans = highlightLog(raw, { ...options, keyword: "中文错误" });
    expect(spans.map((s) => s.text).join("")).toBe(raw);
    expect(spans.some((s) => s.kind === "key")).toBe(true);
    expect(spans.some((s) => s.kind === "number")).toBe(true);
    expect(spans.some((s) => s.match && s.text === "中文错误")).toBe(true);
    expect(
      highlightLog(raw, { ...options, timestamps: false })
        .map((s) => s.text)
        .join(""),
    ).toMatch(/^\{/);
  });
  it("uses safe ANSI colors and removes OSC, cursor moves and control sequences", () => {
    const raw =
      "\x1b]8;;https://untrusted.test\x07link\x1b]8;;\x07 \x1b[32mgreen\x1b[0m\x1b[2J\r";
    const parsed = ansiText(raw);
    expect(parsed.text).toBe("link green");
    expect(parsed.styles[0].color).toBe("green");
    const spans = highlightLog(raw, {
      ...options,
      highlight: false,
      keyword: "GREEN",
    });
    expect(spans.some((s) => s.match)).toBe(true);
    expect(spans.some((s) => s.color)).toBe(false);
  });
  it("treats regex and HTML input as literal text", () => {
    const raw = "<script>alert(1)</script> a+b [ERROR]";
    expect(
      highlightLog(raw, { ...options, keyword: "a+b" })
        .filter((s) => s.match)
        .map((s) => s.text)
        .join(""),
    ).toBe("a+b");
  });
});
