import { presentationFixture } from "./presentation-fixture";
import { test, expect, type Page } from "@playwright/test";

async function openCalls(page: Page, scenario: "long" | "short" | "empty" | "failure" | "long-text" = "long") {
  await presentationFixture(page);
  await page.addInitScript(scenario => {
    const root = window as any;
    const state = root.__CALL_TEST__ = { calls: [] as any[], completed: [] as any[], holdNext: false, failNext: false, failInitial: scenario === "failure" } as any;
    root.__TAURI_INTERNALS__ = { invoke: async (_: string, { request }: any) => {
      const presentation = root.__PRESENTATION__.handle(request);
      if (presentation) return presentation;
      let result: any = {};
      if (request.method === "servers.list") result = { servers: [] };
      if (request.method === "workbench.snapshot") result = { schemaVersion: 4, windowId: "main", windowRole: "main", workspaceIds: [], workspaces: [], activeWorkspaceId: null, dockSlots: [], views: [], revision: 0, split: "none", splitRatio: .5, pendingOpens: [], recoveryWarnings: [] };
      if (request.method === "workbench.contributions") result = { commands: [], views: [] };
      if (["workbench.modal", "windows.chrome"].includes(request.method)) result = null;
      if (request.method === "calls.list") {
        const p = request.params;
        state.calls.push(p);
        if (p.method === "slow") await new Promise(resolve => { state.releaseSlow = resolve; });
        if (p.cursor && state.holdNext) await new Promise(resolve => { state.releaseNext = resolve; });
        if (state.failInitial || (p.cursor && state.failNext)) {
          state.failInitial = false;
          return { requestId: request.requestId, error: { code: "READ_FAILED", message: "测试读取失败" } };
        }
        const filtered = p.method || p.pluginId || p.serverId || p.status;
        const index = p.cursor ? Number(p.cursor.id.split("-")[1]) : 0;
        const count = scenario === "empty" ? 0 : filtered ? 1 : scenario === "short" ? 1 : index ? 20 : 100;
        const records = Array.from({ length: count }, (_, i) => ({
          id: `${index}-${i}`, started: 1000,
          caller: { pluginId: scenario === "long-text" ? "sodeck." + "long-plugin-name".repeat(10) : "sodeck.terminal" },
          method: p.method === "slow" ? "stale.reply" : filtered ? "filtered.call" : scenario === "long-text" ? "long.method.".repeat(20) : `${index ? "older" : "first"}.call.${i}`,
          status: "succeeded", ...(scenario === "long-text" ? { errorCode: "LONG_ERROR_".repeat(20) } : {}),
        }));
        result = { records, nextCursor: !filtered && count && index < (scenario === "short" ? 2 : 1) ? { started: 1000, id: `page-${index + 1}` } : null, retainedFrom: count ? 500 : null };
        state.completed.push(p);
      }
      return { requestId: request.requestId, result };
    } };
  }, scenario);
  await page.goto("/");
  await page.getByRole("button", { name: "调用记录", exact: true }).click();
}

const content = (page: Page) => page.getByRole("region", { name: "调用记录内容" });
const rows = (page: Page) => page.getByRole("table", { name: "核心调用记录", exact: true }).locator("tbody tr");
const requests = (page: Page) => page.evaluate(() => (window as any).__CALL_TEST__.calls);

test("wheel loads older records once, preserves position and stops at the last page", async ({ page }) => {
  await openCalls(page);
  await expect(rows(page)).toHaveCount(100);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "加载更早记录" })).toHaveCount(0);
  expect((await requests(page))).toHaveLength(1);
  expect(await content(page).evaluate(node => ({ scrollbar: getComputedStyle(node).scrollbarWidth, scrollable: node.scrollHeight > node.clientHeight, noHorizontalOverflow: node.scrollWidth <= node.clientWidth }))).toEqual({ scrollbar: "auto", scrollable: true, noHorizontalOverflow: true });
  const filterTop=await page.getByRole('button',{name:'刷新',exact:true}).evaluate(node=>node.getBoundingClientRect().top);
  expect(await page.locator(".call-table").evaluate(node => getComputedStyle(node).overflow)).toBe("visible");
  await page.evaluate(() => { (window as any).__CALL_TEST__.holdNext = true; });
  await content(page).hover();
  await page.mouse.wheel(0, 100000);
  expect(await page.getByRole('button',{name:'刷新',exact:true}).evaluate(node=>node.getBoundingClientRect().top)).toBe(filterTop);
  await expect(page.getByRole("status").filter({ hasText: "正在读取记录" })).toBeVisible();
  await expect(page.getByRole("button", { name: "刷新", exact: true })).toBeDisabled();
  await expect.poll(() => requests(page)).toHaveLength(2);
  const position = await content(page).evaluate(node => node.scrollTop);
  await page.mouse.wheel(0, -300);
  await page.mouse.wheel(0, 100000);
  expect((await requests(page))).toHaveLength(2);
  expect((await requests(page))[1]).toEqual({ limit: 100, cursor: { started: 1000, id: "page-1" } });
  await page.evaluate(() => { (window as any).__CALL_TEST__.releaseNext(); });
  await expect(rows(page)).toHaveCount(120);
  expect(await content(page).evaluate(node => node.scrollTop)).toBeCloseTo(position, 0);
  expect(await rows(page).locator("code").allTextContents()).toEqual([...Array.from({ length: 100 }, (_, i) => `first.call.${i}`), ...Array.from({ length: 20 }, (_, i) => `older.call.${i}`)]);
  await page.mouse.wheel(0, 100000);
  await expect(page.getByText("已全部加载，共 120 条")).toBeVisible();
  await page.waitForTimeout(200);
  expect((await requests(page))).toHaveLength(2);
});

