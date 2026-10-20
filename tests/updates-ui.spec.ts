import { test, expect, type Page } from "@playwright/test";
async function setup(page: Page, automaticInstall = true) {
  await page.addInitScript(
    ({ automaticInstall }) => {
      const w = window as any;
      let serial = 1;
      const callbacks = new Map<number, Function>(),
        listeners = new Map<number, { event: string; handler: number }>();
      const mock = (w.__UPDATES__ = {
        state: {
          revision: 0,
          phase: "idle",
          currentVersion: "0.1.1",
          version: null,
          notes: null,
          publishedAt: null,
          downloaded: 0,
          total: null,
          message: null,
          automaticInstall,
          installReason: automaticInstall
            ? "安装将关闭所有窗口并重启。"
            : "此安装方式请手动下载。",
          autoCheck: true,
          skippedVersion: null,
          lastCheck: null,
        },
        calls: [] as string[],
        blockers: [] as string[],
        failCheck: false,
        emit(patch: any) {
          Object.assign(mock.state, patch, {
            revision: mock.state.revision + 1,
          });
          for (const [id, l] of listeners)
            if (l.event === "app-update-changed")
              callbacks.get(l.handler)?.({
                event: l.event,
                id,
                payload: { ...mock.state },
              });
        },
        event(event: string) {
          for (const [id, l] of listeners)
            if (l.event === event)
              callbacks.get(l.handler)?.({ event, id, payload: null });
        },
      });
      w.isTauri = true;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback(fn: Function) {
          const id = serial++;
          callbacks.set(id, fn);
          return id;
        },
        unregisterCallback(id: number) {
          callbacks.delete(id);
        },
        convertFileSrc: (s: string) => s,
        async invoke(command: string, args: any = {}) {
          mock.calls.push(command);
          if (command === "plugin:event|listen") {
            const id = serial++;
            listeners.set(id, { event: args.event, handler: args.handler });
            return id;
          }
          if (command === "plugin:event|unlisten") {
            listeners.delete(args.eventId);
            return;
          }
          if (command === "window_chrome") return "custom";
          if (command === "get_system_settings")
            return {
              revision: 0,
              developerMode: false,
              activeDeveloperMode: false,
              platform: "windows",
              loadError: null,
              terminalClipboard: {
                shortcutMode: "platform",
                copyOnSelect: false,
                rightClickPaste: false,
              },
            };
          if (command === "list_servers") return [];
          if (command === "update_snapshot") return { ...mock.state };
          if (command === "update_check") {
            if (mock.failCheck) throw { message: "网络不可用，请重试" };
            mock.emit({
              phase: "available",
              version: "0.1.2",
              notes: "修复连接稳定性",
              lastCheck: 1700000000,
            });
            return { ...mock.state };
          }
          if (command === "update_preferences") {
            mock.emit(args);
            return { ...mock.state };
          }
          if (command === "update_download") {
            mock.emit({
              phase: "downloading",
              downloaded: 1048576,
              total: 4194304,
            });
            return { ...mock.state };
          }
          if (command === "update_preflight")
            return {
              blockers: mock.blockers,
              interruptions: ["断开 2 个终端会话", "停止 1 条端口转发"],
            };
          if (command === "update_install") {
            mock.emit({ phase: "installing" });
            return;
          }
          if (command === "update_focus_main") {
            mock.event("open-software-update");
            return;
          }
          if (command === "list_file_tasks")
            return {
              tasks: [],
              summary: { active: 0, attention: 0 },
              nextCursor: null,
            };
          if (command === "list_port_forwards") return [];
          return null;
        },
      };
    },
    { automaticInstall },
  );
  await page.goto("/");
  await page.getByRole("button", { name: "系统设置", exact: true }).click();
  return page.getByRole("region", { name: "软件更新", exact: true });
}
test("download survives settings close and requires a fresh work check before confirmation", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const section = await setup(page);
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(section).toContainText("v0.1.2");
  await section.getByRole("button", { name: "下载更新", exact: true }).click();
  await expect(section.getByRole("progressbar")).toHaveAttribute(
    "value",
    "1048576",
  );
  await page.keyboard.press("Escape");
  await page.evaluate(() =>
    (window as any).__UPDATES__.emit({
      phase: "ready",
      downloaded: 4194304,
      total: 4194304,
    }),
  );
  await page.getByRole("button", { name: "系统设置", exact: true }).click();
  await expect(section).toContainText("更新已准备好");
  await page.evaluate(() => {
    (window as any).__UPDATES__.blockers = ["请保存并关闭所有远程文件编辑窗口"];
  });
  await section
    .getByRole("button", { name: "安装并重启", exact: true })
    .click();
  await expect(section).toContainText("请保存并关闭");
  await expect(
    section.getByRole("button", { name: "确认安装并重启", exact: true }),
  ).toHaveCount(0);
  await page.evaluate(() => {
    (window as any).__UPDATES__.blockers = [];
  });
  await section.getByRole("button", { name: "重新检查", exact: true }).click();
  await expect(section).toContainText("停止 1 条端口转发");
  await expect(
    section.getByRole("button", { name: "确认安装并重启", exact: true }),
  ).toBeEnabled();
  await page.screenshot({ path: ".qa/updates-confirm.png", fullPage: true });
  expect(
    await page.evaluate(() =>
      (window as any).__UPDATES__.calls.includes("update_install"),
    ),
  ).toBe(false);
  await section
    .getByRole("button", { name: "确认安装并重启", exact: true })
    .click();
  await expect(section).toContainText("正在安装");
  expect(errors).toEqual([]);
});
test("manual formats show a download link; network failures remain errors", async ({
  page,
}) => {
  const section = await setup(page, false);
  await page.evaluate(() => {
    (window as any).__UPDATES__.failCheck = true;
  });
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(section.getByRole("alert")).toContainText("网络不可用");
  await page.evaluate(() => {
    (window as any).__UPDATES__.failCheck = false;
  });
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(
    section.getByRole("button", { name: "下载更新", exact: true }),
  ).toHaveCount(0);
  await section.getByRole("button", { name: "跳过此版本" }).click();
  await expect(
    section.getByRole("button", { name: "恢复版本提醒" }),
  ).toBeVisible();
  await section.getByRole("button", { name: "打开发布页 / 手动下载" }).click();
  expect(
    await page.evaluate(() =>
      (window as any).__UPDATES__.calls.includes("update_open_release"),
    ),
  ).toBe(true);
});

test("download verification failure never offers installation and can be retried", async ({
  page,
}) => {
  const section = await setup(page);
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await section.getByRole("button", { name: "下载更新", exact: true }).click();
  await page.evaluate(() =>
    (window as any).__UPDATES__.emit({
      phase: "failed",
      message: "更新下载或签名验证失败；未执行安装。",
    }),
  );
  await expect(section).toContainText("签名验证失败");
  await expect(
    section.getByRole("button", { name: "安装并重启", exact: true }),
  ).toHaveCount(0);
  await section.getByRole("button", { name: "下载更新", exact: true }).click();
  await expect(section.getByRole("progressbar")).toBeVisible();
  expect(
    await page.evaluate(() =>
      (window as any).__UPDATES__.calls.includes("update_install"),
    ),
  ).toBe(false);
});
