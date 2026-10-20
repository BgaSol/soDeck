import { test, expect, type Page } from "@playwright/test";

// Simulated IPC for presentation/interaction; the Rust tests exercise parsing and SSH separately.
import { setup, row } from "./resource-fixture";

test("mount summaries, nested mounts, details and keyboard preserve tree behavior", async ({
  page,
}) => {
  const errors = await setup(page);
  const data = row(page, "/data");
  await expect(data.locator(".mount-source")).toHaveText("/dev/sdb1");
  await expect(row(page, "/").locator(".mount-capacity-total")).toHaveText(
    "1 TiB",
  );
  await expect(data.locator(".mount-capacity-total")).toHaveText("1 TiB");
  await expect(data.locator(".mount-capacity-percent")).toHaveText("68%");
  const capacityTotals = [
    [row(page, "/").locator(".mount-capacity-total"), "39.3 GiB"],
    [data.locator(".mount-capacity-total"), "925.2 MiB"],
  ] as const;
  for (const [total, text] of capacityTotals) {
    await total.evaluate((element, value) => {
      element.textContent = value;
    }, text);
    expect(
      await total.evaluate(
        (element) => element.scrollWidth <= element.clientWidth,
      ),
    ).toBe(true);
  }
  await page.screenshot({ path: ".qa/mount-summary-capacity.png" });
  await expect(data.locator(".mount-usage")).toHaveAttribute(
    "aria-label",
    /总容量 1 TiB，已用 696\.3 GiB，可用 327\.7 GiB，使用率 68%/,
  );
  expect(
    (await data.locator(".mount-meter").boundingBox())!.width,
  ).toBeGreaterThan(20);
  await expect(row(page, "/link").locator(".mount-usage")).toHaveCount(0);
  await data.locator(".mount-usage").click();
  const detail = page.getByRole("dialog", { name: "挂载详情" });
  await expect(detail).toContainText("/data");
  await expect(detail).toContainText("总计 1 TiB");
  await expect(data).toHaveAttribute("aria-expanded", "false");
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await expect(data).toBeFocused();
  await data.press("ArrowRight");
  await expect(
    row(page, "/data/archive").locator(".mount-usage"),
  ).toContainText("96%");
  await expect(row(page, "/data/apps").locator(".mount-usage")).toHaveCount(0);
  await expect(row(page, "/data/archive")).toHaveClass(/mount-critical/);
  await row(page, "/data/archive").click({ button: "right" });
  await page.getByRole("menuitem", { name: "查看挂载详情" }).click();
  await expect(detail).toContainText("/dev/sdc1[/archive]");
  await expect(detail).toContainText("空间即将用尽");
  await page.screenshot({ path: ".qa/mount-details.png" });
  await detail.getByRole("button", { name: "关闭挂载详情" }).click();
  await row(page, "/backup").locator(".mount-source").click();
  await expect(detail).toContainText("网络文件系统");
  await expect(detail).toContainText("只读");
  await page.keyboard.press("Escape");
  await row(page, "/proc").locator(".mount-usage").click();
  await expect(detail).toContainText("此文件系统不提供常规容量");
  await expect(row(page, "/proc").locator(".mount-capacity-total")).toHaveCount(
    0,
  );
  await expect(row(page, "/proc").locator(".mount-capacity-status")).toHaveText(
    "—",
  );
  expect(errors).toEqual([]);
});

