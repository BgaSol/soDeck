import { expect, test, type Page } from "@playwright/test";
import type { LogArchive } from "../src/logApi";

const archives: LogArchive[] = [
  {
    sourceId: "a".repeat(64),
    serverId: "production",
    serverName: "生产服务器",
    host: "10.20.0.12",
    port: 22,
    containerName: "api-gateway",
    containerId: "a18c2d4e6f90".repeat(6),
    endpoint: "unix:///var/run/docker.sock",
    tty: false,
    records: 1284096,
    bytes: 248 * 1024 ** 2,
    lastSeq: 1284096,
    lastTimestamp: "2026-10-15T06:32:18Z",
    runs: 3,
    interruptedRuns: 0,
  },
  {
    sourceId: "b".repeat(64),
    serverId: "staging",
    serverName: "预发布环境",
    host: "staging.internal.example",
    port: 2222,
    containerName: "order-service-with-a-very-long-container-name",
    containerId: "b84d2e9f1a30".repeat(6),
    endpoint: "unix:///var/run/docker.sock",
    tty: false,
    records: 39280,
    bytes: 18.4 * 1024 ** 2,
    lastSeq: 39280,
    lastTimestamp: "2026-10-14T03:08:42Z",
    runs: 2,
    interruptedRuns: 0,
  },
  {
    sourceId: "c".repeat(64),
    serverId: "production",
    serverName: "生产服务器",
    host: "10.20.0.12",
    port: 22,
    containerName: "redis-cache",
    containerId: "c9e1a2d3b4f5".repeat(6),
    endpoint: "unix:///var/run/docker.sock",
    tty: false,
    records: 0,
    bytes: 4096,
    lastSeq: 0,
    lastTimestamp: null,
    runs: 1,
    interruptedRuns: 0,
  },
];

// UI tests simulate IPC; features.spec.ts checks actual native window creation.
async function setup(
  page: Page,
  options: { empty?: boolean; fail?: boolean } = {},
) {
  await page.addInitScript(
    ({ archives, options }) => {
      const w = window as any;
      const state = {
        archives: options.empty ? [] : archives,
        fail: options.fail ?? false,
        failOpen: false,
        calls: [] as { command: string; args: any }[],
      };
      w.__LOG_TEST__ = state;
      w.isTauri = true;
      let serial = 0;
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback: () => ++serial,
        unregisterCallback: () => {},
        invoke: async (command: string, args: any = {}) => {
          state.calls.push({ command, args });
          if (command === "list_log_archives") {
            if (state.fail) throw { message: "存档目录暂时无法读取" };
            return structuredClone(state.archives);
          }
          if (command === "open_log_archive" && state.failOpen)
            throw { message: "存档文件暂时无法打开" };
          if (command === "list_servers" || command === "list_transfers")
            return [];
          if (command === "plugin:event|listen") return ++serial;
          return null;
        },
      };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    },
    { archives, options },
  );
}

test("main entry requests a native window without showing a modal", async ({
  page,
}) => {
  await setup(page);
  await page.goto("/");
  await page.getByRole("button", { name: "本地日志", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).__LOG_TEST__.calls.some(
          (c: any) => c.command === "open_log_archives_window",
        ),
      ),
    )
    .toBe(true);
  await expect(page.getByRole("dialog", { name: "本地日志存档" })).toHaveCount(
    0,
  );
});

test("standalone list searches, sorts and opens the selected archive", async ({
  page,
}) => {
  await setup(page);
  await page.goto("/?archives=1");
  await expect(
    page.getByRole("heading", { name: "本地日志", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".archive-row")).toHaveCount(3);
  await expect(page.locator(".archive-row").first()).toContainText(
    "api-gateway",
  );
  await page.getByLabel("筛选日志存档").fill(" STAGING ");
  await expect(page.locator(".archive-row")).toHaveCount(1);
  await page.locator(".archive-row").press("Enter");
  expect(
    await page.evaluate(
      () =>
        (window as any).__LOG_TEST__.calls.find(
          (c: any) => c.command === "open_log_archive",
        ).args.sourceId,
    ),
  ).toBe(archives[1].sourceId);
  await page.getByLabel("筛选日志存档").fill("does-not-exist");
  await expect(
    page.getByText("没有找到匹配的存档", { exact: true }),
  ).toBeVisible();
  await page
    .locator(".archive-empty")
    .getByRole("button", { name: "清除搜索" })
    .click();
  await expect(page.getByLabel("筛选日志存档")).toBeFocused();
  await page.getByLabel("日志存档排序").selectOption("size");
  await expect(page.locator(".archive-row").first()).toContainText(
    "api-gateway",
  );
  await page.getByLabel("日志存档排序").selectOption("name");
  await expect(page.locator(".archive-row").nth(1)).toContainText(
    "redis-cache",
  );
  await page.getByLabel("日志存档排序").selectOption("recent");
  await page.getByLabel("筛选日志存档").fill("a18c2d4e6f90");
  await expect(page.locator(".archive-row")).toHaveCount(1);
  await page.getByLabel("筛选日志存档").press("Escape");
  await expect(page.locator(".archive-row")).toHaveCount(3);
});

test("loading errors can retry and refresh failures preserve the list", async ({
  page,
}) => {
  await setup(page, { fail: true });
  await page.goto("/?archives=1");
  await expect(page.getByRole("alert")).toContainText("存档目录暂时无法读取");
  await expect(page.getByText("还没有本地日志", { exact: true })).toHaveCount(
    0,
  );
  await page.evaluate(() => {
    (window as any).__LOG_TEST__.fail = false;
  });
  await page.getByRole("button", { name: "重新读取" }).click();
  await expect(page.locator(".archive-row")).toHaveCount(3);
  await page.evaluate(() => {
    (window as any).__LOG_TEST__.fail = true;
  });
  await page.getByLabel("刷新日志存档").click();
  await expect(page.getByRole("alert")).toContainText("存档目录暂时无法读取");
  await expect(page.locator(".archive-row")).toHaveCount(3);
  await page.evaluate(() => {
    (window as any).__LOG_TEST__.fail = false;
    (window as any).__LOG_TEST__.failOpen = true;
  });
  await page.locator(".archive-row").first().click();
  await expect(page.getByRole("alert")).toContainText("存档文件暂时无法打开");
  await expect(page.locator(".archive-row").first()).toBeEnabled();
});

test("empty archives explain how to collect logs", async ({ page }) => {
  await setup(page, { empty: true });
  await page.goto("/?archives=1");
  await expect(page.getByText("还没有本地日志", { exact: true })).toBeVisible();
  await expect(page.locator(".archive-empty")).toContainText("Docker");
  await page.screenshot({ path: ".qa/log-archives-empty.png" });
});

test("light, dark and minimum window layouts keep rows and controls visible", async ({
  page,
}) => {
  await setup(page);
  await page.goto("/?archives=1");
  await expect(page.locator(".archive-row")).toHaveCount(3);
  await page
    .getByLabel("筛选日志存档")
    .evaluate((input: HTMLInputElement) => input.blur());
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await page.screenshot({
      path: `.qa/log-archives-${colorScheme}.png`,
      animations: "disabled",
    });
  }
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await page.setViewportSize({ width: 720, height: 460 });
  expect(
    await page
      .locator(".archive-list-panel")
      .evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await expect(page.locator(".archive-row").last()).toBeInViewport();
  await expect(page.getByLabel("刷新日志存档")).toBeInViewport();
  await page.screenshot({ path: ".qa/log-archives-small.png" });
});
