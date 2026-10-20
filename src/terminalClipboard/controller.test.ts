import { describe, expect, it, vi } from "vitest";
import { TerminalClipboardController } from "./controller";
import type { PasteTarget } from "./types";
const defer = <T>() => {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};
function setup() {
  let target: PasteTarget | null = {
    sessionId: "s1",
    epoch: 1,
    source: { serverName: "test", host: "host", port: 22, username: "ops" },
  };
  const paste = vi.fn(async () => {});
  const readText = vi.fn(async () => "echo ok");
  const copy = vi.fn(async () => true);
  const service = new TerminalClipboardController(
    {
      target: () => target,
      reason: () => "不可输入",
      selection: () => "output",
      bracketed: () => false,
      paste,
      control: vi.fn(),
      focus: vi.fn(),
      selectAll: vi.fn(),
      clipboard: { readText, writeText: vi.fn() },
      copy,
    },
    () => {},
  );
  return {
    service,
    paste,
    readText,
    copy,
    invalidate() {
      target = target ? { ...target, epoch: target.epoch + 1 } : null;
      service.invalidate();
    },
  };
}
describe("paste lifecycle", () => {
  it("submits single-line text exactly once and preserves whitespace", async () => {
    const f = setup();
    await f.service.requestPaste({ text: "  echo ok\t" });
    expect(f.paste).toHaveBeenCalledTimes(1);
    expect(f.paste.mock.calls[0]).toEqual([
      expect.objectContaining({ sessionId: "s1" }),
      "  echo ok\t",
    ]);
    expect(f.readText).not.toHaveBeenCalled();
    f.service.dispose();
  });
  it("does not submit until preview confirmation and never appends Enter", async () => {
    const f = setup();
    await f.service.requestPaste({ text: "echo ok\n" });
    expect(f.paste).not.toHaveBeenCalled();
    const id = f.service.state.preview!.id;
    await f.service.confirm(id, "echo ok");
    await f.service.confirm(id, "echo ok");
    expect(f.paste).toHaveBeenCalledTimes(1);
    expect(f.service.state.busy).toBe(false);
    f.service.dispose();
  });
  it("discards old reads after leaving and returning, without clearing a newer preview", async () => {
    const f = setup();
    const pending = defer<string>();
    f.readText.mockReturnValueOnce(pending.promise);
    const old = f.service.requestPaste();
    f.invalidate();
    await f.service.requestPaste({ text: "new\n" });
    const id = f.service.state.preview!.id;
    pending.resolve("old");
    await old;
    expect(f.paste).not.toHaveBeenCalled();
    expect(f.service.state.preview!.id).toBe(id);
    f.service.dispose();
  });
  it("cancels a preview and rejects repeated requests while reading", async () => {
    const f = setup();
    const pending = defer<string>();
    f.readText.mockReturnValueOnce(pending.promise);
    const first = f.service.requestPaste();
    await f.service.requestPaste();
    expect(f.readText).toHaveBeenCalledTimes(1);
    pending.resolve("one\ntwo");
    await first;
    f.service.cancel();
    expect(f.paste).not.toHaveBeenCalled();
    expect(f.service.state.preview).toBeUndefined();
    f.service.dispose();
  });
  it("blocks control characters and accepts explicit removal", async () => {
    const f = setup();
    await f.service.requestPaste({ text: "x\x1b" });
    const id = f.service.state.preview!.id;
    await f.service.confirm(id, "x\x1b");
    expect(f.paste).not.toHaveBeenCalled();
    await f.service.confirm(id, "x");
    expect(f.paste).toHaveBeenCalledTimes(1);
    f.service.dispose();
  });
  it("does not retry IPC errors and does not call a submitted request cancelled", async () => {
    const f = setup();
    const sent = defer<void>();
    f.paste.mockReturnValueOnce(sent.promise);
    const work = f.service.requestPaste({ text: "one" });
    f.invalidate();
    expect(f.service.state.busy).toBe(true);
    sent.reject(new Error("disconnect"));
    await work;
    expect(f.paste).toHaveBeenCalledTimes(1);
    expect(f.service.state.notice?.text).toContain("不会自动重发");
    f.service.dispose();
  });
  it("ignores failed or completed reads after disposal", async () => {
    const f = setup();
    const pending = defer<string>();
    f.readText.mockReturnValueOnce(pending.promise);
    const work = f.service.requestPaste();
    f.service.dispose();
    pending.resolve("private");
    await work;
    expect(f.paste).not.toHaveBeenCalled();
  });
});