test("copies the right-clicked absolute path without modifying remote clipboard", async ({
  page,
}) => {
  const errors = await setup(page);
  for (const path of ["/data", "/中文 '文件.txt", "/link", "/"]) {
    await row(page, path).click({ button: "right" });
    await page
      .getByRole("menuitem", { name: "复制绝对路径", exact: true })
      .click();
    await expect(
      page.getByRole("status").filter({ hasText: "已复制绝对路径" }),
    ).toBeVisible();
    expect(await page.evaluate(() => (window as any).__MOUNTS__.copied)).toBe(
      path,
    );
  }
  await row(page, "/data").locator(".file-name").click();
  await row(page, "/backup")
    .locator(".file-name")
    .click({ modifiers: ["ControlOrMeta"] });
  await expect(row(page, "/data")).toHaveAttribute("aria-selected", "true");
  await expect(row(page, "/backup")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("menu", { name: "文件操作" })).toHaveCount(0);
  await row(page, "/data").click({ button: "right" });
  await page
    .getByRole("menuitem", { name: "复制绝对路径", exact: true })
    .click();
  expect(await page.evaluate(() => (window as any).__MOUNTS__.copied)).toBe(
    "/data",
  );
  const tree = page.getByRole("tree", { name: "远程文件树" });
  const bounds = await tree.boundingBox();
  await tree.click({
    button: "right",
    position: { x: 10, y: bounds!.height - 10 },
  });
  await page.getByRole("menuitem", { name: "复制当前目录绝对路径" }).click();
  expect(await page.evaluate(() => (window as any).__MOUNTS__.copied)).toBe(
    "/data",
  );
  expect(
    await page.evaluate(() =>
      (window as any).__MOUNTS__.calls.filter(
        (c: any) =>
          c.command === "set_remote_clipboard" ||
          c.command === "clear_remote_clipboard",
      ),
    ),
  ).toEqual([]);
  await page.evaluate(() => {
    (window as any).__MOUNTS__.failCopy = true;
  });
  await row(page, "/data").click({ button: "right" });
  await page
    .getByRole("menuitem", { name: "复制绝对路径", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText("复制绝对路径失败");
  expect(errors).toEqual([]);
});

test("failed capacity refresh keeps old data and does not prevent directory expansion", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    (window as any).__MOUNTS__.failUsage = true;
  });
  await page.getByRole("button", { name: "刷新目录", exact: true }).click();
  await expect(row(page, "/data").locator(".mount-capacity-total")).toHaveText(
    "1 TiB",
  );
  await expect(row(page, "/data").locator(".mount-usage")).toContainText(
    "68% · 上次",
  );
  await row(page, "/data").locator(".mount-usage").click();
  await expect(page.getByRole("dialog", { name: "挂载详情" })).toContainText(
    "网络挂载查询超时",
  );
  await page.keyboard.press("Escape");
  await row(page, "/data").press("ArrowRight");
  await expect(row(page, "/data/apps")).toBeVisible();
  await page.evaluate(() => {
    (window as any).__MOUNTS__.failUsage = false;
    (window as any).__MOUNTS__.table = {
      version: "v2",
      status: "ready",
      mounts: [],
    };
  });
  await page.getByRole("button", { name: "刷新目录", exact: true }).click();
  await expect(row(page, "/data").locator(".mount-usage")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("stacked mount issues are scoped to directories visible in the file tree", async ({
  page,
}) => {
  const errors = await setup(page);
  const notice = page.locator(".mount-panel-notice");
  await page.evaluate(() => {
    const fixture = (window as any).__MOUNTS__;
    fixture.table = {
      ...fixture.table,
      version: "unrelated-stack",
      issues: [
        {
          kind: "ambiguousStack",
          target: "/proc/sys/fs/binfmt_misc",
          layers: 2,
        },
      ],
    };
    fixture.push();
  });
  await expect(notice).toHaveCount(0);
  await page.evaluate(() => {
    const fixture = (window as any).__MOUNTS__;
    fixture.table = {
      ...fixture.table,
      version: "visible-stack",
      mounts: fixture.table.mounts.filter((m: any) => m.target !== "/data"),
      issues: [{ kind: "ambiguousStack", target: "/data", layers: 2 }],
    };
    fixture.push();
  });
  await expect(notice).toContainText(
    "无法确认 /data 当前可见的挂载层，暂不展示该路径容量",
  );
  await expect(row(page, "/data").locator(".mount-usage")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("mount details fit narrow dark windows and share backend updates without polling", async ({
  page,
}) => {
  await page.clock.install();
  await page.emulateMedia({ colorScheme: "dark" });
  const errors = await setup(page);
  await page.setViewportSize({ width: 850, height: 600 });
  await page
    .getByRole("navigation", { name: "工作区域" })
    .getByRole("button", { name: "文件", exact: true })
    .click();
  const resizer = page.getByRole("separator", { name: "调整文件区域宽度" });
  // Start above the wide capacity threshold; the current minimum pane is 280px.
  await resizer.focus();
  for (let i = 0; i < 12; i++) await resizer.press("ArrowRight");
  const normalUsageWidth = (await row(page, "/backup")
    .locator(".mount-usage")
    .boundingBox())!.width;
  await resizer.focus();
  for (let i = 0; i < 14; i++) await resizer.press("ArrowLeft");
  const backup = row(page, "/backup");
  expect(
    (await backup.locator(".mount-usage").boundingBox())!.width,
  ).toBeLessThan(normalUsageWidth);
  expect(
    (await backup.locator(".file-name").boundingBox())!.width,
  ).toBeGreaterThan(20);
  await expect(row(page, "/").locator(".mount-capacity-total")).toHaveText(
    "1 TiB",
  );
  await page.screenshot({ path: ".qa/mount-summary-dark-narrow.png" });
  await backup.locator(".mount-usage").click();
  const detail = page.getByRole("dialog", { name: "挂载详情" });
  await expect(detail).toBeVisible();
  const bounds = (await detail.boundingBox())!;
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.y).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(850);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(600);
  const tree = page.getByRole("tree", { name: "远程文件树" });
  expect(await tree.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(
    true,
  );
  await page.screenshot({ path: ".qa/mount-details-dark-narrow.png" });
  await page.keyboard.press("Escape");
  await page.clock.runFor(31_000);
  await page.evaluate(() => (window as any).__MOUNTS__.push());
  expect(
    await page.evaluate(() =>
      (window as any).__MOUNTS__.calls.filter(
        (c: any) =>
          c.command === "list_remote_mounts" ||
          c.command === "get_remote_mount_usage",
      ),
    ),
  ).toEqual([]);
  await expect(row(page, "/backup").locator(".mount-usage")).toContainText(
    "68%",
  );
  expect(errors).toEqual([]);
});
