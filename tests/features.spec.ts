import { openWorkspaceTool } from "./workspace-tool-helpers";
import { showServers, connectSelectedServer, serverAction } from "./server-picker";
import {
  chromium,
  expect,
  test,
  type Browser,
  type BrowserContext,
  type Page,
} from "@playwright/test";
import { readFile, stat, unlink, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import type { DownloadTask, EditorDocument } from "../src/fileApi";

// These tests use the real Tauri IPC and loopback SSH fixture; no backend is mocked.
// Run alongside desktop.spec.ts, against an isolated development database and CDP 9223.
async function ipc<T>(
  page: Page,
  command: string,
  args: object = {},
): Promise<T> {
  return page.evaluate(
    async ({ command, args }) => {
      try {
        return await (window as any).__TAURI_INTERNALS__.invoke(command, args);
      } catch (error) {
        throw new Error(JSON.stringify(error));
      }
    },
    { command, args },
  );
}

async function closeWindow(page: Page) {
  if (page.isClosed()) return;
  const closed = page.waitForEvent("close");
  await page
    .evaluate(() => {
      const internals = (window as any).__TAURI_INTERNALS__;
      return internals.invoke("plugin:window|close", {
        label: internals.metadata.currentWindow.label,
      });
    })
    .catch((error) => {
      if (
        !/Target page, context or browser has been closed|Execution context was destroyed/.test(
          error.message,
        )
      )
        throw error;
    });
  // Only our disposable QA editor may contain unsaved text during failure cleanup.
  if (!page.isClosed() && page.url().includes("editor=1")) {
    await Promise.race([
      closed,
      page
        .getByRole("button", { name: "放弃修改", exact: true })
        .waitFor({ state: "visible", timeout: 2000 })
        .then(() =>
          page.getByRole("button", { name: "放弃修改", exact: true }).click(),
        )
        .catch(() => {}),
    ]);
  }
  await closed;
}

async function authenticate(page: Page) {
  await expect(async () => {
    const trust = page.getByRole("button", { name: "确认指纹并连接" });
    if (await trust.isVisible()) await trust.click();
    await expect(
      page.locator(".session-page[role=tabpanel]:visible").locator(".connection-state"),
    ).toHaveText("已连接", { timeout: 1000 });
  }).toPass({ timeout: 20000 });
}

async function terminal(page: Page, command: string) {
  await page
    .locator(".session-page[role=tabpanel]:visible")
    .locator(".terminal-surface textarea")
    .focus();
  await page.keyboard.type(command);
  await page.keyboard.press("Enter");
}

async function goTo(page: Page, path: string) {
  const input = page.locator(".session-page[role=tabpanel]:visible").getByLabel("远程目录路径");
  await expect(input).toBeEnabled();
  await input.fill(path);
  await input.press("Enter");
  await expect(input).toBeEnabled();
  await expect(input).toHaveValue(path);
}

interface Fixture {
  browser: Browser;
  context: BrowserContext;
  main: Page;
  name: string;
  serverId: string;
  sessionId: string;
  cleanup: () => Promise<void>;
}

async function openLogs(f: Fixture, id = "a".repeat(64), name = "web-fixture") {
  const opened = f.context.waitForEvent("page");
  await ipc(f.main, "open_container_logs", {
    sessionId: f.sessionId,
    target: { id, name, endpoint: "unix:///var/run/docker.sock" },
  });
  const page = await opened;
  await expect(page.locator(".log-virtual-list")).toBeVisible();
  return page;
}

async function logEvents() {
  return (
    await readFile(
      join(process.env.SSH_WORKSPACE_FIXTURE_DIR || ".qa", "ssh-events.jsonl"),
      "utf8",
    )
  )
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
}

test("language highlighting detects Java, JSON and shebang; manual changes preserve dirty state and undo", async () => {
  const samples = [
    {
      name: "Main.java",
      language: "Java",
      text: 'public class Main {\n  String message = "hello";\n  int port = 8080;\n}\n',
    },
    {
      name: "settings.json",
      language: "JSON",
      text: '{ "port": 8080, "enabled": true }\n',
    },
    {
      name: "entrypoint",
      language: "Shell",
      text: '#!/bin/sh\n# startup\necho "$HOME"\n',
    },
  ];
  for (const sample of samples)
    await writeFile(resolve(".qa/remote/editing", sample.name), sample.text);
  const f = await fixture("syntax");
  let editor: Page | undefined;
  try {
    for (const sample of samples) {
      const opened = f.context.waitForEvent("page");
      await ipc(f.main, "open_editor", {
        sessionId: f.sessionId,
        path: `/editing/${sample.name}`,
      });
      editor = await opened;
      await expect(editor.getByLabel("文件语言")).toHaveValue("auto");
      await expect(editor.locator('option[value="auto"]')).toHaveText(
        `自动：${sample.language}`,
      );
      await expect
        .poll(() => editor!.locator(".cm-line span").count())
        .toBeGreaterThan(2);
      const content = editor.locator(".cm-content");
      const colors = await content
        .locator("span")
        .evaluateAll((nodes) => [
          ...new Set(nodes.map((node) => getComputedStyle(node).color)),
        ]);
      expect(colors.length).toBeGreaterThan(1);
      await content.click();
      await editor.keyboard.press("ControlOrMeta+End");
      await editor.keyboard.insertText("// local edit");
      await expect(editor.getByRole("status")).toHaveText("未保存");
      const editedText = await content.innerText();
      const position = await editor
        .locator(".remote-editor-status > span")
        .last()
        .innerText();
      await editor.getByLabel("文件语言").selectOption("text");
      await expect(editor.locator(".cm-line span")).toHaveCount(0);
      await expect(content).toHaveText(editedText, { useInnerText: true });
      await expect(editor.getByRole("status")).toHaveText("未保存");
      await expect(
        editor.locator(".remote-editor-status > span").last(),
      ).toHaveText(position);
      await editor.getByLabel("文件语言").selectOption("auto");
      await editor.getByRole("button", { name: "撤销", exact: true }).click();
      await expect(editor.getByRole("status")).toHaveText("已与远端同步");
      await editor.getByRole("button", { name: "重做", exact: true }).click();
      await editor.getByRole("button", { name: "保存", exact: true }).click();
      await expect
        .poll(() =>
          readFile(resolve(".qa/remote/editing", sample.name), "utf8"),
        )
        .toBe(sample.text + "// local edit");
      if (sample.language === "Java") {
        await editor.emulateMedia({ colorScheme: "light" });
        await editor.screenshot({ path: ".qa/syntax-java-light.png" });
        await editor.emulateMedia({ colorScheme: "dark" });
        await editor.screenshot({ path: ".qa/syntax-java-dark.png" });
      }
      await closeWindow(editor);
      editor = undefined;
    }
  } finally {
    if (editor && !editor.isClosed()) await closeWindow(editor);
    await f.cleanup();
  }
});

test("container logs persist, highlight, search, virtualize and remain readable offline after closing the main tab", async () => {
  const qa = process.env.SSH_WORKSPACE_FIXTURE_DIR || ".qa";
  await unlink(join(qa, "logs-mode")).catch(() => {});
  await unlink(join(qa, "logs-disconnect")).catch(() => {});
  await unlink(join(qa, "logs-freeze")).catch(() => {});
  const f = await fixture("logs");
  let logs: Page | undefined,
    archive: Page | undefined,
    library: Page | undefined;
  try {
    logs = await openLogs(f);
    await expect(logs.locator(".log-status")).toHaveText("实时接收");
    await expect(logs.locator(".log-viewer")).toContainText(
      "fixture history stdout",
    );
    await expect
      .poll(() => logs!.locator(".log-level-error").count())
      .toBeGreaterThan(0);
    await expect
      .poll(() => logs!.locator(".log-token-key").count())
      .toBeGreaterThan(0);
    await expect
      .poll(() => logs!.locator(".log-ansi-green").count())
      .toBeGreaterThan(0);
    const info = await ipc<any>(logs, "logs_info");
    const pages = f.context.pages().length;
    await ipc(f.main, "open_container_logs", {
      sessionId: f.sessionId,
      target: {
        id: "a".repeat(64),
        name: "web-fixture",
        endpoint: "unix:///var/run/docker.sock",
      },
    });
    expect(f.context.pages().length).toBe(pages);
    // Verify the independent SSH terminal without depending on which native window has OS keyboard focus.
    await ipc(f.main, "terminal_input", { sessionId:f.sessionId, data:"echo usable-during-logs\r" });
    await expect(
      f.main.locator(".session-page[role=tabpanel]:visible").locator(".terminal-surface"),
    ).toContainText(/usable-during-logs\s*ops@host/);
    await logs.getByRole("button", {name:"显示",exact:true}).click();
    await logs.getByLabel("时间戳", { exact: true }).uncheck();
    await expect(logs.locator(".log-row-body").first()).not.toContainText(
      "2026-10-14T",
    );
    await logs.getByLabel("自动换行", { exact: true }).check();
    await expect(logs.locator(".log-viewer")).toHaveClass(/log-wrap/);
    await logs.keyboard.press("Escape");
    await logs.getByRole("button", { name: "查找", exact: true }).click();
    await logs.getByLabel("搜索全部本地日志").fill("中文错误");
    await expect
      .poll(() => logs!.locator(".log-search-match").count())
      .toBeGreaterThan(0);
    await expect(logs.locator(".log-reading-status")).not.toContainText("跟随最新");
    await logs
      .getByRole("button", { name: "上下文", exact: true })
      .first()
      .click();
    await expect(logs.locator(".log-query-status")).toContainText("的上下文");
    await expect(logs.locator(".log-viewer")).toContainText(
      "fixture history stdout",
    );
    if (await logs.getByRole("button",{name:"返回搜索结果"}).isVisible()) await logs.getByRole("button",{name:"返回搜索结果"}).click();
    if (await logs.getByRole("button",{name:"清除条件",exact:true}).isVisible()) await logs.getByRole("button",{name:"清除条件",exact:true}).click();
    await logs.getByRole("button", { name: /继续跟随/ }).click();
    await expect(logs.getByRole("button", {name:"暂停滚动",exact:true})).toBeVisible();
    await expect(logs.getByLabel("显示日志行数")).toHaveCount(0);
    for (const name of ["刷新视图", "恢复采集", "补取全部历史", "停止采集"])
      await expect(logs.getByRole("button", { name, exact: true })).toHaveCount(
        0,
      );
    const openedBefore = (await logEvents()).filter(
      (e) => e.event === "logs-open",
    ).length;
    await logs.getByRole("button", {name:"显示",exact:true}).click();
    await logs.getByLabel("高亮", { exact: true }).uncheck();
    await logs.getByLabel("高亮", { exact: true }).check();
    await logs.keyboard.press("Escape");
    expect(
      (await logEvents()).filter((e) => e.event === "logs-open").length,
    ).toBe(openedBefore);
    await logs.locator(".log-virtual-list").hover();
    await logs.mouse.wheel(0, -200);
    await expect(logs.locator(".log-reading-status")).not.toContainText("跟随最新");
    if (await logs.getByRole("button",{name:"返回搜索结果"}).isVisible()) await logs.getByRole("button",{name:"返回搜索结果"}).click();
    if (await logs.getByRole("button",{name:"清除条件",exact:true}).isVisible()) await logs.getByRole("button",{name:"清除条件",exact:true}).click();
    await logs.getByRole("button", { name: /继续跟随/ }).click();
    await logs.emulateMedia({ colorScheme: "light" });
    await logs.screenshot({ path: ".qa/container-logs-light.png" });
    await logs.emulateMedia({ colorScheme: "dark" });
    await logs.screenshot({ path: ".qa/container-logs-dark.png" });
    await f.main
      .getByRole("button", { name: "关闭 " + f.name + " 会话", exact: true })
      .click();
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(0);
    await expect.poll(() => ipc<number>(f.main, "log_session_count")).toBe(1);
    await expect(logs.locator(".log-status")).toHaveText("实时接收");
    await closeWindow(logs);
    logs = undefined;
    await expect.poll(() => ipc<number>(f.main, "log_session_count")).toBe(0);
    const saved = (await ipc<any[]>(f.main, "list_log_archives")).find(
      (a) => a.sourceId === info.sourceId,
    );
    expect(saved.records).toBeGreaterThan(5);
    const libraryOpened = f.context.waitForEvent("page");
    await f.main.getByRole("button", { name: "本地日志", exact: true }).click();
    library = await libraryOpened;
    await expect(
      library.getByRole("heading", { name: "本地日志", exact: true }),
    ).toBeVisible();
    await expect(
      f.main.getByRole("dialog", { name: "本地日志存档" }),
    ).toHaveCount(0);
    await ipc(f.main, "open_log_archives_window");
    expect(
      f.context
        .pages()
        .filter((p) => new URL(p.url()).searchParams.has("archives")),
    ).toHaveLength(1);
    await library.getByLabel("筛选日志存档").fill(info.containerId);
    const opened = f.context.waitForEvent("page");
    await library
      .getByRole("button", {
        name: `打开 ${info.serverName} / ${info.containerName} 日志存档`,
        exact: true,
      })
      .click();
    archive = await opened;
    await closeWindow(library);
    library = undefined;
    await expect(archive.locator(".log-status")).toHaveText("本地存档");
    expect((await ipc<any>(archive, "logs_info")).stats.records).toBe(
      saved.records,
    );
    await archive.getByRole("button", { name: "查找", exact: true }).click();
    await archive.getByLabel("搜索全部本地日志").fill("中文");
    await expect(archive.locator(".log-viewer")).toContainText("中文错误");
    await expect.poll(() => ipc<number>(f.main, "log_session_count")).toBe(0);
    const copied = await ipc<string>(archive, "logs_copy_range", {
      from: 1,
      to: 1,
    });
    expect(copied).toContain("fixture history stdout");
  } finally {
    for (const page of [logs, archive, library])
      if (page && !page.isClosed()) await closeWindow(page);
    await f.cleanup();
  }
});

test("container logs automatically sync all history, deduplicate reopen/reconnect and resume a stopped container", async () => {
  const qa = process.env.SSH_WORKSPACE_FIXTURE_DIR || ".qa";
  for (const name of ["logs-disconnect", "docker-mode", "docker-started-at"])
    await unlink(join(qa, name)).catch(() => {});
  await writeFile(join(qa, "logs-mode"), "burst");
  await writeFile(join(qa, "logs-freeze"), "1");
  const containerId = Date.now().toString(16).padStart(64, "d");
  const emptyId = Date.now().toString(16).padStart(64, "e");
  const f = await fixture("logs-lifecycle");
  let logs: Page | undefined;
  try {
    logs = await openLogs(f, containerId, "burst-fixture");
    await expect(logs.locator(".log-status")).toHaveText("实时接收", {
      timeout: 60000,
    });
    await expect
      .poll(async () => (await ipc<any>(logs!, "logs_info")).stats.records, {
        timeout: 60000,
      })
      .toBe(12005);
    expect(await logs.locator(".log-row").count()).toBeLessThan(150);
    await logs.getByRole("button", { name: "查找", exact: true }).click();
    await logs.getByLabel("搜索全部本地日志").fill("burst-0 ");
    await expect(logs.locator(".log-viewer")).toContainText("burst-0 ");
    await expect
      .poll(() => logs!.locator(".log-search-match").count())
      .toBeGreaterThan(0);
    if (await logs.getByRole("button",{name:"返回搜索结果"}).isVisible()) await logs.getByRole("button",{name:"返回搜索结果"}).click();
    if (await logs.getByRole("button",{name:"清除条件",exact:true}).isVisible()) await logs.getByRole("button",{name:"清除条件",exact:true}).click();
    await logs.getByRole("button", { name: /继续跟随/ }).click();
    const source = (await ipc<any>(logs, "logs_info")).sourceId;
    await closeWindow(logs);
    logs = undefined;
    await expect.poll(() => ipc<number>(f.main, "log_session_count")).toBe(0);
    logs = await openLogs(f, containerId, "burst-fixture");
    await expect(logs.locator(".log-status")).toHaveText("实时接收");
    await expect
      .poll(async () => (await ipc<any>(logs!, "logs_info")).stats.records)
      .toBe(12005);
    const last = await ipc<any>(logs, "logs_page", {
      query: { keyword: "burst-11999 ", limit: 100 },
    });
    expect(last.rows).toHaveLength(1);
    expect((await ipc<any>(logs, "logs_info")).sourceId).toBe(source);
    await writeFile(join(qa, "logs-disconnect"), "1");
    await expect(logs.locator(".log-status")).toContainText("秒后重试");
    await unlink(join(qa, "logs-disconnect"));
    await expect(logs.locator(".log-status")).toHaveText("实时接收", {
      timeout: 30000,
    });
    await expect
      .poll(async () => (await ipc<any>(logs!, "logs_info")).stats.records)
      .toBe(12005);
    await writeFile(join(qa, "logs-mode"), "unsupported");
    await writeFile(join(qa, "logs-disconnect"), "1");
    await expect(logs.locator(".log-status")).toContainText("秒后重试");
    await unlink(join(qa, "logs-disconnect"));
    await expect(logs.locator(".log-connection-banner")).toContainText(
      "logging driver does not support",
      { timeout: 30000 },
    );
    await expect(
      logs.getByRole("button", { name: "重试", exact: true }),
    ).toBeVisible();
    await writeFile(join(qa, "logs-mode"), "burst");
    await logs.getByRole("button", { name: "重试", exact: true }).click();
    await expect(logs.locator(".log-status")).toHaveText("实时接收", {
      timeout: 30000,
    });
    // CLI diagnostics are stored as system events, not application records.
    expect((await ipc<any>(logs, "logs_info")).stats.records).toBe(12005);
    await closeWindow(logs);
    logs = undefined;
    await writeFile(join(qa, "logs-mode"), "empty");
    await writeFile(join(qa, "docker-mode"), "stopped");
    logs = await openLogs(f, emptyId, "empty-fixture");
    await expect(logs.locator(".log-status")).toHaveText("等待容器启动");
    await expect(logs.getByText("容器暂无日志", { exact: true })).toBeVisible();
    await writeFile(join(qa, "docker-mode"), "running");
    await writeFile(join(qa, "logs-mode"), "normal");
    await unlink(join(qa, "logs-freeze"));
    await expect(logs.locator(".log-status")).toHaveText("实时接收", {
      timeout: 20000,
    });
    await expect
      .poll(async () => (await ipc<any>(logs!, "logs_info")).stats.records)
      .toBeGreaterThan(5);
    // Preserve reading intent across a real SSH stream EOF and a new observed start.
    await logs.getByRole("button", { name: "暂停滚动", exact: true }).click();
    const scrollBefore = await logs.getByLabel("容器日志内容", {exact:true}).evaluate(el => el.scrollTop);
    const recordsBefore = (await ipc<any>(logs, "logs_info")).stats.records;
    await writeFile(join(qa,"docker-mode"),"stopped");
    await expect(logs.locator(".log-status")).toHaveText("等待容器启动", {timeout:20000});
    await writeFile(join(qa,"docker-started-at"),"2026-10-14T09:10:00.000000000Z");
    await writeFile(join(qa,"docker-mode"),"running");
    await expect(logs.locator(".log-status")).toHaveText("实时接收", {timeout:20000});
    await expect.poll(async()=>(await ipc<any>(logs!,"logs_info")).stats.records).toBeGreaterThan(recordsBefore);
    await expect(logs.locator(".log-reading-status")).toContainText("已暂停滚动");
    expect(Math.abs(await logs.getByLabel("容器日志内容", {exact:true}).evaluate(el=>el.scrollTop)-scrollBefore)).toBeLessThan(4);
    await logs.getByRole("button", {name:/继续跟随/}).click();
    await expect(logs.getByRole("button", {name:"暂停滚动",exact:true})).toBeVisible();
    await logs.setViewportSize({ width: 650, height: 400 });
    await logs.screenshot({ path: ".qa/container-logs-small.png" });
  } finally {
    if (logs && !logs.isClosed()) await closeWindow(logs);
    for (const name of [
      "logs-mode",
      "logs-disconnect",
      "docker-mode",
      "docker-started-at",
      "logs-freeze",
    ])
      await unlink(join(qa, name)).catch(() => {});
    await f.cleanup();
  }
});

async function fixture(label: string): Promise<Fixture> {
  const browser = await chromium.connectOverCDP(
    process.env.SSH_WORKSPACE_QA_CDP_URL || "http://127.0.0.1:9223",
  );
  const context = browser.contexts()[0];
  const main = context.pages().find((page) => {
    if (
      !page
        .url()
        .startsWith(
          process.env.SSH_WORKSPACE_QA_APP_URL || "http://127.0.0.1:1420",
        )
    )
      return false;
    const params = new URL(page.url()).searchParams;
    return (
      !params.has("container") &&
      !params.has("editor") &&
      !params.has("logs") &&
      !params.has("archives") &&
      !params.has("workspace")
    );
  })!;
  expect(
    main,
    "start the desktop app with WebView2 debugging enabled",
  ).toBeTruthy();
  const name = `QA ${label} ${Date.now()}`;
  const server = await ipc<any>(main, "save_server", {
    server: {
      id: "",
      name,
      host: "127.0.0.1",
      port: Number(process.env.SSH_WORKSPACE_FIXTURE_PORT || 22222),
      username: "ops",
      tags: ["QA"],
      authType: "password",
      privateKey: "",
      passwordChange: { type: "set", value: "fixture-password" },
      jumpServerId: null,
      directHost: null,
      directPort: 22,
    },
  });
  await main.reload();
  // Observe a real directory call to discover the backend session handle.
  await main.evaluate(async () => {
    // Vite may append an HMR timestamp; observe the instance the app imported.
    const modulePath =
      performance
        .getEntriesByType("resource")
        .map((entry) => entry.name)
        .find((url) => new URL(url).pathname === "/src/api.ts") ||
      "/src/api.ts";
    const { api } = await import(modulePath);
    const original = api.directory;
    (window as any).__qaFileSession = "";
    api.directory = (sessionId: string, path?: string) => {
      (window as any).__qaFileSession = sessionId;
      return original(sessionId, path);
    };
    const originalChain = api.directoryChain;
    api.directoryChain = (
      sessionId: string,
      path?: string,
      knownPaths?: string[],
    ) => {
      (window as any).__qaFileSession = sessionId;
      return originalChain(sessionId, path, knownPaths);
    };
  });
  await showServers(main);
  await main.locator(".server-row").filter({ hasText: name }).dblclick();
  await authenticate(main);
  await expect
    .poll(() => main.evaluate(() => (window as any).__qaFileSession))
    .not.toBe("");
  const sessionId = (await main.evaluate(
    () => (window as any).__qaFileSession,
  )) as string;
  return {
    browser,
    context,
    main,
    name,
    serverId: server.id,
    sessionId,
    async cleanup() {
      await main.reload();
      await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
      await ipc(main, "delete_server", { id: server.id });
      await main.reload();
      await browser.close();
    },
  };
}

test("server deletion blocks tabs and independent windows, then removes the saved configuration", async () => {
  const f = await fixture("deletion");
  const children: Page[] = [];
  const confirmation = () =>
    f.main.getByRole("dialog", { name: "删除服务器配置？" });
  try {
    await showServers(f.main);
    await serverAction(f.main, "删除服务器");
    await expect(confirmation()).toContainText(f.name);
    await expect(
      confirmation().getByRole("button", { name: "确认删除" }),
    ).toBeDisabled();
    await confirmation()
      .getByRole("button", { name: "取消", exact: true })
      .click();
    await f.main.getByRole("button", { name: "关闭服务器窗口" }).click();
    const editorOpened = f.context.waitForEvent("page");
    const [editor] = await Promise.all([
      editorOpened,
      ipc(f.main, "open_editor", {
        sessionId: f.sessionId,
        path: "/editing/config.yaml",
      }),
    ]);
    children.push(editor);
    await expect(editor.locator(".cm-editor")).toBeVisible();
    const logs = await openLogs(f);
    children.push(logs);
    await ipc(logs, "logs_stop");
    await f.main.getByRole("tablist", { name: "SSH 会话", exact: true }).getByRole("tab").click({ button: "right" });
    const detachedOpened = f.context.waitForEvent("page");
    await f.main
      .getByRole("menuitem", { name: "移到独立窗口", exact: true })
      .click();
    const detached = await detachedOpened;
    children.push(detached);
    await expect(f.main.getByRole("tablist", { name: "SSH 会话", exact: true }).getByRole("tab")).toHaveCount(0);
    await detached.getByRole("button", { name: "断开", exact: true }).click();
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(0);
    await showServers(f.main);
    await f.main
      .locator(".server-row")
      .filter({ hasText: f.name })
      .getByRole("button", { name: "删除配置", exact: true }).click();
    await confirmation().getByRole("button", { name: "确认删除" }).click();
    await expect(confirmation().getByRole("alert")).toContainText("独立会话");
    expect(
      (await ipc<any[]>(f.main, "list_servers")).some(
        (s) => s.id === f.serverId,
      ),
    ).toBe(true);
    await closeWindow(detached);
    await confirmation().getByRole("button", { name: "确认删除" }).click();
    await expect(confirmation().getByRole("alert")).toContainText("日志窗口");
    await closeWindow(logs);
    await confirmation().getByRole("button", { name: "确认删除" }).click();
    await expect(confirmation().getByRole("alert")).toContainText(
      "文件编辑窗口",
    );
    await closeWindow(editor);
    await confirmation().getByRole("button", { name: "确认删除" }).click();
    await expect(confirmation()).not.toBeVisible();
    expect(
      (await ipc<any[]>(f.main, "list_servers")).some(
        (s) => s.id === f.serverId,
      ),
    ).toBe(false);
    await f.main.reload();
    await showServers(f.main);
    await expect(
      f.main.locator(".server-row").filter({ hasText: f.name }),
    ).toHaveCount(0);
  } finally {
    for (const child of children)
      if (!child.isClosed()) await closeWindow(child).catch(() => {});
    await f.cleanup();
  }
});

test("server deletion supports cancellation, direct actions and filtered neighbour selection", async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  const main = browser
    .contexts()[0]
    .pages()
    .find((p) => p.url() === "http://127.0.0.1:1420/")!;
  const prefix = `QA delete ${Date.now()}`;
  const created: any[] = [];
  const confirmation = () =>
    main.getByRole("dialog", { name: "删除服务器配置？" });
  try {
    for (const suffix of ["A", "B", "C"])
      created.push(
        await ipc(main, "save_server", {
          server: {
            id: "",
            name: `${prefix} ${suffix}`,
            host: "127.0.0.1",
            port: 22222,
            username: "ops",
            tags: [prefix],
            authType: "password",
            privateKey: "",
            passwordChange: { type: "set", value: "fixture-password" },
            jumpServerId: null,
            directHost: null,
            directPort: 22,
          },
        }),
      );
    await main.reload();
    await showServers(main);
    await main.getByLabel("按标签筛选").selectOption(prefix);
    await showServers(main);
    await main
      .locator(".server-row")
      .filter({ hasText: `${prefix} B` })
      .click();
    await serverAction(main, "删除服务器");
    await expect(confirmation()).toContainText("127.0.0.1:22222");
    await main.screenshot({ path: ".qa/server-delete-confirmation.png" });
    await confirmation()
      .getByRole("button", { name: "取消", exact: true })
      .click();
    expect(
      (await ipc<any[]>(main, "list_servers")).some(
        (s) => s.id === created[1].id,
      ),
    ).toBe(true);
    await serverAction(main, "删除服务器");
    await confirmation().getByRole("button", { name: "确认删除" }).click();
    await expect(main.locator(".server-row.selected")).toContainText(
      `${prefix} C`,
    );
    await serverAction(main, "编辑");
    const serverEditor = main.getByRole("dialog", { name: "服务器配置", exact: true });
    await expect(serverEditor.getByRole("button", { name: "删除", exact: true })).toHaveCount(0);
    await serverEditor.getByRole("button", { name: "取消", exact: true }).click();
    await serverAction(main, "删除服务器");
    await confirmation().getByRole("button", { name: "确认删除" }).click();
    await expect(main.getByRole("region", { name: "服务器", exact: true })).toBeVisible();
    await expect(main.locator(".server-row.selected")).toContainText(
      `${prefix} A`,
    );
    await showServers(main);
    await serverAction(main, "删除服务器");
    await confirmation().getByRole("button", { name: "确认删除" }).click();
    await expect(main.getByLabel("按标签筛选")).toHaveValue("");
    expect(
      (await ipc<any[]>(main, "list_servers")).some((s) =>
        created.some((c) => c.id === s.id),
      ),
    ).toBe(false);
  } finally {
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    for (const server of created)
      await ipc(main, "delete_server", { id: server.id });
    await main.reload();
    await browser.close();
  }
});

