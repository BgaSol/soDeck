import { test, expect } from "@playwright/test";
import { setup, row } from "./resource-fixture";
import { openWorkspaceTool } from "./workspace-tool-helpers";

test("default rails, toggles and service switching preserve terminal and file state", async ({
  page,
}, info) => {
  const errors = await setup(page);
  const files = page.locator('[data-tool="files"]');
  const monitor = page.locator('[data-tool="monitor"]');
  const terminal = page.locator(".workspace-terminal");
  await expect(files).toHaveAttribute("aria-expanded", "true");
  await expect(monitor).toHaveAttribute("aria-expanded", "false");
  await row(page, "/data").press("ArrowRight");
  await expect(row(page, "/data/archive")).toBeVisible();
  await page.evaluate(() => {
    (window as any).__terminalNode =
      document.querySelector(".terminal-surface");
  });
  const before = (await terminal.boundingBox())!.width;
  await files.click();
  await expect(page.locator(".workspace-files")).toBeHidden();
  expect((await terminal.boundingBox())!.width).toBeGreaterThan(before + 200);
  await files.click();
  await expect(row(page, "/data/archive")).toBeVisible();
  await openWorkspaceTool(page, "docker");
  await page.getByRole("textbox", { name: "搜索容器" }).fill("web");
  await openWorkspaceTool(page, "monitor");
  await expect(page.locator('[data-tool="docker"]')).toHaveAttribute(
    "aria-expanded",
    "false",
  );
  await openWorkspaceTool(page, "docker");
  await expect(page.getByRole("textbox", { name: "搜索容器" })).toHaveValue(
    "web",
  );
  await page.locator('[data-tool="docker"]').click();
  await expect(page.locator(".workspace-services")).toBeHidden();
  expect(
    await page.evaluate(
      () =>
        (window as any).__terminalNode ===
        document.querySelector(".terminal-surface"),
    ),
  ).toBe(true);
  const calls = await page.evaluate(() => (window as any).__MOUNTS__.calls);
  expect(
    calls.filter((c: any) => c.command === "open_connection"),
  ).toHaveLength(1);
  expect(
    calls.filter((c: any) => c.command === "subscribe_monitor"),
  ).toHaveLength(1);
  await page.locator(".terminal-surface textarea").focus();
  await page.mouse.move(0, 0);
  await page.screenshot({
    path: info.outputPath("workspace-default-light.png"),
  });
  expect(errors).toEqual([]);
});