test("short pages fill automatically", async ({ page }) => {
  await openCalls(page, "short");
  await expect(rows(page)).toHaveCount(3);
  await expect(page.getByText("已全部加载，共 3 条")).toBeVisible();
  expect((await requests(page))).toHaveLength(3);
});

test("empty results are distinct from loading and failure", async ({ page }) => {
  await openCalls(page, "empty");
  await expect(page.getByText("没有符合筛选条件的记录。")).toBeVisible();
  await page.waitForTimeout(200);
  expect((await requests(page))).toHaveLength(1);
});

test("filters and refresh reset scrolling and discard stale initial and pagination replies", async ({ page }) => {
  await openCalls(page);
  await expect(rows(page)).toHaveCount(100);
  await page.evaluate(() => { (window as any).__CALL_TEST__.holdNext = true; });
  await content(page).focus();
  await page.keyboard.press("End");
  await expect.poll(() => page.evaluate(() => Boolean((window as any).__CALL_TEST__.releaseNext))).toBe(true);
  await page.getByLabel("来源筛选").fill("sodeck.terminal");
  await page.getByLabel("服务器筛选").fill("test-server");
  await page.getByLabel("结果筛选").selectOption("failed");
  await expect.poll(async () => (await requests(page)).at(-1)).toEqual({ limit: 100, pluginId: "sodeck.terminal", serverId: "test-server", status: "failed" });
  await expect(rows(page)).toHaveCount(1);
  await page.evaluate(() => { (window as any).__CALL_TEST__.releaseNext(); });
  await expect.poll(() => page.evaluate(() => (window as any).__CALL_TEST__.completed.some((p: any) => p.cursor))).toBe(true);
  await expect(page.getByText("older.call.0", { exact: true })).toHaveCount(0);
  await page.getByLabel("方法筛选").fill("slow");
  await expect.poll(() => page.evaluate(() => Boolean((window as any).__CALL_TEST__.releaseSlow))).toBe(true);
  await page.getByLabel("方法筛选").fill("new");
  await expect(page.getByText("filtered.call", { exact: true })).toBeVisible();
  await page.evaluate(() => { (window as any).__CALL_TEST__.releaseSlow(); });
  await expect.poll(() => page.evaluate(() => (window as any).__CALL_TEST__.completed.some((p: any) => p.method === "slow"))).toBe(true);
  await expect(page.getByText("stale.reply", { exact: true })).toHaveCount(0);
  await page.getByLabel("方法筛选").fill("");
  await page.getByLabel("来源筛选").fill("");
  await page.getByLabel("服务器筛选").fill("");
  await page.getByLabel("结果筛选").selectOption("");
  await expect(rows(page)).toHaveCount(100);
  await content(page).focus();
  await content(page).evaluate(node => {
    (window as any).__CALL_TEST__.scrollEnded = false;
    node.addEventListener("scrollend", () => { (window as any).__CALL_TEST__.scrollEnded = true; }, { once: true });
  });
  await page.keyboard.press("PageDown");
  await expect.poll(() => page.evaluate(() => (window as any).__CALL_TEST__.scrollEnded)).toBe(true);
  await expect.poll(() => content(page).evaluate(node => node.scrollTop)).toBeGreaterThan(0);
  // Activate without scrolling the button into view, so refresh itself must reset position.
  await page.getByRole("button", { name: "刷新", exact: true }).evaluate((button: HTMLButtonElement) => button.click());
  await expect(rows(page)).toHaveCount(100);
  await expect.poll(() => content(page).evaluate(node => node.scrollTop)).toBe(0);
  expect((await requests(page)).at(-1)).toEqual({ limit: 100 });
});

