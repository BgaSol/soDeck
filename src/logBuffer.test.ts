import { describe, expect, it } from "vitest";
import { LogBuffer, logChange } from "./logBuffer";
const bytes = (value: string) => new TextEncoder().encode(value);

describe("bounded log streaming", () => {
  it("decodes split UTF-8 and strips split terminal escape sequences", () => {
    const buffer = new LogBuffer();
    const value = bytes(
      "中文🙂 \x1b[32mgreen\x1b[0m\x1b]0;window title\x07\r\nnext\tline",
    );
    for (const byte of value) buffer.append("stdout", new Uint8Array([byte]));
    buffer.append("stdout");
    expect(buffer.render(true)).toBe("中文🙂 green\nnext\tline");
  });
  it("keeps partial stdout and stderr lines separate and toggles timestamps without data loss", () => {
    const buffer = new LogBuffer();
    buffer.append("stdout", bytes("2026-10-14T09:00:00.000000000Z hel"));
    buffer.append("stderr", bytes("2026-10-14T09:00:01Z failure\n"));
    buffer.append("stdout", bytes("lo\n"));
    expect(buffer.render(false)).toBe("hello\n[stderr] failure");
    expect(buffer.render(true)).toContain(
      "2026-10-14T09:00:00.000000000Z hello",
    );
    expect(buffer.render(false, true)).toBe("hello\nfailure");
  });
  it("evicts oldest lines and forgets removed partial stream lines", () => {
    const buffer = new LogBuffer(3, 100);
    buffer.append("stderr", bytes("old partial"));
    buffer.append("stdout", bytes("a\nb\nc\n"));
    buffer.append("stderr", bytes("new\n"));
    expect(buffer.render(true)).toBe("b\nc\n[stderr] new");
    expect(buffer.dropped).toBe(2);
    expect(buffer.lines.length).toBe(3);
  });
  it("caps byte usage even for an unbroken multibyte line", () => {
    const buffer = new LogBuffer(100, 200);
    buffer.append("stdout", bytes("first\n" + "中文🙂".repeat(200) + "tail"));
    expect(buffer.bytes).toBeLessThanOrEqual(200);
    expect(buffer.dropped).toBeGreaterThan(0);
    expect(buffer.render(true)).toMatch(/tail$/);
    expect(buffer.render(true)).not.toContain("�");
  });
  it("keeps the newest 10000 lines of a large stream", () => {
    const buffer = new LogBuffer();
    buffer.append(
      "stdout",
      bytes(
        Array.from({ length: 12000 }, (_, i) => `line-${i}`).join("\n") + "\n",
      ),
    );
    expect(buffer.lines.length).toBe(10000);
    expect(buffer.dropped).toBe(2000);
    expect(buffer.render(true)).toMatch(/^line-2000\n/);
    expect(buffer.render(true)).toMatch(/line-11999$/);
  });
  it.each([
    ["abc", "abcdef"],
    ["old\nkept", "kept\nnew"],
    ["a🙂b", "a😀b"],
    ["text", ""],
    ["", "new"],
  ])("applies a minimal update from %s to %s", (previous, next) => {
    const change = logChange(previous, next);
    expect(
      previous.slice(0, change.from) +
        change.insert +
        previous.slice(change.to),
    ).toBe(next);
  });
});
