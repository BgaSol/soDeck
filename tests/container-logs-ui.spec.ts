import { test, expect, type Page } from "@playwright/test";
import { logFixture } from "./log-fixture";
async function event(
  page: Page,
  phase: string,
  extra: Record<string, unknown> = {},
) {
  await page.evaluate(
    ({ phase, extra }) => (window as any).__LOG_VIEW__.event(phase, extra),
    { phase, extra },
  );
}
async function append(page: Page, text = "new output") {
  await page.evaluate(
    (text) => (window as any).__LOG_VIEW__.append(text),
    text,
  );
}
const scroller = (page: Page) =>
  page.getByLabel("容器日志内容", { exact: true });
async function top(page: Page) {
  return scroller(page).evaluate((el) => el.scrollTop);
}

test("clicking text keeps following; restart resumes and manual pause preserves the viewport", async ({
  page,
}) => {
  await logFixture(page);
  await expect(page.locator(".log-status")).toContainText("实时接收");
  await expect(page.locator('[data-seq="1200"]')).toBeVisible();
  await page.locator('[data-seq="1200"] .log-row-body').click();
  await expect(
    page.getByRole("button", { name: "暂停滚动", exact: true }),
  ).toBeVisible();
  await event(page, "waitingContainer");
  await append(page, "after restart");
  await event(page, "streaming");
  await expect(page.locator('[data-seq="1201"]')).toBeVisible();
  await page.getByRole("button", { name: "暂停滚动", exact: true }).click();
  const before = await top(page);
  await event(page, "reconnecting", {
    retryAt: Date.now() + 8000,
    error: "连接中断",
  });
  await append(page, "paused incoming");
  await event(page, "streaming");
  await expect(
    page.getByRole("button", { name: /继续跟随 · 新增 1 条/ }),
  ).toBeVisible();
  expect(Math.abs((await top(page)) - before)).toBeLessThan(3);
  await page.getByRole("button", { name: /继续跟随/ }).click();
  await expect(page.locator('[data-seq="1202"]')).toBeVisible();
});

test("browsing pauses, reaching the bottom resumes only browsing pauses", async ({
  page,
}) => {
  await logFixture(page);
  await expect(page.locator('[data-seq="1200"]')).toBeVisible();
  await scroller(page).hover();
  await page.mouse.wheel(0, -350);
  await expect(page.locator(".log-reading-status")).toContainText("已暂停滚动");
  await append(page);
  await expect(page.getByRole("button", { name: /继续跟随/ })).toBeVisible();
  await scroller(page).focus();
  await page.keyboard.press("End");
  await expect(
    page.getByRole("button", { name: "暂停滚动", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "暂停滚动", exact: true }).click();
  await scroller(page).focus();
  await page.keyboard.press("End");
  await expect(page.getByRole("button", { name: /继续跟随/ })).toBeVisible();
});

test("search stays visible as chips, snapshots stay stable, returning live preserves the keyword", async ({
  page,
}) => {
  await logFixture(page);
  await page.getByRole("button", { name: "查找", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "暂停滚动", exact: true }),
  ).toBeVisible();
  await page.getByLabel("搜索全部本地日志").fill("timeout");
  await expect(page.locator(".log-reading-status")).toContainText("快照截至");
  await expect(page.locator(".log-row")).not.toHaveCount(0);
  await page.getByRole("button", { name: "收起查找" }).click();
  await expect(
    page.getByRole("button", { name: "移除关键词筛选" }),
  ).toBeVisible();
  await append(page, "timeout just arrived");
  await expect(
    page.getByRole("button", { name: "有新日志 · 更新结果" }),
  ).toBeVisible();
  await expect(page.locator('[data-seq="1201"]')).toHaveCount(0);
  await page.getByRole("button", { name: "实时筛选并跟随" }).click();
  await expect(
    page.getByRole("button", { name: "移除关键词筛选" }),
  ).toBeVisible();
  await expect(page.locator('[data-seq="1201"]')).toBeVisible();
  await append(page, "timeout next");
  await expect(page.locator('[data-seq="1202"]')).toBeVisible();
});

test("context returns to the previous query and export uses the whole snapshot", async ({
  page,
}) => {
  await logFixture(page);
  await page.getByRole("button", { name: "查找", exact: true }).click();
  await page.getByLabel("搜索全部本地日志").fill("timeout");
  await expect(page.locator(".log-reading-status")).toContainText("快照截至");
  await page.locator(".log-context").last().click();
  await expect(
    page.getByRole("button", { name: "返回搜索结果" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "返回搜索结果" }).click();
  await expect(page.getByRole("button", { name: "返回搜索结果" })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("button", { name: "移除关键词筛选" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "导出", exact: true }).click();
  await page.getByRole("button", { name: "当前筛选结果", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).__LOG_VIEW__.data.exports.length),
    )
    .toBe(1);
  expect(
    await page.evaluate(() => (window as any).__LOG_VIEW__.data.exports[0]),
  ).toMatchObject({
    type: "query",
    query: { keyword: "timeout" },
    snapshotId: expect.any(String),
  });
});

test("live filters advance through no-match rows and refresh changes to existing records", async ({
  page,
}) => {
  await logFixture(page);
  await page.getByRole("button", { name: "查找", exact: true }).click();
  await page.getByLabel("实时筛选", { exact: true }).check();
  await page.getByLabel("搜索全部本地日志").fill("needle");
  await expect(
    page.getByText("没有匹配的已保存日志", { exact: true }),
  ).toBeVisible();
  await append(page, "does not match");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__LOG_VIEW__.data.calls
            .filter((c: any) => c.command === "logs_page")
            .at(-1)?.args.query.after,
      ),
    )
    .toBe(1200);
  await append(page, "needle");
  await expect(page.locator('[data-seq="1202"]')).toBeVisible();
  await page.evaluate(() =>
    (window as any).__LOG_VIEW__.update(1202, "changed without the keyword"),
  );
  await expect(page.locator('[data-seq="1202"]')).toHaveCount(0);
});

