import { test, expect, type Page } from "@playwright/test";

// Simulated IPC for presentation/interaction; the Rust tests exercise parsing and SSH separately.
async function setup(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(() => {
    const w = window as any;
    let serial = 1;
    const server = {
      id: "mount-test",
      name: "Mount fixture",
      host: "fixture.invalid",
      port: 22,
      username: "ops",
      authType: "password",
      hasSavedPassword: true,
      privateKey: "",
      tags: [],
      jumpServerId: null,
      directHost: null,
      directPort: 22,
    };
    const entry = (path: string, isDir = true, isSymlink = false) => ({
      path,
      name: path.split("/").pop() || "/",
      isDir,
      isSymlink,
      size: isDir ? 0 : 42,
      modified: 0,
    });
    const entries = [
      entry("/data"),
      entry("/backup"),
      entry("/proc"),
      entry("/link", true, true),
      entry("/中文 '文件.txt", false),
    ];
    const directory = (path: string) => ({
      path,
      resolvedPath: path === "/link" ? "/data" : path,
      entries:
        path === "/"
          ? entries
          : path === "/data"
            ? [entry("/data/apps"), entry("/data/archive")]
            : [],
    });
    const table = {
      version: "v1",
      status: "ready",
      mounts: [
        {
          key: "root",
          target: "/",
          source: "/dev/sda1",
          fsRoot: "/",
          fsType: "ext4",
          readOnly: false,
        },
        {
          key: "data",
          target: "/data",
          source: "/dev/sdb1",
          fsRoot: "/",
          fsType: "xfs",
          readOnly: false,
        },
        {
          key: "backup",
          target: "/backup",
          source: "nas:/backup",
          fsRoot: "/",
          fsType: "nfs4",
          readOnly: true,
        },
        {
          key: "archive",
          target: "/data/archive",
          source: "/dev/sdc1",
          fsRoot: "/archive",
          fsType: "ext4",
          readOnly: false,
        },
        {
          key: "proc",
          target: "/proc",
          source: "proc",
          fsRoot: "/",
          fsType: "proc",
          readOnly: false,
        },
      ],
    };
    const calls: any[] = [];
    const callbacks = new Map<number, Function>();
    w.__MOUNTS__ = {
      calls,
      table,
      copied: "",
      failCopy: false,
      failUsage: false,
      clipboard: {
        id: "existing",
        paths: ["/data/apps"],
        intent: "move",
        sameServer: true,
        reserved: false,
      },
    };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          if (w.__MOUNTS__.failCopy) throw new Error("denied");
          w.__MOUNTS__.copied = text;
        },
      },
    });
    w.isTauri = true;
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: (fn: Function) => {
        const id = serial++;
        callbacks.set(id, fn);
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any = {}) => {
        calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
        if (command === "window_chrome") return "custom";
        if (command === "list_servers") return [server];
        if (command === "list_transfers") return [];
        if (command === "plugin:event|listen") return serial++;
        if (command === "open_connection") {
          setTimeout(
            () =>
              args.events.onmessage({
                type: "ready",
                connection: {
                  target: server,
                  hops: [{ index: 0, role: "target", server }],
                  identity: "fixture",
                },
              }),
            0,
          );
          return "mount-session";
        }
        if (command === "get_remote_clipboard")
          return structuredClone(w.__MOUNTS__.clipboard);
        if (command === "list_directory_chain")
          return {
            homePath: "/",
            path: "/",
            directories: [{ path: "/", directory: directory("/") }],
          };
        if (command === "list_directory") return directory(args.path);
        if (command === "list_remote_mounts")
          return structuredClone(w.__MOUNTS__.table);
        if (command === "get_remote_mount_usage") {
          if (w.__MOUNTS__.failUsage)
            throw { kind: "timeout", message: "网络挂载查询超时" };
          return args.mountKeys.map((key: string) => ({
            mountKey: key,
            status: key === "proc" ? "unsupported" : "ready",
            totalBytes: "1099511627776",
            usedBytes: "747667906888",
            availableBytes: "351843720888",
            usedPercent: key === "proc" ? null : key === "archive" ? 96 : 68,
            updatedAt: key === "proc" ? null : Date.now(),
            error: key === "proc" ? "此文件系统不提供常规容量" : null,
          }));
        }
        if (command === "detect_docker")
          return { state: "missing", message: "fixture", endpoint: "" };
        if (
          command.endsWith("session_count") ||
          command === "unsaved_editor_count"
        )
          return 0;
        return null;
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await page
    .locator(".server-row")
    .filter({ hasText: "Mount fixture" })
    .dblclick();
  await expect(row(page, "/data").locator(".mount-usage")).toContainText("68%");
  return errors;
}
const row = (page: Page, path: string) =>
  page.locator(`[role=treeitem][data-path=${JSON.stringify(path)}]`);

test("mount summaries, nested mounts, details and keyboard preserve tree behavior", async ({
  page,
}) => {
  const errors = await setup(page);
  const data = row(page, "/data");
  await expect(data.locator(".mount-source")).toHaveText("/dev/sdb1");
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
    .click({ modifiers: ["Control"] });
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
  await expect(row(page, "/data").locator(".mount-usage")).toContainText(
    "68% · 旧",
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

test("mount details fit narrow dark windows and refresh after 30 seconds", async ({
  page,
}) => {
  await page.clock.install();
  await page.emulateMedia({ colorScheme: "dark" });
  const errors = await setup(page);
  await page.setViewportSize({ width: 850, height: 600 });
  const resizer = page.getByRole("separator", { name: "调整文件区域宽度" });
  await resizer.focus();
  for (let i = 0; i < 14; i++) await resizer.press("ArrowLeft");
  await row(page, "/backup").locator(".mount-usage").click();
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
  const before = await page.evaluate(
    () =>
      (window as any).__MOUNTS__.calls.filter(
        (c: any) => c.command === "list_remote_mounts",
      ).length,
  );
  await page.clock.runFor(31_000);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__MOUNTS__.calls.filter(
            (c: any) => c.command === "list_remote_mounts",
          ).length,
      ),
    )
    .toBeGreaterThan(before);
  expect(errors).toEqual([]);
});