test("remote text editor saves through SFTP, detects conflicts, and keeps the SSH terminal usable", async () => {
  const remotePath = resolve(".qa/remote/editing/config.yaml");
  const original = "# SSH Workspace 编辑测试\nport: 8080\nenabled: true\n";
  await writeFile(remotePath, original);
  const f = await fixture("editor");
  let editor: Page | undefined;
  try {
    await goTo(f.main, "/editing");
    const opened = f.context.waitForEvent("page");
    await f.main
      .locator(".session-page[role=tabpanel]:visible")
      .locator('.file-row[data-path="/editing/config.yaml"]')
      .dblclick();
    editor = await opened;
    await expect(editor.locator(".cm-editor")).toBeVisible();
    expect(editor.url()).toContain("editor=1");
    const first = await ipc<EditorDocument>(editor, "editor_load");
    expect(first.path).toBe("/editing/config.yaml");
    expect(first.text).toBe(original);
    expect(first.serverName).toBe(f.name);
    const count = f.context.pages().length;
    await f.main
      .locator(".session-page[role=tabpanel]:visible")
      .locator('.file-row[data-path="/editing/config.yaml"]')
      .dblclick();
    expect(f.context.pages()).toHaveLength(count);

    await terminal(f.main, "echo editor-independent");
    await expect(
      f.main.locator(".session-page[role=tabpanel]:visible").locator(".terminal-surface"),
    ).toContainText(/editor-independent\s*ops@host/);
    const content = editor.locator(".cm-content");
    await content.click();
    await editor.keyboard.press("ControlOrMeta+A");
    await editor.keyboard.insertText("# 保存验证\nport: 9000\nenabled: true\n");
    await editor.keyboard.press("ControlOrMeta+S");
    await expect
      .poll(() => readFile(remotePath, "utf8"))
      .toBe("# 保存验证\nport: 9000\nenabled: true\n");
    const saved = await ipc<EditorDocument>(editor, "editor_load");
    expect(saved.version).not.toBe(first.version);
    expect(saved.text).toContain("port: 9000");

    await content.click();
    await editor.keyboard.press("ControlOrMeta+A");
    await editor.keyboard.insertText("port: 9100\n");
    await writeFile(remotePath, "port: 9200\n");
    await editor.keyboard.press("ControlOrMeta+S");
    await expect(editor.getByRole("dialog")).toContainText(/修改|变化|冲突/);
    expect(await readFile(remotePath, "utf8")).toBe("port: 9200\n");
    await editor.getByRole("button", { name: "覆盖远端", exact: true }).click();
    await expect.poll(() => readFile(remotePath, "utf8")).toBe("port: 9100\n");
    await closeWindow(editor);
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(1);

    // Content restrictions come from the real remote bytes, not just extensions.
    for (const path of [
      "/editing/binary.dat",
      "/editing/invalid-utf8.txt",
      "/editing/too-large.txt",
    ]) {
      const before = f.context.pages().length;
      const rejection = await f.main.evaluate(
        async ({ sessionId, path }) => {
          try {
            await (window as any).__TAURI_INTERNALS__.invoke("open_editor", {
              sessionId,
              path,
            });
            return null;
          } catch (error) {
            return error;
          }
        },
        { sessionId: f.sessionId, path },
      );
      expect(
        rejection,
        `${path} must be rejected as an editable text document`,
      ).toBeTruthy();
      expect(f.context.pages()).toHaveLength(before);
    }
    await expect(
      f.main.locator(".session-page[role=tabpanel]:visible").locator(".connection-state"),
    ).toHaveText("已连接");
  } finally {
    if (editor && !editor.isClosed()) await closeWindow(editor).catch(() => {});
    await writeFile(remotePath, original);
    await f.cleanup();
  }
});

