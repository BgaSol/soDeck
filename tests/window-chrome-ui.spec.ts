import { test, expect, type Page } from "@playwright/test";

// Browser IPC simulation verifies application close guards and layout; native
// drag, resize, shadow and OS snapping still require the Windows desktop check.
async function setup(
  page: Page,
  options: { mode?: string; editor?: boolean } = {},
) {
  await page.addInitScript(({ mode = "custom", editor = false }) => {
    const w = window as any;
    let serial = 1;
    let maximized = false;
    const callbacks = new Map<number, Function>();
    const listeners = new Map<number, { event: string; handler: number }>();
    const calls: string[] = [];
    const emit = async (event: string, payload: unknown) => {
      for (const [id, listener] of [...listeners]) {
        if (listener.event === event)
          await callbacks.get(listener.handler)?.({ event, id, payload });
      }
    };
    const doc = {
      serverName: "QA server",
      path: "/long/path/配置文件.txt",
      text: "original\n",
      size: 9,
      version: "v1",
    };
    w.__CHROME_TEST__ = {
      calls,
      setMaximized: async (value: boolean) => {
        maximized = value;
        await emit("tauri://resize", { width: 1000, height: 700 });
      },
      close: () => emit("tauri://close-requested", null),
    };
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: editor ? "editor-qa" : "main" },
        currentWebview: { label: editor ? "editor-qa" : "main" },
      },
      transformCallback: (callback: Function) => {
        const id = serial++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any = {}) => {
        calls.push(command);
        if (command === "window_chrome") return mode;
        if (command === "plugin:event|listen") {
          const id = serial++;
          listeners.set(id, { event: args.event, handler: args.handler });
          return id;
        }
        if (command === "plugin:event|unlisten")
          return listeners.delete(args.eventId);
        if (command === "plugin:window|is_maximized") return maximized;
        if (command === "plugin:window|is_fullscreen") return false;
        if (command === "plugin:window|is_focused") return true;
        if (command === "plugin:window|toggle_maximize") {
          maximized = !maximized;
          return emit("tauri://resize", { width: 1000, height: 700 });
        }
        if (command === "plugin:window|close")
          return emit("tauri://close-requested", null);
        if (command === "list_servers" || command === "list_transfers")
          return [];
        if (command === "list_file_tasks")
          return { tasks: [], summary: { total: 0, active: 0, attention: 0, completed: 0 }, nextCursor: null };
        if (command === "session_count") return 1;
        if (
          command === "log_session_count" ||
          command === "unsaved_editor_count"
        )
          return 0;
        if (command === "editor_load") return doc;
        if (command === "editor_save") {
          doc.text = args.text;
          return { ...doc };
        }
        return null;
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  }, options);
  await page.goto(options.editor ? "/?editor=1" : "/");
  await expect(page.locator(".window-titlebar")).toHaveAttribute(
    "data-chrome",
    options.mode ?? "custom",
  );
}

test("window controls track native state changes and keep close confirmation", async ({
  page,
}) => {
  await setup(page);
  await page.getByRole("button", { name: "最大化窗口", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "还原窗口", exact: true }),
  ).toBeVisible();
  await page.evaluate(() =>
    (window as any).__CHROME_TEST__.setMaximized(false),
  );
  await expect(
    page.getByRole("button", { name: "最大化窗口", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "最小化窗口", exact: true }).click();
  await page.getByRole("button", { name: "关闭窗口", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("活动终端");
  const calls = await page.evaluate(
    () => (window as any).__CHROME_TEST__.calls,
  );
  expect(calls).toContain("plugin:window|minimize");
  expect(calls).not.toContain("plugin:window|destroy");
  expect(calls).not.toContain("quit_app");
});

test("editor close protects edits, allows cancellation, and saves before closing", async ({
  page,
}) => {
  await setup(page, { editor: true });
  await page.locator(".cm-content").fill("changed\n");
  await expect(page.getByLabel("未保存", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "关闭窗口", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("保存修改后关闭");
  await page.getByRole("button", { name: "继续编辑", exact: true }).click();
  await expect(page.locator(".cm-content")).toContainText("changed");
  expect(
    await page.evaluate(() => (window as any).__CHROME_TEST__.calls),
  ).not.toContain("plugin:window|destroy");
  // Simulate the same native close event used by Alt+F4.
  await page.evaluate(() => (window as any).__CHROME_TEST__.close());
  await page.getByRole("button", { name: "保存并关闭", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).__CHROME_TEST__.calls))
    .toContain("plugin:window|destroy");
  const calls: string[] = await page.evaluate(
    () => (window as any).__CHROME_TEST__.calls,
  );
  expect(calls.indexOf("editor_save")).toBeLessThan(
    calls.indexOf("plugin:window|destroy"),
  );
});

test("compact chrome stays usable at minimum sizes and excludes actions from dragging", async ({
  page,
}) => {
  await setup(page);
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await page.setViewportSize({ width: 860, height: 580 });
    await expect(page.locator(".window-titlebar")).toHaveCSS("height", "48px");
    await expect(page.locator("[data-tauri-drag-region] button")).toHaveCount(
      0,
    );
    await expect(
      page.locator(".session-tabs [data-tauri-drag-region]"),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "关闭窗口", exact: true }),
    ).toBeInViewport();
    expect(
      await page
        .locator(".window-drag-region")
        .evaluate((el) => el.getBoundingClientRect().width),
    ).toBeGreaterThanOrEqual(72);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({ path: `.qa/window-chrome-${colorScheme}.png` });
  }
  await page.getByRole("button", { name: "关于 SoDeck", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "关于 SoDeck", exact: true }),
  ).toBeVisible();
});

test("macOS reserves native controls in a narrow editor", async ({ page }) => {
  await setup(page, { mode: "overlay", editor: true });
  await expect(page.locator(".cm-content")).toContainText("original");
  await page.setViewportSize({ width: 560, height: 360 });
  await expect(page.locator(".window-titlebar")).toHaveCSS(
    "padding-left",
    "88px",
  );
  await expect(page.getByRole("group", { name: "窗口控制" })).toHaveCount(0);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({ path: ".qa/window-chrome-editor-overlay.png" });
});

test("Linux retains native window controls", async ({ page }) => {
  await setup(page, { mode: "native" });
  await expect(page.getByRole("group", { name: "窗口控制" })).toHaveCount(0);
  await expect(page.locator("[data-tauri-drag-region]")).toHaveCount(0);
});

test("browser preview has no desktop controls or draggable regions", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator(".window-titlebar")).toHaveAttribute(
    "data-chrome",
    "browser",
  );
  await expect(page.getByRole("group", { name: "窗口控制" })).toHaveCount(0);
  await expect(page.locator("[data-tauri-drag-region]")).toHaveCount(0);
});
