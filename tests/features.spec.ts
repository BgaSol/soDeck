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
    ({ command, args }) =>
      (window as any).__TAURI_INTERNALS__.invoke(command, args),
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
      page.getByRole("tabpanel").locator(".connection-state"),
    ).toHaveText("已连接", { timeout: 1000 });
  }).toPass({ timeout: 20000 });
}

async function terminal(page: Page, command: string) {
  await page
    .getByRole("tabpanel")
    .locator(".terminal-surface textarea")
    .focus();
  await page.keyboard.type(command);
  await page.keyboard.press("Enter");
}

async function goTo(page: Page, path: string) {
  const input = page.getByRole("tabpanel").getByLabel("远程目录路径");
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
  await expect(page.locator(".cm-editor")).toBeVisible();
  return page;
}

async function logEvents() {
  return (await readFile(".qa/ssh-events.jsonl", "utf8"))
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

test("container logs stream independently, reuse windows, reload, search, and survive closing the main tab", async () => {
  await unlink(".qa/logs-mode").catch(() => {});
  await unlink(".qa/logs-disconnect").catch(() => {});
  const f = await fixture("logs");
  let logs: Page | undefined, stopped: Page | undefined;
  const openedBefore = (await logEvents()).filter(
    (e) => e.event === "logs-open",
  ).length;
  const closedBefore = (await logEvents()).filter(
    (e) => e.event === "logs-close",
  ).length;
  try {
    const tree = f.main
      .getByRole("tabpanel")
      .getByRole("button", { name: /^容器/ });
    await expect(tree).toBeVisible();
    if ((await tree.getAttribute("aria-expanded")) !== "true")
      await tree.click();
    const opened = f.context.waitForEvent("page");
    await f.main
      .getByRole("button", { name: "查看 web-fixture 容器日志", exact: true })
      .click();
    logs = await opened;
    await expect(logs.getByRole("status")).toHaveText("实时接收中");
    await expect(logs.locator(".cm-content")).toContainText("中文日志 green");
    await expect(logs.locator(".cm-content")).toContainText("[stderr]");
    await expect(logs.locator(".cm-content")).toContainText("live-");
    const pages = f.context.pages().length;
    await f.main
      .getByRole("button", { name: "查看 web-fixture 容器日志", exact: true })
      .click();
    expect(f.context.pages().length).toBe(pages);
    await terminal(f.main, "echo usable-during-logs");
    await expect(
      f.main.getByRole("tabpanel").locator(".terminal-surface"),
    ).toContainText(/usable-during-logs\s*ops@host/);
    await logs.getByLabel("时间戳", { exact: true }).uncheck();
    await expect(logs.locator(".cm-content")).not.toContainText("2026-10-14T");
    await logs.getByLabel("自动换行", { exact: true }).check();
    await expect(logs.locator(".cm-content")).toHaveClass(/cm-lineWrapping/);
    await logs.getByRole("button", { name: "查找", exact: true }).click();
    await logs
      .locator(".cm-search input[name=search]")
      .pressSequentially("history");
    await expect
      .poll(() => logs!.locator(".cm-searchMatch").count())
      .toBeGreaterThan(0);
    await logs.locator(".cm-search button[name=close]").click();
    await logs.getByLabel("历史日志行数").selectOption("100");
    await logs.getByRole("button", { name: "重新加载", exact: true }).click();
    await expect
      .poll(
        async () =>
          (await logEvents()).filter((e) => e.event === "logs-open").at(-1)
            ?.tail,
      )
      .toBe(100);
    await expect(logs.getByRole("status")).toHaveText("实时接收中");
    await logs.getByRole("button", { name: "跳到最新", exact: true }).click();
    await logs.locator(".cm-content").hover();
    await logs.mouse.wheel(0, -200);
    await expect(
      logs.getByLabel("自动滚动", { exact: true }),
    ).not.toBeChecked();
    const stats = await logs.locator(".log-footer > span").innerText();
    await expect(logs.locator(".log-footer > span")).not.toHaveText(stats);
    await logs.getByRole("button", { name: "跳到最新", exact: true }).click();
    await expect(logs.getByLabel("自动滚动", { exact: true })).toBeChecked();
    await logs.emulateMedia({ colorScheme: "light" });
    await logs.screenshot({ path: ".qa/container-logs-light.png" });
    await logs.emulateMedia({ colorScheme: "dark" });
    await logs.screenshot({ path: ".qa/container-logs-dark.png" });
    const stoppedOpened = f.context.waitForEvent("page");
    await f.main
      .getByRole("button", {
        name: "查看 stopped-fixture 容器日志",
        exact: true,
      })
      .click();
    stopped = await stoppedOpened;
    await expect(stopped.getByRole("status")).toHaveText("日志读取已结束");
    await expect(stopped.locator(".cm-content")).toContainText(
      "fixture history stdout",
    );
    expect(
      (await logEvents()).filter((e) => e.event === "logs-open").at(-1)
        .following,
    ).toBe(false);
    await closeWindow(stopped);
    stopped = undefined;
    await f.main
      .getByRole("button", { name: `关闭 ${f.name} 会话`, exact: true })
      .click();
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(0);
    await expect.poll(() => ipc<number>(f.main, "log_session_count")).toBe(1);
    await expect(logs.getByRole("status")).toHaveText("实时接收中");
    const still = await logs.locator(".log-footer > span").innerText();
    await expect(logs.locator(".log-footer > span")).not.toHaveText(still);
    await closeWindow(logs);
    logs = undefined;
    await expect.poll(() => ipc<number>(f.main, "log_session_count")).toBe(0);
    await expect
      .poll(async () => {
        const events = await logEvents();
        return (
          events.filter((e) => e.event === "logs-open").length -
          openedBefore -
          (events.filter((e) => e.event === "logs-close").length - closedBefore)
        );
      })
      .toBe(0);
  } finally {
    for (const p of [logs, stopped])
      if (p && !p.isClosed()) await closeWindow(p);
    await f.cleanup();
  }
});

test("container logs retain content after disconnect, retry, bound output, and show Docker errors and empty history", async () => {
  await unlink(".qa/logs-mode").catch(() => {});
  await unlink(".qa/logs-disconnect").catch(() => {});
  const f = await fixture("logs-lifecycle");
  let logs: Page | undefined;
  try {
    logs = await openLogs(f);
    await expect(logs.locator(".cm-content")).toContainText("live-");
    await writeFile(".qa/logs-disconnect", "1");
    await expect(logs.getByRole("status")).toHaveText("读取已中断");
    await expect(logs.locator(".cm-content")).toContainText(
      "fixture history stdout",
    );
    await unlink(".qa/logs-disconnect");
    await logs.getByRole("button", { name: "重新连接", exact: true }).click();
    await expect(logs.getByRole("status")).toHaveText("实时接收中");
    await writeFile(".qa/logs-mode", "burst");
    await logs.getByRole("button", { name: "重新加载", exact: true }).click();
    await expect(logs.locator(".log-footer > span")).toContainText(
      "已保留 10000 行",
    );
    await expect(logs.locator(".log-footer > span")).toContainText(
      "已移除最旧内容",
    );
    await expect(logs.locator(".cm-content")).toContainText("burst-11999");
    await expect(logs.getByLabel("自动滚动", { exact: true })).toBeChecked();
    await logs.setViewportSize({ width: 650, height: 400 });
    await logs.screenshot({ path: ".qa/container-logs-small.png" });
    await expect
      .poll(() =>
        logs!.locator(".cm-scroller").evaluate((node) => node.scrollLeft),
      )
      .toBe(0);
    await writeFile(".qa/logs-mode", "unsupported");
    await logs.getByRole("button", { name: "重新加载", exact: true }).click();
    await expect(logs.getByRole("alert")).toContainText(
      "logging driver does not support",
    );
    await expect(logs.getByRole("status")).toHaveText("读取已中断");
    await closeWindow(logs);
    logs = undefined;
    await writeFile(".qa/logs-mode", "empty");
    logs = await openLogs(f, "b".repeat(64), "stopped-fixture");
    await expect(logs.getByText("容器暂无日志", { exact: true })).toBeVisible();
    await closeWindow(logs);
    logs = undefined;
    await unlink(".qa/logs-mode");
    await writeFile(".qa/docker-mode", "tty");
    logs = await openLogs(f);
    await expect(logs.locator(".log-footer > span")).toContainText(
      "容器 TTY 输出已合并",
    );
    await expect(logs.locator(".cm-content")).toContainText(
      "fixture history stderr",
    );
    await expect(logs.locator(".cm-content")).not.toContainText("[stderr]");
  } finally {
    if (logs && !logs.isClosed()) await closeWindow(logs);
    await unlink(".qa/logs-mode").catch(() => {});
    await unlink(".qa/logs-disconnect").catch(() => {});
    await unlink(".qa/docker-mode").catch(() => {});
    await f.cleanup();
  }
});

async function fixture(label: string): Promise<Fixture> {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  const context = browser.contexts()[0];
  const main = context.pages().find((page) => {
    if (!page.url().startsWith("http://127.0.0.1:1420")) return false;
    const params = new URL(page.url()).searchParams;
    return (
      !params.has("container") &&
      !params.has("editor") &&
      !params.has("logs") &&
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
      port: 22222,
      username: "ops",
      tags: ["QA"],
      authType: "password",
      privateKey: "",
      password: "fixture-password",
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
  });
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
      await ipc(main, "delete_server", { id: server.id });
      await main.reload();
      await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
      await browser.close();
    },
  };
}

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
      .getByRole("tabpanel")
      .getByRole("button", { name: /config.yaml/ })
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
      .getByRole("tabpanel")
      .getByRole("button", { name: /config.yaml/ })
      .dblclick();
    expect(f.context.pages()).toHaveLength(count);

    await terminal(f.main, "echo editor-independent");
    await expect(
      f.main.getByRole("tabpanel").locator(".terminal-surface"),
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
      f.main.getByRole("tabpanel").locator(".connection-state"),
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
      .getByRole("tabpanel")
      .getByRole("button", { name: /readme.txt/ })
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
      f.main.getByRole("tabpanel").locator(".terminal-surface"),
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
      f.main.getByRole("tabpanel").locator(".connection-state"),
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
  const active = (page: Page) => page.getByRole("tabpanel");
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
    await active(f.main).getByRole("button", { name: /^镜像/ }).click();
    await expect(
      active(f.main).getByText("nginx:alpine", { exact: true }),
    ).toBeVisible();
    await terminal(f.main, "echo before-detach");
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      /before-detach\s*ops@host/,
    );
    const separator = active(f.main).getByRole("separator", {
      name: /文件和服务区域宽度|左侧宽度/,
    });
    const startingWidth = Number(await separator.getAttribute("aria-valuenow"));
    await separator.focus();
    await f.main.keyboard.press("ArrowRight");
    await f.main.keyboard.press("ArrowRight");
    await expect
      .poll(async () => Number(await separator.getAttribute("aria-valuenow")))
      .toBe(startingWidth + 20);
    const resourceWidth = Number(await separator.getAttribute("aria-valuenow"));
    const split = active(f.main).locator(".horizontal-resizer");
    const startingRatio = Number(await split.getAttribute("aria-valuenow"));
    await split.focus();
    await f.main.keyboard.press("ArrowDown");
    await expect
      .poll(async () => Number(await split.getAttribute("aria-valuenow")))
      .toBeGreaterThan(startingRatio);
    const fileRatio = Number(await split.getAttribute("aria-valuenow"));

    await f.main.getByRole("button", { name: "新建连接", exact: true }).click();
    await authenticate(f.main);
    await expect(f.main.getByRole("tab")).toHaveCount(2);
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
    await expect(f.main.getByRole("tab").first()).toHaveAttribute(
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
    await expect(f.main.getByRole("tab")).toHaveCount(1);
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
    ).toHaveAttribute("aria-expanded", "true");
    await expect(
      active(detached).getByText("nginx:alpine", { exact: true }),
    ).toBeVisible();
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(2);
    expect(await terminalOpens()).toBe(connectionsBefore);
    expect(
      Number(
        await active(detached)
          .getByRole("separator", { name: /文件和服务区域宽度|左侧宽度/ })
          .getAttribute("aria-valuenow"),
      ),
    ).toBeCloseTo(resourceWidth, 0);
    expect(
      Number(
        await active(detached)
          .locator(".horizontal-resizer")
          .getAttribute("aria-valuenow"),
      ),
    ).toBeCloseTo(fileRatio, 0);

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
    await expect(f.main.getByRole("tab")).toHaveCount(2);
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
    ).toHaveAttribute("aria-expanded", "true");
    expect(await terminalOpens()).toBe(connectionsBefore);
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(2);
    await f.main.screenshot({ path: ".qa/movable-tabs-rejoined.png" });
  } finally {
    if (detached && !detached.isClosed())
      await closeWindow(detached).catch(() => {});
    await f.cleanup();
  }
});