async function downloadTask(main: Page, id: string) {
  const task = (await ipc<DownloadTask[]>(main, "list_downloads")).find(
    (task) => task.id === id,
  );
  expect(task, `download ${id} must remain observable`).toBeTruthy();
  return task!;
}

test("SFTP downloads preserve directories, report progress, cancel, retry, and respect existing files", async () => {
  // Start the debug app with SSH_WORKSPACE_QA_DOWNLOAD_DIR pointing inside .qa.
  // Only the native chooser is substituted by a process-launch setting; IPC and SFTP remain real.
  const f = await fixture("downloads");
  const taskIds: string[] = [];
  try {
    await unlink(".qa/sftp-read-fail").catch(() => {});
    await unlink(".qa/sftp-read-delay").catch(() => {});
    const previous = new Set(
      (await ipc<DownloadTask[]>(f.main, "list_downloads")).map(
        (task) => task.id,
      ),
    );
    await f.main
      .locator(".session-page[role=tabpanel]:visible")
      .locator('.file-row[data-path="/readme.txt"]')
      .click({ button: "right" });
    await f.main
      .getByRole("menuitem", { name: "下载文件", exact: true })
      .click();
    await f.main
      .getByRole("dialog")
      .getByLabel("同名文件处理")
      .selectOption("rename");
    await f.main
      .getByRole("dialog")
      .getByRole("button", { name: "选择保存位置" })
      .click();
    await expect
      .poll(
        async () =>
          (await ipc<DownloadTask[]>(f.main, "list_downloads")).filter(
            (task) => !previous.has(task.id),
          ).length,
      )
      .toBe(1);
    const first = (await ipc<DownloadTask[]>(f.main, "list_downloads")).find(
      (task) => !previous.has(task.id),
    )!;
    taskIds.push(first.id);
    await expect
      .poll(async () => (await downloadTask(f.main, first.id)).status)
      .toBe("completed");
    const savedFile = await downloadTask(f.main, first.id);
    expect(await readFile(savedFile.localPath)).toEqual(
      await readFile(resolve(".qa/remote/readme.txt")),
    );
    await expect(
      f.main.getByRole("region", { name: "下载任务" }),
    ).toBeVisible();
    await expect(f.main.locator(`[data-task-id="${first.id}"]`)).toContainText(
      "已完成",
    );

    const start = async (
      path: string,
      isDir: boolean,
      conflict: string = "rename",
    ) => {
      const id = await ipc<string>(f.main, "start_download", {
        sessionId: f.sessionId,
        path,
        isDir,
        conflict,
      });
      expect(id).toBeTruthy();
      taskIds.push(id);
      return id;
    };

    const directoryId = await start("/download", true);
    await expect
      .poll(async () => (await downloadTask(f.main, directoryId)).status, {
        timeout: 30000,
      })
      .toBe("completed");
    const directory = await downloadTask(f.main, directoryId);
    expect(directory.filesTotal).toBe(4);
    expect(directory.filesDone).toBe(4);
    expect(directory.bytesDone).toBe(directory.bytesTotal);
    expect(directory.skipped).toBe(2);
    for (const relative of [
      "readme.txt",
      ".hidden",
      join("nested", "中文.txt"),
    ]) {
      expect(await readFile(join(directory.localPath, relative))).toEqual(
        await readFile(resolve(".qa/remote/download", relative)),
      );
    }
    expect(
      (await stat(join(directory.localPath, "nested", "empty"))).isDirectory(),
    ).toBe(true);
    expect((await stat(join(directory.localPath, "slow.bin"))).size).toBe(
      8 * 1024 * 1024,
    );
    await expect(
      stat(join(directory.localPath, "loop-link")),
    ).rejects.toThrow();

    // A same-name file policy must not silently destroy local edits.
    const skipId = await start("/readme.txt", false, "skip");
    await expect
      .poll(async () => (await downloadTask(f.main, skipId)).status)
      .toBe("completed");
    const skipped = await downloadTask(f.main, skipId);
    expect(skipped.skipped).toBe(1);
    await writeFile(skipped.localPath, "keep-local\n");
    const secondSkip = await start("/readme.txt", false, "skip");
    await expect
      .poll(async () => (await downloadTask(f.main, secondSkip)).status)
      .toBe("completed");
    expect(await readFile(skipped.localPath, "utf8")).toBe("keep-local\n");
    const overwriteId = await start("/readme.txt", false, "overwrite");
    await expect
      .poll(async () => (await downloadTask(f.main, overwriteId)).status)
      .toBe("completed");
    expect(await readFile(skipped.localPath)).toEqual(
      await readFile(resolve(".qa/remote/readme.txt")),
    );

    await writeFile(".qa/sftp-read-delay", "0.08");
    const slowId = await start("/download/slow.bin", false);
    await expect
      .poll(async () => (await downloadTask(f.main, slowId)).bytesDone)
      .toBeGreaterThan(0);
    expect((await downloadTask(f.main, slowId)).status).toBe("running");
    await terminal(f.main, "echo during-download");
    await expect(
      f.main.locator(".session-page[role=tabpanel]:visible").locator(".terminal-surface"),
    ).toContainText(/during-download\s*ops@host/);
    await f.main
      .locator(`[data-task-id="${slowId}"]`)
      .getByRole("button", { name: "取消下载" })
      .click();
    await expect
      .poll(async () => (await downloadTask(f.main, slowId)).status)
      .toBe("cancelled");
    await unlink(".qa/sftp-read-delay");
    await f.main
      .locator(`[data-task-id="${slowId}"]`)
      .getByRole("button", { name: "重试下载" })
      .click();
    await expect
      .poll(async () => (await downloadTask(f.main, slowId)).status, {
        timeout: 30000,
      })
      .toBe("completed");
    const completed = await downloadTask(f.main, slowId);
    expect(
      (await readFile(completed.localPath)).equals(
        await readFile(".qa/remote/download/slow.bin"),
      ),
    ).toBe(true);

    await writeFile(".qa/sftp-read-fail", "1");
    const failedId = await start("/download/slow.bin", false);
    await expect
      .poll(async () => (await downloadTask(f.main, failedId)).status)
      .toBe("failed");
    expect((await downloadTask(f.main, failedId)).error).toBeTruthy();
    await unlink(".qa/sftp-read-fail");
    await f.main
      .locator(`[data-task-id="${failedId}"]`)
      .getByRole("button", { name: "重试下载" })
      .click();
    await expect
      .poll(async () => (await downloadTask(f.main, failedId)).status, {
        timeout: 30000,
      })
      .toBe("completed");
    await expect(
      f.main.locator(".session-page[role=tabpanel]:visible").locator(".connection-state"),
    ).toHaveText("已连接");
  } finally {
    await unlink(".qa/sftp-read-delay").catch(() => {});
    await unlink(".qa/sftp-read-fail").catch(() => {});
    for (const id of taskIds)
      await ipc(f.main, "cancel_download", { id }).catch(() => {});
    await f.cleanup();
  }
});

