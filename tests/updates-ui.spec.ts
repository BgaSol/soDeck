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
          currentVersion: "1.0.0",
          release: null,
          install: {
            mode: automaticInstall ? "inApp" : "manual",
            reason: automaticInstall ? null : "此安装方式请手动下载。",
            target: automaticInstall ? "windows-x86_64-nsis" : null,
            manualFormat: automaticInstall ? "exe" : "msi",
          },
          operation: null,
          progress: null,
          preferences: { revision: 0, autoCheck: true, autoDownload: true },
          lastSuccessfulCheckAt: null,
          suppression: null,
          error: null,
          result: null,
          message: null,
          skippedVersion: null,
        },
        calls: [] as string[],
        blockers: [] as any[],
        failCheck: false,
        reviewChanged: false,
        installToken: "",
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
              release: {
                id: "release-a",
                version: "1.0.1",
                notes: "修复连接稳定性",
                publishedAt: null,
              },
              lastSuccessfulCheckAt: 1700000000,
            });
            return { ...mock.state };
          }
          if (command === "update_preferences") {
            mock.emit({
              preferences: {
                ...mock.state.preferences,
                ...args.patch,
                revision: mock.state.preferences.revision + 1,
              },
            });
            return { ...mock.state };
          }
          if (command === "update_download") {
            mock.emit({
              phase: "downloading",
              operation: { id: "operation-a", source: "manual" },
              progress: { downloaded: 1048576, total: 4194304 },
            });
            return { ...mock.state };
          }
          if (command === "update_cancel_download") {
            mock.emit({
              phase: "available",
              operation: null,
              progress: null,
              suppression: "cancelled",
            });
            return { ...mock.state };
          }
          if (command === "update_skip_version") {
            mock.emit({ skippedVersion: args.skip ? "1.0.1" : null });
            return { ...mock.state };
          }
          if (command === "update_ack_notice") return { ...mock.state };
          if (command === "update_ack_result") {
            if (args.resultId === mock.state.result?.id)
              mock.emit({ result: null });
            return { ...mock.state };
          }
          if (command === "update_preflight")
            return {
              releaseId: "release-a",
              reviewToken: mock.blockers.length ? null : "review-a",
              expiresAt: Date.now() / 1000 + 60,
              blockers: mock.blockers,
              interruptions: [
                { kind: "terminals", count: 2, connections: null },
                { kind: "portForwarding", count: 1, connections: 3 },
              ],
            };
          if (command === "update_install") {
            mock.installToken = args.reviewToken;
            if (mock.reviewChanged)
              return {
                status: "reviewChanged",
                review: {
                  releaseId: "release-a",
                  reviewToken: "review-b",
                  expiresAt: Date.now() / 1000 + 60,
                  blockers: [],
                  interruptions: [
                    { kind: "terminals", count: 3, connections: null },
                  ],
                },
              };
            mock.emit({ phase: "installing" });
            return { status: "accepted" };
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

test("startup result survives a background check until explicitly dismissed", async ({
  page,
}) => {
  const section = await setup(page);
  await page.evaluate(() =>
    (window as any).__UPDATES__.emit({
      result: { id: "attempt-a", targetVersion: "1.0.0", outcome: "completed" },
    }),
  );
  await expect(section).toContainText("已更新到 v1.0.0");
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(section).toContainText("已更新到 v1.0.0");
  await section.getByRole("button", { name: "关闭", exact: true }).click();
  await expect(section).not.toContainText("已更新到 v1.0.0");
  expect(
    await page.evaluate(() =>
      (window as any).__UPDATES__.calls.includes("update_ack_result"),
    ),
  ).toBe(true);
});

test("failed installer handoff requires recovery restart before more work", async ({
  page,
}) => {
  const section = await setup(page);
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await page.evaluate(() =>
    (window as any).__UPDATES__.emit({
      phase: "failed",
      error: { stage: "install", code: "restartRequired", retry: "none" },
      message: "安装未完成，部分连接已关闭。请重启 SoDeck 后继续工作。",
    }),
  );
  await expect(
    section.getByRole("button", { name: "检查更新", exact: true }),
  ).toBeDisabled();
  await expect(
    section.getByRole("button", { name: "下载更新", exact: true }),
  ).toHaveCount(0);
  await expect(
    section.getByRole("checkbox", { name: "自动下载更新", exact: true }),
  ).toBeDisabled();
  await section
    .getByRole("button", { name: "重启 SoDeck", exact: true })
    .click();
  expect(
    await page.evaluate(() =>
      (window as any).__UPDATES__.calls.includes("update_restart_recovery"),
    ),
  ).toBe(true);
});
test("download survives settings close and requires a fresh work check before confirmation", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const section = await setup(page);
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(section).toContainText("v1.0.1");
  await section.getByRole("button", { name: "下载更新", exact: true }).click();
  await expect(section.getByRole("progressbar")).toHaveAttribute(
    "value",
    "1048576",
  );
  await page.keyboard.press("Escape");
  await page.evaluate(() =>
    (window as any).__UPDATES__.emit({
      phase: "ready",
      progress: null,
      operation: null,
    }),
  );
  await page.getByRole("button", { name: "系统设置", exact: true }).click();
  await expect(section).toContainText("更新已准备好");
  await page.evaluate(() => {
    (window as any).__UPDATES__.blockers = [
      { kind: "editors", count: 1, action: "saveAndCloseEditors" },
    ];
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
  await expect(section).toContainText("停止端口转发与内网代理：1 项");
  await expect(
    section.getByRole("button", { name: "确认安装并重启", exact: true }),
  ).toBeEnabled();
  await expect(
    section.getByRole("button", { name: "安装并重启", exact: true }),
  ).toHaveCount(0);
  await page.screenshot({
    path: ".qa/updates-confirm-1.0.png",
    fullPage: true,
  });
  expect(
    await page.evaluate(() =>
      (window as any).__UPDATES__.calls.includes("update_install"),
    ),
  ).toBe(false);
  await section
    .getByRole("button", { name: "确认安装并重启", exact: true })
    .click();
  await expect(section).toContainText("正在关闭连接并安装");
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
  await section.getByRole("button", { name: "下载 MSI 安装包" }).click();
  expect(
    await page.evaluate(() =>
      (window as any).__UPDATES__.calls.includes("update_open_manual_asset"),
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
      operation: null,
      progress: null,
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

test("cancellation stays available and preferences are independent during download", async ({
  page,
}) => {
  const section = await setup(page);
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await section.getByRole("button", { name: "下载更新", exact: true }).click();
  await expect(section.getByRole("button", { name: "取消下载" })).toBeEnabled();
  await section.getByRole("checkbox", { name: "自动检查更新" }).uncheck();
  await expect(
    section.getByRole("checkbox", { name: "自动下载更新" }),
  ).toBeChecked();
  await section.getByRole("button", { name: "取消下载" }).click();
  await expect(
    section.getByRole("button", { name: "下载更新", exact: true }),
  ).toBeEnabled();
  await expect(section.getByRole("alert")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "系统设置", exact: true }).click();
  await expect(section.getByRole("progressbar")).toHaveCount(0);
  expect(
    await page.evaluate(() => (window as any).__UPDATES__.state.suppression),
  ).toBe("cancelled");
});

test("changed installation impact requires a fresh confirmation and later keeps the package", async ({
  page,
}) => {
  const section = await setup(page);
  await section.getByRole("button", { name: "检查更新", exact: true }).click();
  await page.evaluate(() => {
    const m = (window as any).__UPDATES__;
    m.emit({ phase: "ready" });
    m.reviewChanged = true;
  });
  await section
    .getByRole("button", { name: "安装并重启", exact: true })
    .click();
  await section
    .getByRole("button", { name: "确认安装并重启", exact: true })
    .click();
  await expect(section).toContainText("断开终端会话：3 项");
  expect(
    await page.evaluate(() => (window as any).__UPDATES__.installToken),
  ).toBe("review-a");
  await section.getByRole("button", { name: "稍后", exact: true }).click();
  await expect(section).toContainText("更新已准备好");
  await expect(section.getByRole("group", { name: "安装前检查" })).toHaveCount(
    0,
  );
});

test("background progress never opens settings or moves focus and reduced motion is respected", async ({
  page,
}) => {
  const section = await setup(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "系统设置", exact: true }).focus();
  await page.evaluate(() =>
    (window as any).__UPDATES__.emit({
      phase: "downloading",
      operation: { id: "background", source: "automatic" },
      progress: { downloaded: 512, total: null },
    }),
  );
  await expect(section).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "系统设置", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(section.getByRole("progressbar")).not.toHaveAttribute("value");
  await expect(section.getByRole("button", { name: "取消下载" })).toBeEnabled();
});
