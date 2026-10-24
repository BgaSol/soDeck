import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 220, height: 560 });
  await page.addInitScript(() => {
    const sessions = [{ sessionId: "session-1", viewId: "view-1", title: "终端 1", status: "active", serverId: "server-1" }];
    const fixture = { calls: [] as any[], sessions, failRename: false };
    (window as any).__SESSION_TEST__ = fixture;
    (window as any).__SODECK_CONTEXT__ = { viewId: "list-1", viewType: "terminal.sessions", workspaceId: "server-1", serverId: "server-1", serverName: "隔离列表契约" };
    (window as any).ipc = { postMessage(encoded: string) {
      const request = JSON.parse(encoded); fixture.calls.push(request);
      let result: any = {}, error: any;
      if (request.method === "plugin.invoke" && request.params.command === "sessions.list") result = sessions.map(session => ({ ...session }));
      if (request.method === "plugin.invoke" && request.params.command === "sessions.rename") {
        if (fixture.failRename) { error = { code: "BUSY", message: "隔离重命名失败，请重试" }; fixture.failRename = false; }
        else { sessions[0].title = request.params.args.title; result = { ...sessions[0] }; }
      }
      queueMicrotask(() => (window as any).__sodeckReceive({ requestId: request.requestId, ...(error ? { error } : { result }) }));
    } };
  });
  await page.goto("http://127.0.0.1:1432");
  await page.locator(".session-row").first().hover();
  await expect(page.getByRole("button", { name: "重命名 终端 1" })).toBeAttached();
});

test("inline rename cancels without writing, validates and saves trimmed title", async ({ page }) => {
  const start = page.getByRole("button", { name: "重命名 终端 1" });
  await start.focus(); await start.press("Enter");
  const field = page.getByLabel("终端显示名称");
  await expect(field).toBeFocused(); await expect(field).toHaveValue("终端 1");
  await field.fill("临时草稿"); await field.press("Escape");
  await expect(field).toHaveCount(0); await expect(start).toBeFocused();
  expect(await page.evaluate(() => (window as any).__SESSION_TEST__.calls.filter((call: any) => call.params.command === "sessions.rename").length)).toBe(0);
  await start.press("Enter"); await field.fill("    ");
  await expect(page.getByRole("button", { name: "保存名称" })).toBeDisabled();
  await field.fill("x".repeat(81)); await expect(page.getByRole("button", { name: "保存名称" })).toBeDisabled();
  await field.fill("  发布窗口 🙂  ");
  await page.screenshot({ path: ".qa/ui/session-rename-contract.png" });
  await field.press("Enter");
  await expect(page.getByRole("button", { name: "重命名 发布窗口 🙂" })).toBeFocused();
  await expect.poll(() => page.evaluate(() => (window as any).__SESSION_TEST__.calls.find((call: any) => call.params.command === "sessions.rename")?.params.args)).toEqual({ sessionId: "session-1", title: "发布窗口 🙂" });
  expect(await page.evaluate(() => (window as any).__SESSION_TEST__.sessions[0].viewId)).toBe("view-1");
});

test("composition Enter is ignored and a failed save keeps the draft for retry", async ({ page }) => {
  const start = page.getByRole("button", { name: "重命名 终端 1" });
  await start.focus(); await start.press("Enter");
  const field = page.getByLabel("终端显示名称"); await field.fill("组合中的名字");
  await field.dispatchEvent("keydown", { key: "Enter", code: "Enter", isComposing: true, bubbles: true, cancelable: true });
  expect(await page.evaluate(() => (window as any).__SESSION_TEST__.calls.filter((call: any) => call.params.command === "sessions.rename").length)).toBe(0);
  await page.evaluate(() => { (window as any).__SESSION_TEST__.failRename = true; });
  await field.press("Enter"); await expect(page.getByRole("alert")).toContainText("隔离重命名失败"); await expect(field).toHaveValue("组合中的名字");
  await page.getByRole("button", { name: "保存名称" }).click();
  await expect(page.getByRole("button", { name: "重命名 组合中的名字" })).toBeAttached();
  expect(await page.evaluate(() => (window as any).__SESSION_TEST__.calls.filter((call: any) => call.params.command === "sessions.rename").length)).toBe(2);
  expect(await page.evaluate(() => (window as any).__SESSION_TEST__.calls.some((call: any) => ["pty.open", "pty.close", "workbench.close"].includes(call.method)))).toBe(false);
  await page.screenshot({ path: ".qa/ui/session-list-contract.png" });
});