test("tabs reorder, detach and rejoin without replacing their SSH connection or workspace state", async () => {
  const f = await fixture("movable tab");
  let detached: Page | undefined;
  const active = (page: Page) => page.locator(".session-page[role=tabpanel]:visible");
  const title = `${f.name}，已连接`;
  const duplicateTitle = `${f.name} · 2，已连接`;
  const terminalOpens = async () =>
    (await readFile(".qa/ssh-events.jsonl", "utf8"))
      .split("\n")
      .filter(
        (line) =>
          line.includes('"event": "terminal-open"') &&
          line.includes('"target": "host"'),
      ).length;
  try {
    await goTo(f.main, "/projects");
    await openWorkspaceTool(active(f.main), "docker");
    await active(f.main).getByRole("button", { name: /^镜像/ }).click();
    await expect(
      active(f.main).getByText("nginx:alpine", { exact: true }),
    ).toBeVisible();
    await terminal(f.main, "echo before-detach");
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      /before-detach\s*ops@host/,
    );
    const separator = active(f.main).getByRole("separator", {
      name: "调整文件区域宽度",
    });
    const startingWidth = Number(await separator.getAttribute("aria-valuenow"));
    await separator.focus();
    await f.main.keyboard.press("ArrowRight");
    await f.main.keyboard.press("ArrowRight");
    await expect
      .poll(async () => Number(await separator.getAttribute("aria-valuenow")))
      .toBe(startingWidth + 20);
    const resourceWidth = Number(await separator.getAttribute("aria-valuenow"));
    const split = active(f.main).getByRole("separator", {
      name: "调整服务区域宽度",
    });
    const startingServiceWidth = Number(
      await split.getAttribute("aria-valuenow"),
    );
    await split.focus();
    await f.main.keyboard.press("ArrowLeft");
    await expect
      .poll(async () => Number(await split.getAttribute("aria-valuenow")))
      .toBeGreaterThan(startingServiceWidth);
    const serviceWidth = Number(await split.getAttribute("aria-valuenow"));

    await connectSelectedServer(f.main);
    await authenticate(f.main);
    await expect(f.main.getByRole("tablist", { name: "SSH 会话", exact: true }).getByRole("tab")).toHaveCount(2);
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(2);
    const connectionsBefore = await terminalOpens();
    const firstTab = f.main.getByRole("tab", { name: title, exact: true });
    const secondTab = f.main.getByRole("tab", {
      name: duplicateTitle,
      exact: true,
    });
    const secondBounds = await secondTab.boundingBox();
    expect(secondBounds).toBeTruthy();
    await firstTab.dragTo(secondTab, {
      targetPosition: {
        x: secondBounds!.width - 3,
        y: secondBounds!.height / 2,
      },
    });
    await expect(f.main.getByRole("tablist", { name: "SSH 会话", exact: true }).getByRole("tab").first()).toHaveAttribute(
      "aria-label",
      duplicateTitle,
    );
    await firstTab.click({ button: "right" });
    const opened = f.context.waitForEvent("page");
    await f.main
      .getByRole("menuitem", { name: "移到独立窗口", exact: true })
      .click();
    detached = await opened;
    await expect(
      detached.getByRole("tab", { name: title, exact: true }),
    ).toBeVisible();
    expect(detached.url()).toContain("workspace=");
    await expect(f.main.getByRole("tablist", { name: "SSH 会话", exact: true }).getByRole("tab")).toHaveCount(1);
    await expect(active(detached).locator(".connection-state")).toHaveText(
      "已连接",
    );
    await expect(active(detached).locator(".terminal-surface")).toContainText(
      "before-detach",
    );
    await expect(active(detached).getByLabel("远程目录路径")).toHaveValue(
      "/projects",
    );
    await expect(
      active(detached).getByRole("button", { name: /^镜像/ }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(
      active(detached).getByText("nginx:alpine", { exact: true }),
    ).toBeVisible();
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(2);
    expect(await terminalOpens()).toBe(connectionsBefore);
    expect(
      Number(
        await active(detached)
          .getByRole("separator", { name: "调整文件区域宽度" })
          .getAttribute("aria-valuenow"),
      ),
    ).toBeCloseTo(resourceWidth, 0);
    expect(
      Number(
        await active(detached)
          .getByRole("separator", { name: "调整服务区域宽度" })
          .getAttribute("aria-valuenow"),
      ),
    ).toBeCloseTo(serviceWidth, 0);

    await terminal(f.main, "echo main-remains-usable");
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      /main-remains-usable\s*ops@host/,
    );
    await terminal(detached, "echo detached-remains-usable");
    await expect(active(detached).locator(".terminal-surface")).toContainText(
      /detached-remains-usable\s*ops@host/,
    );
    await expect(active(f.main).locator(".terminal-surface")).not.toContainText(
      "detached-remains-usable",
    );
    await terminal(detached, "later output-during-transfer");
    await detached
      .getByRole("tab", { name: title, exact: true })
      .click({ button: "right" });
    await detached
      .getByRole("menuitem", { name: "移回主窗口", exact: true })
      .click();
    await expect(f.main.getByRole("tablist", { name: "SSH 会话", exact: true }).getByRole("tab")).toHaveCount(2);
    await expect.poll(() => detached!.isClosed()).toBe(true);
    await f.main.getByRole("tab", { name: title, exact: true }).click();
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      "before-detach",
    );
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      "detached-remains-usable",
    );
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      /output-during-transfer\s*ops@host/,
    );
    await expect(active(f.main).getByLabel("远程目录路径")).toHaveValue(
      "/projects",
    );
    await expect(
      active(f.main).getByRole("button", { name: /^镜像/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(await terminalOpens()).toBe(connectionsBefore);
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(2);
    await f.main.screenshot({ path: ".qa/movable-tabs-rejoined.png" });
  } finally {
    if (detached && !detached.isClosed())
      await closeWindow(detached).catch(() => {});
    await f.cleanup();
  }
});