test("compact dark layout, keyboard popovers, and display preferences", async ({
  page,
}) => {
  await page.setViewportSize({ width: 650, height: 400 });
  await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "dark" });
  await logFixture(page);
  await expect(
    page.getByRole("button", { name: "暂停滚动", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "显示", exact: true }).click();
  await page.getByLabel("自动换行", { exact: true }).check();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "显示", exact: true }),
  ).toBeFocused();
  await expect(page.locator(".log-viewer")).toHaveClass(/log-wrap/);
  await page.screenshot({ path: ".qa/container-logs-redesign-compact.png" });
  await page.reload();
  await expect(page.locator(".log-viewer")).toHaveClass(/log-wrap/);
});

test("offline archives do not start a collector and missing containers retain saved logs", async ({
  page,
}) => {
  await logFixture(page, { offline: true });
  await expect(page.locator(".log-status")).toContainText("本地存档");
  expect(
    await page.evaluate(
      () =>
        (window as any).__LOG_VIEW__.data.calls.filter(
          (c: any) => c.command === "logs_start",
        ).length,
    ),
  ).toBe(0);
  await logFixture(page);
  await expect(page.locator(".log-status")).toContainText("实时接收");
  await event(page, "failed", {
    reason: "containerMissing",
    error: "No such container",
  });
  await expect(page.locator(".log-status")).toContainText("原容器已不存在");
  await expect(page.locator(".log-row")).not.toHaveCount(0);
  await page.getByRole("button", { name: "查找新容器" }).click();
  await expect(page.getByText(/未发现身份匹配的替代容器/)).toBeVisible();
});

test("selection clears when changing the query and an idle producer still drains a large backlog", async ({
  page,
}) => {
  await logFixture(page);
  await expect(page.locator('[data-seq="1200"]')).toBeVisible();
  await page.locator('[data-seq="1200"] .log-seq').click();
  await expect(
    page.getByRole("button", { name: "复制原文", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "查找", exact: true }).click();
  await page.getByLabel("搜索全部本地日志").fill("timeout");
  await expect(page.locator(".log-reading-status")).toContainText("快照截至");
  await expect(
    page.getByRole("button", { name: "复制原文", exact: true }),
  ).toHaveCount(0);
  await expect(page.locator('[data-seq="1200"]')).toHaveCount(0);
  await page.getByRole("button", { name: "清除条件", exact: true }).click();
  await page.getByRole("button", { name: /继续跟随/ }).click();
  await page.evaluate(() => {
    for (let i = 0; i < 1200; i++) (window as any).__LOG_VIEW__.append("burst");
  });
  await expect(page.locator('[data-seq="2400"]')).toBeVisible();
});

test("200 percent text and high contrast keep the main reading control reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 650, height: 400 });
  await logFixture(page);
  await page.evaluate(() => (document.documentElement.style.fontSize = "28px"));
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });
  await expect(
    page.getByRole("button", { name: "暂停滚动", exact: true }),
  ).toBeVisible();
  const box = await page
    .getByRole("button", { name: "暂停滚动", exact: true })
    .boundingBox();
  expect(box!.y + box!.height).toBeLessThanOrEqual(400);
  const heading = await page.locator(".window-drag-region h1").boundingBox();
  const subtitle = await page.locator(".window-drag-region p").boundingBox();
  expect(heading!.y + heading!.height).toBeLessThanOrEqual(subtitle!.y + 1);
  await page.screenshot({ path: ".qa/container-logs-redesign-accessible.png" });
});

test("failed snapshots keep old results explicit and cannot export them as the new query", async ({
  page,
}) => {
  await logFixture(page);
  await expect(page.locator('[data-seq="1200"]')).toBeVisible();
  await page.evaluate(() => {
    (window as any).__LOG_VIEW__.data.failSnapshot = true;
  });
  await page.getByRole("button", { name: "查找", exact: true }).click();
  await page.getByLabel("搜索全部本地日志").fill("timeout");
  await expect(page.getByRole("alert")).toContainText("快照空间不足");
  await expect(page.locator(".log-reading-status")).toContainText(
    "当前条件尚未应用",
  );
  await expect(page.locator('[data-seq="1200"]')).toBeVisible();
  await page.getByRole("button", { name: "导出", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "当前筛选结果", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "全部已保存", exact: true }),
  ).toBeEnabled();
  await page.keyboard.press("Escape");
  await page.evaluate(() => {
    (window as any).__LOG_VIEW__.data.failSnapshot = false;
  });
  await page.getByRole("button", { name: /更新结果/ }).click();
  await expect(page.locator(".log-reading-status")).toContainText("快照截至");
  await expect(page.locator('[data-seq="1200"]')).toHaveCount(0);
  await page.getByRole("button", { name: "导出", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "当前筛选结果", exact: true }),
  ).toBeEnabled();
});
