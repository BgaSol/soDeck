import { expect, test, type Page } from "@playwright/test";

import { setup } from "./terminal-fixture";

test("history preserves terminal instance, geometry and input ownership", async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 580 });
  const errors = await setup(page);
  await page.goto("/?container=1");
  await expect(page.getByText("正在保存历史", { exact: true })).toBeVisible();
  await page.locator(".xterm-helper-textarea").fill("abc");
  const before = await page.evaluate(() => {
    const state = (window as any).__HISTORY_TEST__;
    (window as any).__terminalNode =
      document.querySelector(".terminal-surface");
    return state.calls.filter((c: any) => c.command === "terminal_resize")
      .length;
  });
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await expect(page.getByLabel("搜索终端历史")).toBeVisible();
  await page.getByLabel("搜索终端历史").fill("中文");
  await page.evaluate(() =>
    (window as any).__HISTORY_TEST__.emit({
      type: "data",
      data: btoa("background\r\n"),
    }),
  );
  await expect(page.getByText("实时终端有新输出")).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        document.querySelector(".terminal-surface") ===
        (window as any).__terminalNode,
    ),
  ).toBe(true);
  expect(
    await page.evaluate(
      () =>
        (window as any).__HISTORY_TEST__.calls.filter(
          (c: any) => c.command === "terminal_resize",
        ).length,
    ),
  ).toBe(before);
  expect(
    await page.evaluate(
      () =>
        (window as any).__HISTORY_TEST__.calls.filter(
          (c: any) => c.command === "close_connection",
        ).length,
    ),
  ).toBe(0);
  expect(
    await page.evaluate(() =>
      (window as any).__HISTORY_TEST__.calls
        .filter((c: any) => c.command === "terminal_input")
        .some((c: any) => c.args.data.includes("中文")),
    ),
  ).toBe(false);
  await page.getByRole("button", { name: "返回实时终端" }).click();
  await expect(page.locator(".xterm-helper-textarea")).toBeFocused();
  await page.getByRole("button", { name: "清理显示", exact: true }).click();
  await expect(page.getByText("已清理 · 查看此前记录")).toBeVisible();
  expect(errors).toEqual([]);
});
test("offline global search reaches old records and opens bounded context", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?terminal-history=1");
  await expect(
    page.getByRole("heading", { name: "终端历史", exact: true }),
  ).toBeVisible();
  await page.getByLabel("搜索终端历史").fill("中文");
  await page.getByRole("button", { name: "搜索", exact: true }).click();
  await expect(page.locator(".history-search-hit")).toHaveCount(2);
  await expect(page.locator(".history-search-hit").last()).toContainText(
    "预发布服务器",
  );
  await page
    .getByRole("button", { name: "查看前后记录", exact: true })
    .first()
    .click();
  await expect(page.locator('[data-history-id="15"]')).toBeVisible();
  await expect(page.getByLabel("历史会话", { exact: true })).toHaveValue(
    "container-1",
  );
  await page.getByRole("button", { name: "查看当时屏幕", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "历史屏幕", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("已恢复选中记录时的屏幕")).toBeVisible();
  expect(
    await page.evaluate(() =>
      (window as any).__HISTORY_TEST__.calls.some(
        (c: any) => c.command === "terminal_input",
      ),
    ),
  ).toBe(false);
  await page.getByRole("button", { name: "返回记录", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "查看当时屏幕", exact: true }),
  ).toBeFocused();
  await expect(page.locator('[data-history-id="15"]')).toBeVisible();
  await page.getByRole("button", { name: "导出", exact: true }).click();
  await page.getByRole("button", { name: "导出原始记录" }).click();
  await expect(
    page.getByText("已导出到 C:\\exports\\history.txt"),
  ).toBeVisible();
  expect(errors).toEqual([]);
});
test("virtualized history pages past the old 5000-line limit and shows failures", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?terminal-history=1");
  await expect(page.locator(".terminal-history-row").first()).toBeVisible();
  for (let n = 0; n < 13; n++) {
    await page.getByRole("button", { name: "更早记录" }).click();
    await expect(page.getByRole("button", { name: "更早记录" })).toBeEnabled();
  }
  const before = await page.evaluate(() =>
    Math.min(
      ...(window as any).__HISTORY_TEST__.calls
        .filter(
          (c: any) =>
            c.command === "terminal_history_page" && c.args.query.before,
        )
        .map((c: any) => Number(c.args.query.before)),
    ),
  );
  expect(before).toBeLessThan(7000);
  expect(await page.locator(".terminal-history-row").count()).toBeLessThan(100);
  await page.evaluate(() => {
    (window as any).__HISTORY_TEST__.fail = true;
  });
  await page.getByRole("button", { name: "更早记录" }).click();
  await expect(page.getByRole("alert")).toContainText("历史文件暂时无法读取");
  await page.setViewportSize({ width: 680, height: 540 });
  await page.screenshot({ path: ".qa/terminal-history-narrow.png" });
  expect(errors).toEqual([]);
});