test("failures stop automatic retries, keep existing records and recover through refresh", async ({ page }) => {
  await openCalls(page, "failure");
  await expect(page.getByRole("alert")).toContainText("测试读取失败");
  await expect(page.getByText("没有符合筛选条件的记录。")).toHaveCount(0);
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  await expect(rows(page)).toHaveCount(100);
  await page.evaluate(() => { (window as any).__CALL_TEST__.failNext = true; });
  await content(page).focus();
  await page.keyboard.press("End");
  await expect(page.getByText("较早记录读取失败。")).toBeVisible();
  await expect(rows(page)).toHaveCount(100);
  await content(page).press("Home");
  await content(page).press("End");
  await page.waitForTimeout(200);
  expect((await requests(page))).toHaveLength(3);
  await page.evaluate(() => { (window as any).__CALL_TEST__.failNext = false; });
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  await expect(rows(page)).toHaveCount(100);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await content(page).focus();
  await page.keyboard.press("End");
  await expect(rows(page)).toHaveCount(120);
});

test("append failure retries its cursor without replacing retained rows",async({page})=>{
  await openCalls(page);await expect(rows(page)).toHaveCount(100);
  await page.evaluate(()=>(window as any).__CALL_TEST__.failNext=true);
  await content(page).focus();await page.keyboard.press('End');
  const retry=page.getByRole('button',{name:'重试读取较早记录',exact:true});await expect(retry).toBeVisible();
  await expect(rows(page)).toHaveCount(100);const before=await content(page).evaluate(e=>e.scrollTop);
  await page.evaluate(()=>(window as any).__CALL_TEST__.failNext=false);await retry.click();
  await expect(rows(page)).toHaveCount(120);expect((await requests(page)).slice(1)).toEqual([{limit:100,cursor:{started:1000,id:'page-1'}},{limit:100,cursor:{started:1000,id:'page-1'}}]);
  expect(await content(page).evaluate(e=>e.scrollTop)).toBeGreaterThanOrEqual(before-2);
});

test("closing disconnects pagination and ignores the pending reply after reopening", async ({ page }) => {
  await openCalls(page);
  await expect(rows(page)).toHaveCount(100);
  await page.evaluate(() => { (window as any).__CALL_TEST__.holdNext = true; });
  await content(page).focus();
  await page.keyboard.press("End");
  await expect.poll(() => page.evaluate(() => Boolean((window as any).__CALL_TEST__.releaseNext))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.getByRole("button", { name: "调用记录", exact: true }).click();
  await expect(rows(page)).toHaveCount(100);
  await page.evaluate(() => { (window as any).__CALL_TEST__.releaseNext(); });
  await expect.poll(() => page.evaluate(() => (window as any).__CALL_TEST__.completed.some((p: any) => p.cursor))).toBe(true);
  await expect(rows(page)).toHaveCount(100);
  expect((await requests(page))).toHaveLength(3);
});

test("three themes, narrow windows and keyboard access keep the list readable with a visible scrollbar", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 900, height: 640 });
  await openCalls(page);
  await expect(rows(page)).toHaveCount(100);
  for (const theme of ["dark", "light", "dopamine"]) {
    await page.evaluate(theme => (window as any).__PRESENTATION__.change(theme), theme);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme === "dopamine" ? theme : `soft-${theme}`);
    await page.screenshot({ path: `.qa/ui/call-records-auto-${theme}.png` });
  }
  await page.setViewportSize({ width: 480, height: 640 });
  await page.emulateMedia({ forcedColors: "active" });
  expect(await content(page).evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole("combobox", { name: "结果筛选" }).focus();
  await page.keyboard.press("Tab");
  await expect(content(page)).toBeFocused();
  expect(await content(page).evaluate(node => getComputedStyle(node).outlineStyle)).not.toBe("none");
  expect(await content(page).evaluate(node => Number.parseFloat(getComputedStyle(node).outlineOffset))).toBeLessThan(0);
  await page.keyboard.press("End");
  await expect(rows(page)).toHaveCount(120);
  await page.screenshot({ path: ".qa/ui/call-records-auto-narrow-contrast.png" });
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("long methods, sources and error codes wrap within a narrow dialog", async ({ page }) => {
  await page.setViewportSize({ width: 480, height: 640 });
  await openCalls(page, "long-text");
  await expect(rows(page)).toHaveCount(100);
  expect(await content(page).evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: ".qa/ui/call-records-auto-long-text.png" });
});
