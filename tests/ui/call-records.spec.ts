import { test, expect } from "@playwright/test";

test("call viewer pages with opaque cursor, resets filters and ignores stale replies", async ({ page }) => {
  await page.addInitScript(() => {
    const root = window as any;
    const calls: any[] = [];
    root.__CALL_TEST__ = { calls };
    root.__TAURI_INTERNALS__ = { invoke: async (_: string, { request }: any) => {
      let result: any = {};
      if (request.method === "servers.list") result = { servers: [] };
      if (request.method === "workbench.snapshot") result = { views: [], pendingOpens: [], activeWorkspaceId: null };
      if (request.method === "calls.list") {
        calls.push(request.params);
        const p = request.params;
        if (p.method === "slow") await new Promise(resolve => { root.__CALL_TEST__.releaseSlow = resolve; });
        const filtered = p.method || p.pluginId || p.serverId || p.status;
        result = { records: [{ id: p.cursor ? "older" : "current", started: 1000, caller: { pluginId: "sodeck.terminal" }, method: p.method === "slow" ? "stale.reply" : filtered ? "filtered.call" : p.cursor ? "older.call" : "first.call", status: "succeeded" }], nextCursor: !filtered && !p.cursor ? { started: 1000, id: "current" } : null, retainedFrom: 500 };
      }
      return { requestId: request.requestId, result };
    } };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "调用记录", exact: true }).click();
  await expect(page.getByText("first.call", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "加载更早记录" }).click();
  await expect(page.getByText("older.call", { exact: true })).toBeVisible();
  await expect(page.getByText("first.call", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__CALL_TEST__.calls.at(-1).cursor)).toEqual({ started: 1000, id: "current" });
  await page.getByLabel("来源筛选").fill("sodeck.terminal");
  await page.getByLabel("服务器筛选").fill("test-server");
  await page.getByLabel("结果筛选").selectOption("failed");
  await expect.poll(() => page.evaluate(() => (window as any).__CALL_TEST__.calls.at(-1))).toMatchObject({ pluginId: "sodeck.terminal", serverId: "test-server", status: "failed" });
  await expect(page.getByText("first.call", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "加载更早记录" })).toHaveCount(0);
  await page.getByLabel("方法筛选").fill("slow");
  await expect.poll(() => page.evaluate(() => Boolean((window as any).__CALL_TEST__.releaseSlow))).toBe(true);
  await page.getByLabel("方法筛选").fill("new");
  await expect(page.getByText("filtered.call", { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).__CALL_TEST__.releaseSlow());
  await expect(page.getByText("stale.reply", { exact: true })).toHaveCount(0);
  await expect(page.getByText(/保留起点/)).toBeVisible();
  await page.setViewportSize({ width: 900, height: 640 });
  await page.screenshot({ path: ".qa/ui/call-records-pagination.png" });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