test("responsive panels, font scaling and focus retain user intent", async ({
  page,
}, info) => {
  const errors = await setup(page);
  await openWorkspaceTool(page, "monitor");
  await expect(page.locator(".workspace-files")).toBeVisible();
  await page.locator(".terminal-surface textarea").focus();
  await page.mouse.move(0, 0);
  await page.screenshot({ path: info.outputPath("workspace-wide-light.png") });
  await page.setViewportSize({ width: 1024, height: 780 });
  await expect(page.locator(".workspace-files")).toBeHidden();
  await expect(page.locator(".workspace-services")).toHaveAttribute(
    "data-placement",
    "docked",
  );
  expect(
    (await page.locator(".workspace-terminal").boundingBox())!.width,
  ).toBeGreaterThanOrEqual(560);
  await page.setViewportSize({ width: 860, height: 650 });
  await expect(page.locator(".workspace-services")).toHaveAttribute(
    "data-placement",
    "overlay",
  );
  const overlay = (await page.locator(".workspace-services").boundingBox())!;
  expect(overlay.x).toBeGreaterThan(44);
  expect(overlay.x + overlay.width).toBeLessThan(860 - 44);
  const input = page.locator(".terminal-surface textarea");
  await input.focus();
  await page.mouse.move(0, 0);
  await page.screenshot({
    path: info.outputPath("workspace-overlay-light.png"),
  });
  await input.press("Escape");
  await expect(page.locator(".workspace-services")).toBeVisible();
  await page.getByRole("button", { name: "刷新资源监控" }).focus();
  await page.keyboard.press("Escape");
  await expect(page.locator(".workspace-services")).toBeHidden();
  await expect(page.locator('[data-tool="monitor"]')).toBeFocused();
  await openWorkspaceTool(page, "monitor");
  await page.setViewportSize({ width: 650, height: 540 });
  await expect(page.locator(".workspace-terminal")).toBeHidden();
  await page.getByRole("button", { name: "刷新资源监控" }).focus();
  await page.mouse.move(0, 0);
  await page.screenshot({
    path: info.outputPath("workspace-narrow-light.png"),
  });
  await page.getByRole("button", { name: "返回终端", exact: true }).click();
  await expect(input).toBeFocused();
  await openWorkspaceTool(page, "files");
  await expect(row(page, "/data")).toBeVisible();
  await page.getByRole("button", { name: "返回终端", exact: true }).click();
  await page.setViewportSize({ width: 1440, height: 950 });
  await expect(page.locator(".workspace-files")).toBeVisible();
  await expect(page.locator(".workspace-services")).toBeVisible();
  await input.focus();
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "21px";
    document.documentElement.dataset.theme = "dark";
  });
  await page.emulateMedia({ reducedMotion: "reduce", contrast: "more" });
  await expect(page.locator(".responsive-workspace")).toHaveCSS(
    "--workspace-rail-width",
    "66px",
  );
  await expect(input).toBeFocused();
  await expect(page.locator(".workspace-files")).toBeVisible();
  await expect(page.locator(".workspace-services")).toBeHidden();
  await openWorkspaceTool(page, "monitor");
  await expect(page.locator(".workspace-services")).toBeVisible();
  await input.focus();
  await page.mouse.move(0, 0);
  expect(
    await page
      .locator(".responsive-workspace")
      .evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await page.screenshot({ path: info.outputPath("workspace-scaled-dark.png") });
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "14px";
  });
  await page.emulateMedia({
    reducedMotion: "no-preference",
    contrast: "no-preference",
  });
  await expect(page.locator(".responsive-workspace")).toHaveCSS(
    "--workspace-rail-width",
    "44px",
  );
  await expect(page.locator(".workspace-files")).toBeVisible();
  await page.screenshot({ path: info.outputPath("workspace-wide-dark.png") });
  expect(errors).toEqual([]);
});

test("presented file modal survives resize and restores focus to its rail", async ({
  page,
}) => {
  const errors = await setup(page);
  await row(page, "/data").click({ button: "right" });
  await page.getByRole("menuitem", { name: "新建文件", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("textbox").fill("keep-draft.txt");
  await page.setViewportSize({ width: 650, height: 540 });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("textbox")).toHaveValue("keep-draft.txt");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('[data-tool="files"]')).toBeFocused();
  expect(errors).toEqual([]);
});

test("terminal resize IPC serializes and sends the newest dimensions while output continues", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const w = window as any,
      base = w.__TAURI_INTERNALS__.invoke;
    w.__sizes = { busy: 0, peak: 0, values: [] };
    w.__TAURI_INTERNALS__.invoke = async (command: string, args: any) => {
      if (command !== "terminal_resize") return base(command, args);
      w.__sizes.peak = Math.max(w.__sizes.peak, ++w.__sizes.busy);
      w.__sizes.values.push(args);
      await new Promise((resolve) => setTimeout(resolve, 80));
      w.__sizes.busy--;
    };
  });
  for (const width of [1100, 1024, 860, 1440]) {
    await page.setViewportSize({ width, height: 850 });
    await openWorkspaceTool(page, "docker");
  }
  await page.locator('[data-tool="docker"]').click();
  await page.locator('[data-tool="files"]').click();
  await page.evaluate(() =>
    (window as any).__MOUNTS__.terminal.onmessage({
      type: "data",
      data: btoa("continued-output\r\n"),
    }),
  );
  await expect(page.locator(".terminal-surface")).toContainText(
    "continued-output",
  );
  await expect
    .poll(() => page.evaluate(() => (window as any).__sizes.busy))
    .toBe(0);
  const sizes = await page.evaluate(() => (window as any).__sizes);
  expect(sizes.peak).toBe(1);
  expect(sizes.values.length).toBeGreaterThan(0);
  expect(sizes.values.every((v: any) => v.cols > 0 && v.rows > 0)).toBe(true);
  expect(
    new Set(sizes.values.map((v: any) => `${v.cols}:${v.rows}`)).size,
  ).toBeGreaterThan(1);
  expect(errors).toEqual([]);
});