test("clearing during full screen is disabled and history restores its reading position", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?container=1");
  const emit = async (data: string) =>
    page.evaluate(
      (data) =>
        (window as any).__HISTORY_TEST__.emit({
          type: "data",
          data: btoa(data),
        }),
      data,
    );
  await expect(page.getByText("正在保存历史", { exact: true })).toBeVisible();
  await emit("\x1b[?1049hfullscreen");
  await expect(
    page.getByRole("button", { name: "清理显示", exact: true }),
  ).toBeDisabled();
  await emit("\x1b[?1049l");
  await expect(
    page.getByRole("button", { name: "清理显示", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await page.getByRole("button", { name: "更早记录" }).click();
  await expect(page.getByRole("button", { name: "更早记录" })).toBeEnabled();
  const rowId = await page
    .locator("[data-history-id]")
    .first()
    .getAttribute("data-history-id");
  await page.getByRole("button", { name: "返回实时终端" }).click();
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await expect(page.locator(`[data-history-id="${rowId}"]`)).toBeVisible();
  expect(errors).toEqual([]);
});

test("session navigation, scoped search and range actions keep the correct source", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          (window as any).__HISTORY_COPIED__ = text;
        },
      },
    });
  });
  await page.goto("/?terminal-history=1");
  const sidebar = page.getByRole("complementary", { name: "历史会话导航" });
  await expect(sidebar.getByRole("button", { pressed: true })).toContainText(
    "api-service",
  );
  await expect(page.locator(".terminal-history-row").last()).toBeVisible();
  await page.screenshot({ path: ".qa/terminal-history-redesign-wide.png" });
  await sidebar.getByRole("button", { name: /预发布服务器/ }).click();
  await expect(page.locator(".history-session-identity h2")).toHaveText(
    "预发布服务器",
  );
  await expect(page.locator('[data-history-id="100"]')).toBeVisible();
  await page.getByLabel("搜索终端历史").fill("中文");
  await page
    .getByRole("button", { name: "此服务器 / 容器", exact: true })
    .click();
  await expect(page.getByLabel("搜索终端历史")).toHaveValue("中文");
  await page.getByRole("button", { name: "搜索", exact: true }).click();
  await expect(page.locator(".history-search-hit")).toHaveCount(1);
  expect(
    await page.evaluate(
      () =>
        (window as any).__HISTORY_TEST__.calls
          .filter((call: any) => call.command === "terminal_history_search")
          .at(-1).args.query.sourceId,
    ),
  ).toBe("source-server");
  await page.getByRole("button", { name: "全部终端", exact: true }).click();
  await expect(page.locator(".history-search-hit")).toHaveCount(2);
  await page
    .getByRole("button", { name: "查看前后记录", exact: true })
    .first()
    .click();
  await expect(sidebar.getByRole("button", { pressed: true })).toContainText(
    "api-service",
  );
  await page.locator('[data-history-id="13"] .history-select-record').click();
  await page
    .locator('[data-history-id="12"] .history-select-record')
    .click({ modifiers: ["Shift"] });
  await page.getByRole("button", { name: "复制范围", exact: true }).click();
  expect(await page.evaluate(() => (window as any).__HISTORY_COPIED__)).toBe(
    "copied 12-13",
  );
  await page.getByRole("button", { name: "导出范围", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__HISTORY_TEST__.calls
          .filter((call: any) => call.command === "terminal_history_export")
          .at(-1).args,
    ),
  ).toMatchObject({
    sessionId: "container-1",
    from: "12",
    to: "13",
    raw: false,
  });
  const exportButton = page.getByRole("button", { name: "导出", exact: true });
  await exportButton.click();
  await page.getByRole("button", { name: "导出原始记录" }).focus();
  await page.keyboard.press("Escape");
  await expect(exportButton).toBeFocused();
  await expect(page.getByRole("group", { name: "导出格式" })).not.toBeVisible();
  expect(errors).toEqual([]);
});

test("narrow history retains sessions, timestamps and accessible filters in both themes", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 740 });
  const errors = await setup(page);
  await page.goto("/?terminal-history=1");
  const picker = page.getByLabel("历史会话", { exact: true });
  await expect(picker).toBeVisible();
  await expect(
    page.getByRole("complementary", { name: "历史会话导航" }),
  ).not.toBeVisible();
  await picker.selectOption("server-1");
  await expect(page.locator('[data-history-id="100"] time')).toBeVisible();
  await page.getByRole("button", { name: "筛选", exact: true }).click();
  await page.getByLabel("区分大小写", { exact: true }).check();
  await page.getByLabel("开始", { exact: true }).fill("2026-10-16T00:00");
  await page.getByLabel("结束", { exact: true }).fill("2026-10-15T00:00");
  await expect(page.getByRole("alert")).toContainText(
    "开始时间不能晚于结束时间",
  );
  expect(
    await page.evaluate(() =>
      (window as any).__HISTORY_TEST__.calls
        .filter((call: any) => call.command === "terminal_history_page")
        .every(
          (call: any) =>
            !call.args.query.to ||
            !call.args.query.from ||
            call.args.query.from <= call.args.query.to,
        ),
    ),
  ).toBe(true);
  await page.getByRole("button", { name: "清除筛选" }).click();
  await expect(page.getByRole("alert")).not.toBeVisible();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "筛选", exact: true }),
  ).toBeFocused();
  const assertFits = async () => {
    const dimensions = await page.evaluate(() => ({
      width: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      listHeight: document
        .querySelector(".history-list")!
        .getBoundingClientRect().height,
    }));
    expect(dimensions.scrollWidth).toBeLessThanOrEqual(dimensions.width);
    expect(dimensions.listHeight).toBeGreaterThan(100);
    await expect(page.locator('[data-history-id="100"] time')).toBeVisible();
  };
  await assertFits();
  await page.screenshot({ path: ".qa/terminal-history-redesign-narrow.png" });
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await assertFits();
  await page.screenshot({ path: ".qa/terminal-history-redesign-dark.png" });
  expect(errors).toEqual([]);
});
