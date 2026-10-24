import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const bytes = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));
    const fixture = { requests: [] as any[], input: [] as { sequence: number; text: string }[], chunks: [] as string[], snapshot: null as any, failInput: "", clipboard: "粘贴🙂", readAcknowledged: 0, generation: 1 };
    (window as any).__TERMINAL_TEST__ = fixture;
    (window as any).__SODECK_CONTEXT__ = { viewId: "fixture-view", viewType: "terminal.session", workspaceId: "fixture-server", serverId: "fixture-server", serverName: "隔离浏览器契约", sessionId: "fixture-session", pluginVersion: "0.1.0", mountGeneration: 1, ...(location.search.includes("recover") ? { snapshot: { recovered: true, acknowledged: 1, lastInputSeq: 4 } } : {}) };
    (window as any).ipc = { postMessage(encoded: string) {
      const request = JSON.parse(encoded); fixture.requests.push(request);
      const params = request.params;
      let result: any = {}, error: any;
      if (request.method === "plugin.invoke") {
        if (params.command === "settings.get" || params.command === "settings.set") result = { fontSize: params.args.fontSize ?? 14 };
        if (params.command === "sessions.get" || params.command === "sessions.reconnect") {
          if (params.command === "sessions.reconnect") { fixture.generation++; fixture.readAcknowledged = 0; }
          result = { sessionId: "fixture-session", ptyId: `fixture-pty-${fixture.generation}`, generation: fixture.generation, status: "active" };
        }
      }
      if (request.method === "view.ready") result = { ready: true };
      if (request.method === "pty.read") result = { frames: params.after === 0 ? [{ sequence: 1, bytes: bytes("\u001b[32m浏览器契约 · Unicode 🙂\u001b[0m\r\n$ ") }] : [], state: "active", lastSequence: 1 };
      if (request.method === "pty.ack") { fixture.readAcknowledged = params.sequence; result = { acknowledged: params.sequence }; }
      if (request.method === "pty.write") {
        if (fixture.failInput) { error = { code: fixture.failInput, message: "隔离故障注入" }; fixture.failInput = ""; }
        else { const data = Uint8Array.from(atob(params.bytes), c => c.charCodeAt(0)); fixture.input.push({ sequence: params.inputSeq, text: new TextDecoder().decode(data) }); result = { inputSeq: params.inputSeq, acceptedBytes: data.length }; }
      }
      if (request.method === "clipboard.readText") result = { text: fixture.clipboard };
      if (request.method === "clipboard.writeText") fixture.clipboard = params.text;
      if (request.method === "view.snapshotChunk") { fixture.chunks[params.index] = params.data; if (params.index + 1 === params.total) fixture.snapshot = JSON.parse(fixture.chunks.join("")); result = { accepted: true }; }
      queueMicrotask(() => (window as any).__sodeckReceive({ requestId: request.requestId, ...(error ? { error } : { result }) }));
    } };
  });
  await page.goto("http://127.0.0.1:1432");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.readAcknowledged)).toBe(1);
});

test("xterm consumes bytes, fits its container and sends exact UTF-8 paste", async ({ page }) => {
  await expect(page.getByText("已连接", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input.map((item: any) => item.text).join(""))).toBe("粘贴🙂");
  const dimensions = await page.evaluate(() => (window as any).__TERMINAL_TEST__.requests.filter((item: any) => item.method === "pty.resize").at(-1)?.params);
  expect(dimensions.cols).toBeGreaterThan(2); expect(dimensions.rows).toBeGreaterThan(1);
  await page.getByRole("button", { name: "增大字号" }).click(); await expect(page.locator(".font-size")).toHaveText("15");
  await page.screenshot({ path: ".qa/ui/terminal-contract.png" });
});

test("snapshot freezes input and resumes only after native cancellation", async ({ page }) => {
  await page.evaluate(() => (window as any).__sodeckNotify({ type: "view.prepareMove", payload: { operationId: "fixture-move" } }));
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.snapshot?.schema)).toBe(1);
  const snapshot = await page.evaluate(() => (window as any).__TERMINAL_TEST__.snapshot);
  expect(snapshot.cursor).toBe(1); expect(snapshot.screen).toContain("Unicode");
  await expect(page.getByRole("button", { name: "粘贴", exact: true })).toBeDisabled();
  await page.evaluate(() => (window as any).__sodeckNotify({ type: "view.moveCancelled", payload: { operationId: "fixture-move" } }));
  await expect(page.getByRole("button", { name: "粘贴", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input.length)).toBe(1);
});

test("backpressure does not skip input sequence; an unknown result blocks further input", async ({ page }) => {
  await page.evaluate(() => { (window as any).__TERMINAL_TEST__.failInput = "BACKPRESSURE"; });
  await page.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("剩余输入未发送");
  await page.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input[0]?.sequence)).toBe(1);
  await page.evaluate(() => { (window as any).__TERMINAL_TEST__.failInput = "RESULT_UNKNOWN"; });
  await page.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect(page.getByText("输入已暂停，请先核对远端执行结果")).toBeVisible();
  await page.getByRole("button", { name: "粘贴", exact: true }).click();
  expect(await page.evaluate(() => (window as any).__TERMINAL_TEST__.input.length)).toBe(1);
});

test("a recovered renderer uses authoritative output and input cursors", async ({ page }) => {
  await page.goto("http://127.0.0.1:1432/?recover");
  await expect(page.getByText("已连接", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toContainText("旧屏幕无法恢复");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.requests.find((item: any) => item.method === "pty.read")?.params.after)).toBe(1);
  await page.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input[0]?.sequence)).toBe(5);
});
