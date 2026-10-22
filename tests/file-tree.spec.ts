import { showServers, connectSelectedServer } from "./server-picker";
import {
  chromium,
  expect,
  test,
  type Browser,
  type BrowserContext,
  type Page,
} from "@playwright/test";
import { readFile, unlink, writeFile } from "node:fs/promises";
import { join, relative, resolve } from "node:path";
import type { DownloadTask, EditorDocument } from "../src/fileApi";
import type { ToolContextRef } from "../src/toolContext";

// Real Tauri + SSH/SFTP on a separate fixture and WebView profile. The suite only
// observes protocol calls; it does not replace backend or shell responses.
const qa = process.env.SSH_WORKSPACE_FIXTURE_DIR || ".qa/tree-fixture";
const cdp = process.env.SSH_WORKSPACE_QA_CDP_URL || "http://127.0.0.1:9225";
const port = Number(process.env.SSH_WORKSPACE_FIXTURE_PORT || 22225);
const home = process.env.SSH_WORKSPACE_FIXTURE_HOME || "/home/ops";

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

const active = (page: Page) =>
  page.locator(".session-page[role=tabpanel]:visible");
const node = (page: Page, path: string) =>
  active(page).locator(`[role="treeitem"][data-path=${JSON.stringify(path)}]`);

async function authenticate(page: Page, expectedDirectory = home) {
  await expect(async () => {
    const trust = page.getByRole("button", { name: "确认指纹并连接" });
    if (await trust.isVisible()) await trust.click();
    await expect(active(page).locator(".connection-state")).toHaveText(
      "已连接",
      { timeout: 1000 },
    );
  }).toPass({ timeout: 20000 });
  await expect(
    active(page).getByRole("tree", { name: "远程文件树" }),
  ).toBeVisible();
  await expect(active(page).getByLabel("远程目录路径")).toHaveValue(
    expectedDirectory,
  );
  await expect(node(page, expectedDirectory)).toHaveAttribute(
    "aria-selected",
    "true",
  );
}

async function command(page: Page, value: string) {
  await active(page)
    .locator(".terminal-session:visible .terminal-surface textarea")
    .focus();
  await page.keyboard.insertText(value);
  await page.keyboard.press("Enter");
}

async function locate(page: Page, path: string) {
  const button = page.getByRole("button", {
    name: "定位到当前终端目录",
    exact: true,
  });
  await expect(button).toHaveAttribute("title", path);
  await button.click();
}

async function atDirectory(page: Page, path: string) {
  await expect(active(page).getByLabel("远程目录路径")).toHaveValue(path);
  await expect(node(page, path)).toHaveAttribute("aria-expanded", "true");
  await expect(node(page, path)).toHaveAttribute("aria-selected", "true");
}

async function events() {
  return (await readFile(join(qa, "ssh-events.jsonl"), "utf8"))
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

async function listCount(path: string) {
  return (await events()).filter(
    (event) => event.event === "sftp-list" && event.path === path,
  ).length;
}

async function terminalCount() {
  return (await events()).filter(
    (event) => event.event === "terminal-open" && event.target === "host",
  ).length;
}

interface Fixture {
  browser: Browser;
  context: BrowserContext;
  main: Page;
  name: string;
  contextRef: ToolContextRef;
  cleanup: () => Promise<void>;
}

async function fixture(label: string): Promise<Fixture> {
  const browser = await chromium.connectOverCDP(cdp);
  const context = browser.contexts()[0];
  const main = context.pages().find((page) => {
    if (
      !page.url().startsWith("http://127.0.0.1:") &&
      !page.url().startsWith("http://tauri.localhost")
    )
      return false;
    const params = new URL(page.url()).searchParams;
    return !["container", "editor", "logs", "workspace"].some((key) =>
      params.has(key),
    );
  })!;
  expect(main, `start the isolated tree QA app at ${cdp}`).toBeTruthy();
  const errors: string[] = [];
  const captureError = (error: Error) => errors.push(error.message);
  const observePage = (page: Page) => page.on("pageerror", captureError);
  main.on("pageerror", captureError);
  context.on("page", observePage);
  const name = `QA file tree ${label} ${Date.now()}`;
  const server = await ipc<any>(main, "save_server", {
    server: {
      id: "",
      name,
      host: "127.0.0.1",
      port,
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
  const cleanup = async () => {
    try {
      await main.reload();
      await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
      await ipc(main, "delete_server", { id: server.id });
      await main.reload();
    } finally {
      context.off("page", observePage);
      for (const page of context.pages()) page.off("pageerror", captureError);
      await browser.close();
    }
    expect(errors).toEqual([]);
  };
  try {
    await main.reload();
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: name }).dblclick();
    await authenticate(main);
    const contextId = await active(main)
      .getByRole("region", { name: "远程文件" })
      .getAttribute("data-context-id");
    expect(contextId).toBeTruthy();
    const contextRef = {
      id: contextId!,
      epoch: Number(contextId!.split(".").at(-1)),
    };
    return { browser, context, main, name, contextRef, cleanup };
  } catch (error) {
    await cleanup().catch(() => {});
    throw error;
  }
}

async function closeWindow(page: Page) {
  if (page.isClosed()) return;
  const closed = page.waitForEvent("close");
  await page
    .evaluate(() => {
      const internals = (window as any).__TAURI_INTERNALS__;
      return internals.invoke("plugin:window|destroy", {
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
  await closed;
}

test("remote tree loads only root-to-home, and expanding cached or empty folders stays local", async () => {
  const before = (await events()).length;
  const f = await fixture("lazy loading");
  try {
    const listed = (await events())
      .slice(before)
      .filter((event) => event.event === "sftp-list")
      .map((event) => event.path);
    expect([...new Set(listed)].sort()).toEqual(["/", "/home", home].sort());
    for (const path of ["/", "/home", home]) {
      await expect(node(f.main, path)).toHaveAttribute("aria-expanded", "true");
    }
    for (const path of ["/projects", "/var", `${home}/projects`, "/empty"]) {
      await expect(node(f.main, path)).toHaveAttribute(
        "aria-expanded",
        "false",
      );
    }
    await expect(node(f.main, `${home}/projects/home-note.txt`)).toHaveCount(0);
    const split = active(f.main).getByRole("separator", {
      name: "调整文件区域宽度",
    });
    await split.focus();
    for (let step = 0; step < 10; step++)
      await f.main.keyboard.press("ArrowRight");
    await active(f.main)
      .getByRole("tree", { name: "远程文件树" })
      .evaluate((tree) => {
        tree.scrollTop = 0;
      });
    await f.main.screenshot({ path: ".qa/tree-connected.png" });
    for (let step = 0; step < 10; step++)
      await f.main.keyboard.press("ArrowLeft");
    const varBefore = await listCount("/var"),
      logBefore = await listCount("/var/log");
    await node(f.main, "/var").click();
    await expect(node(f.main, "/var/log")).toBeVisible();
    await expect.poll(() => listCount("/var")).toBe(varBefore + 1);
    expect(await listCount("/var/log")).toBe(logBefore);
    await node(f.main, "/var/log").click();
    await expect(node(f.main, "/var/log/app.log")).toBeVisible();
    const afterLoad = await listCount("/var/log");
    await node(f.main, "/var/log").click();
    await expect(node(f.main, "/var/log/app.log")).toHaveCount(0);
    await node(f.main, "/var/log").click();
    await expect(node(f.main, "/var/log/app.log")).toBeVisible();
    expect(await listCount("/var/log")).toBe(afterLoad);
    await node(f.main, "/empty").click();
    await expect(node(f.main, "/empty")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await expect(
      active(f.main).getByText("目录为空", { exact: true }),
    ).toBeVisible();
    const emptyReads = await listCount("/empty");
    await node(f.main, "/empty").click();
    await node(f.main, "/empty").click();
    await expect(node(f.main, "/empty")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(await listCount("/empty")).toBe(emptyReads);
    await f.main.screenshot({ path: ".qa/file-tree-lazy.png" });
  } finally {
    await f.cleanup();
  }
});

test("symlink descendants retain their logical paths, and readable descendants survive denied ancestor listings", async () => {
  const f = await fixture("links and permissions");
  try {
    const root = await ipc<any>(f.main, "list_directory", {
      context: f.contextRef,
      path: "/",
    });
    expect(
      root.entries.find((entry: any) => entry.path === "/tree-link"),
    ).toMatchObject({ isDir: true, isSymlink: true });
    const link = await ipc<any>(f.main, "list_directory", {
      context: f.contextRef,
      path: "/tree-link",
    });
    expect(link.path).toBe("/tree-link");
    expect(link.resolvedPath).toBe("/var");
    expect(link.entries.map((entry: any) => entry.path)).toContain(
      "/tree-link/log",
    );
    const linkedChild = await ipc<any>(f.main, "list_directory", {
      context: f.contextRef,
      path: "/tree-link/log",
    });
    expect(linkedChild.resolvedPath).toBe("/var/log");
    expect(linkedChild.entries[0].path).toBe("/tree-link/log/app.log");
    await node(f.main, "/tree-link").click();
    await expect(node(f.main, "/tree-link/log")).toBeVisible();
    await node(f.main, "/tree-link/log").click();
    await expect(node(f.main, "/tree-link/log/app.log")).toBeVisible();
    await f.main.screenshot({ path: ".qa/tree-symlink.png" });
    const chain = await ipc<any>(f.main, "list_directory_chain", {
      context: f.contextRef,
      path: "/traverse-only/leaf",
      knownPaths: ["/"],
    });
    expect(chain.directories[0]).toMatchObject({ path: "/traverse-only" });
    expect(chain.directories[0].error).toBeTruthy();
    expect(chain.directories[0].directory).toBeUndefined();
    expect(chain.directories[1].directory.entries[0].path).toBe(
      "/traverse-only/leaf/visible.txt",
    );
    await command(f.main, "cd /traverse-only/leaf");
    await locate(f.main, "/traverse-only/leaf");
    await atDirectory(f.main, "/traverse-only/leaf");
    await expect(node(f.main, "/traverse-only")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await expect(node(f.main, "/traverse-only/leaf/visible.txt")).toBeVisible();
    await expect(active(f.main).locator(".file-tree-error")).toHaveCount(1);
    await f.main.screenshot({ path: ".qa/tree-permissions.png" });
  } finally {
    await f.cleanup();
  }
});

test("shell CWD is a marker until explicit locate, and late reads do not replace a newer location", async () => {
  const f = await fixture("explicit shell locate");
  try {
    for (const path of [
      home + "/projects",
      home,
      "/var/log",
      home,
      "/中文 空格/子目录",
    ]) {
      await command(f.main, "cd '" + path + "'");
      const previous = await active(f.main)
        .getByLabel("远程目录路径")
        .inputValue();
      await expect(
        f.main.getByRole("button", { name: "定位到当前终端目录" }),
      ).toHaveAttribute("title", path);
      await expect(active(f.main).getByLabel("远程目录路径")).toHaveValue(
        previous,
      );
      await locate(f.main, path);
      await atDirectory(f.main, path);
    }
    await command(f.main, "cd /does-not-exist");
    await expect(
      active(f.main).locator(".terminal-session:visible .terminal-surface"),
    ).toContainText("No such file or directory");
    await atDirectory(f.main, "/中文 空格/子目录");
    await command(f.main, "cd ~");
    await locate(f.main, home);
    await atDirectory(f.main, home);
    await writeFile(join(qa, "sftp-delay"), "1");
    const pendingBefore = await listCount("/projects");
    await command(f.main, "cd /projects");
    await locate(f.main, "/projects");
    await expect(node(f.main, "/projects")).toHaveAttribute(
      "aria-busy",
      "true",
    );
    await command(f.main, "cd /var/log");
    await locate(f.main, "/var/log");
    await atDirectory(f.main, "/var/log");
    await expect
      .poll(() => listCount("/projects"))
      .toBeGreaterThan(pendingBefore);
    await expect(node(f.main, "/projects")).not.toHaveAttribute(
      "aria-busy",
      "true",
    );
    await atDirectory(f.main, "/var/log");
    await f.main.screenshot({ path: ".qa/file-tree-terminal-locate.png" });
  } finally {
    await unlink(join(qa, "sftp-delay")).catch(() => {});
    await f.cleanup();
  }
});

test("shared tree and both terminal CWDs survive collective native window transfers", async () => {
  const f = await fixture("window migration");
  let detached: Page | undefined;
  try {
    await command(f.main, "cd projects");
    await locate(f.main, home + "/projects");
    await atDirectory(f.main, home + "/projects");
    await node(f.main, "/var").click();
    await expect(node(f.main, "/var/log")).toBeVisible();
    await command(f.main, "cd '/中文 空格/子目录'");
    await locate(f.main, "/中文 空格/子目录");
    await atDirectory(f.main, "/中文 空格/子目录");
    await f.main.getByRole("button", { name: "新建终端", exact: true }).click();
    await expect(
      active(f.main).locator(".terminal-session:visible .connection-state"),
    ).toHaveText("已连接");
    await expect(f.main.locator(".session-tab")).toHaveCount(1);
    await expect(
      f.main.getByRole("tablist", { name: "终端标签" }).getByRole("tab"),
    ).toHaveCount(2);
    await expect
      .poll(
        async () =>
          (await ipc<any>(f.main, "list_server_workspaces")).workspaces
            .find((w: any) => w.server.name === f.name)
            ?.terminals.filter((t: any) => t.state === "connected").length,
      )
      .toBe(2);
    await atDirectory(f.main, "/中文 空格/子目录");
    await expect(node(f.main, "/var")).toHaveAttribute("aria-expanded", "true");
    const connections = await terminalCount();
    const opened = f.context.waitForEvent("page");
    await f.main
      .locator(".tab-select[aria-selected=true]")
      .click({ button: "right" });
    await f.main
      .getByRole("menuitem", { name: "移到独立窗口", exact: true })
      .click();
    detached = await opened;
    await detached.waitForURL(/workspace=/);
    await expect
      .poll(
        async () =>
          (await ipc<any>(detached!, "list_server_workspaces")).workspaces.find(
            (w: any) => w.server.name === f.name,
          )?.lifecycle,
      )
      .toBe("active");
    await expect(
      active(detached).locator(".terminal-session:visible .connection-state"),
    ).toHaveText("已连接");
    await expect(
      detached.getByRole("tablist", { name: "终端标签" }).getByRole("tab"),
    ).toHaveCount(2);
    await atDirectory(detached, "/中文 空格/子目录");
    await expect(node(detached, home + "/projects")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await expect(node(detached, "/var")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(await terminalCount()).toBe(connections);
    await detached.getByRole("tab", { name: "终端 1", exact: true }).click();
    await expect(
      detached.getByRole("button", { name: "定位到当前终端目录" }),
    ).toHaveAttribute("title", "/中文 空格/子目录");
    await detached.getByRole("tab", { name: "终端 2", exact: true }).click();
    await command(detached, "cd /var/log");
    await locate(detached, "/var/log");
    await atDirectory(detached, "/var/log");
    await detached
      .locator(".tab-select[aria-selected=true]")
      .click({ button: "right" });
    await detached
      .getByRole("menuitem", { name: "移回主窗口", exact: true })
      .click();
    await expect.poll(() => detached!.isClosed()).toBe(true);
    await expect(f.main.locator(".session-tab")).toHaveCount(1);
    await atDirectory(f.main, "/var/log");
    await expect(node(f.main, home + "/projects")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(await terminalCount()).toBe(connections);
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(2);
    await f.main.getByRole("tab", { name: "终端 1", exact: true }).click();
    await atDirectory(f.main, "/var/log");
    await expect(
      f.main.getByRole("button", { name: "定位到当前终端目录" }),
    ).toHaveAttribute("title", "/中文 空格/子目录");
    await f.main.screenshot({ path: ".qa/file-tree-migration.png" });
  } finally {
    if (detached && !detached.isClosed())
      await closeWindow(detached).catch(() => {});
    await f.cleanup();
  }
});

test("tree files still open the remote editor and download actual file and directory bytes", async () => {
  const f = await fixture("editor and download");
  let editor: Page | undefined;
  try {
    const address = active(f.main).getByLabel("远程目录路径");
    await address.fill("/editing");
    await address.press("Enter");
    await atDirectory(f.main, "/editing");
    await expect(node(f.main, "/editing/config.yaml")).toBeVisible();
    await command(f.main, "pwd");
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      home,
    );
    // Reading the same shell cwd again must keep a manually browsed directory.
    await atDirectory(f.main, "/editing");
    const opened = f.context.waitForEvent("page");
    await node(f.main, "/editing/config.yaml").dblclick();
    editor = await opened;
    await expect(editor.locator(".cm-editor")).toBeVisible();
    const document = await ipc<EditorDocument>(editor, "editor_load");
    expect(document.path).toBe("/editing/config.yaml");
    expect(document.text).toBe(
      await readFile(join(qa, "remote", "editing", "config.yaml"), "utf8"),
    );
    expect(document.serverName).toBe(f.name);
    await closeWindow(editor);
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(1);

    const download = async (path: string, isDir: boolean) => {
      const previous = new Set(
        (await ipc<DownloadTask[]>(f.main, "list_downloads")).map(
          (task) => task.id,
        ),
      );
      if (isDir) {
        await node(f.main, path).click({ button: "right" });
        await f.main
          .getByRole("menuitem", { name: "下载文件夹", exact: true })
          .click();
      } else {
        await node(f.main, path).click({ button: "right" });
        await f.main
          .getByRole("menuitem", { name: "下载文件", exact: true })
          .click();
      }
      const dialog = f.main.getByRole("dialog");
      await dialog.getByLabel("同名文件处理").selectOption("rename");
      await dialog
        .getByRole("button", { name: "选择保存位置", exact: true })
        .click();
      await expect
        .poll(
          async () =>
            (await ipc<DownloadTask[]>(f.main, "list_downloads")).filter(
              (task) => !previous.has(task.id),
            ).length,
        )
        .toBe(1);
      const task = (await ipc<DownloadTask[]>(f.main, "list_downloads")).find(
        (task) => !previous.has(task.id),
      )!;
      const latest = async () =>
        (await ipc<DownloadTask[]>(f.main, "list_downloads")).find(
          (item) => item.id === task.id,
        )!;
      await expect.poll(async () => (await latest()).status).toBe("completed");
      const result = await latest();
      expect(result.remotePath).toBe(path);
      expect(result.isDir).toBe(isDir);
      expect(result.bytesDone).toBe(result.bytesTotal);
      expect(
        relative(resolve(".qa/tree-download-test"), resolve(result.localPath)),
      ).not.toMatch(/^\.\./);
      return result;
    };
    const file = await download("/readme.txt", false);
    expect(await readFile(file.localPath)).toEqual(
      await readFile(join(qa, "remote", "readme.txt")),
    );
    const directory = await download("/projects", true);
    expect(directory.filesDone).toBe(1);
    expect(await readFile(join(directory.localPath, "hello.txt"))).toEqual(
      await readFile(join(qa, "remote", "projects", "hello.txt")),
    );
    await expect(node(f.main, "/editing/config.yaml")).toBeVisible();
    await expect(
      f.main.getByRole("button", { name: "查看文件任务", exact: true }),
    ).toBeVisible();
    await command(f.main, "echo tree-editor-download-usable");
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      /tree-editor-download-usable\s*ops@host/,
    );
    await f.main.screenshot({ path: ".qa/tree-editor-and-download.png" });
  } finally {
    if (editor && !editor.isClosed()) await closeWindow(editor).catch(() => {});
    await f.cleanup();
  }
});

test("unsupported or failed shell probes keep tools usable, and reconnect restores explicit CWD locate", async () => {
  for (const mode of ["unsupported", "fail"]) {
    await writeFile(join(qa, "shell-probe-mode"), mode);
    const before = (await events()).length;
    let f: Fixture | undefined;
    try {
      f = await fixture(`probe ${mode}`);
      const opened = (await events()).slice(before);
      expect(
        opened.filter(
          (event) => event.event === "terminal-open" && event.target === "host",
        ),
      ).toHaveLength(1);
      expect(
        opened.some(
          (event) =>
            event.event === "exec" &&
            event.command.includes("__SSH_WORKSPACE_PROBE__"),
        ),
      ).toBe(true);
      expect(
        opened.some(
          (event) =>
            event.event === "exec" &&
            event.command.includes("__SSH_WORKSPACE_BOOTSTRAP__"),
        ),
      ).toBe(false);
      await command(f.main, "cd /var/log");
      await command(f.main, `echo fallback-${mode}-usable`);
      await expect(active(f.main).locator(".terminal-surface")).toContainText(
        `fallback-${mode}-usable`,
      );
      await atDirectory(f.main, home);
      const address = active(f.main).getByLabel("远程目录路径");
      await address.fill("/var/log");
      await address.press("Enter");
      await atDirectory(f.main, "/var/log");
      await expect(node(f.main, "/var/log/app.log")).toBeVisible();
      await unlink(join(qa, "shell-probe-mode"));
      await active(f.main)
        .getByRole("button", { name: "断开", exact: true })
        .click();
      await expect.poll(() => ipc<number>(f!.main, "session_count")).toBe(0);
      await active(f.main)
        .getByRole("button", { name: "重新连接", exact: true })
        .click();
      await authenticate(f.main, "/var/log");
      await expect(
        f.main
          .getByRole("tablist", { name: "服务器标签", exact: true })
          .getByRole("tab"),
      ).toHaveCount(1);
      await command(f.main, "cd projects");
      await locate(f.main, home + "/projects");
      await atDirectory(f.main, `${home}/projects`);
      await expect(
        node(f.main, `${home}/projects/home-note.txt`),
      ).toBeVisible();
    } finally {
      await unlink(join(qa, "shell-probe-mode")).catch(() => {});
      await f?.cleanup();
    }
  }
});

test("download keyboard ownership and theme preferences in native WebView", async () => {
  const f = await fixture("ui refinement");
  try {
    await active(f.main).getByLabel("远程目录路径").fill("/");
    await active(f.main).getByLabel("远程目录路径").press("Enter");
    const file = node(f.main, "/readme.txt");
    await expect(file).toBeVisible();
    await file.focus();
    await file.press("Shift+F10");
    const item = f.main.getByRole("menuitem", { name: /下载/ });
    await item.focus();
    await item.press("Enter");
    const dialog = f.main.getByRole("dialog", {
        name: "下载文件",
        exact: true,
      }),
      select = dialog.getByLabel("同名文件处理");
    await expect(select).toBeFocused();
    await expect(f.main.getByRole("menu", { name: "文件操作" })).toBeHidden();
    await select.press("ArrowDown");
    await expect(select).toHaveValue("skip");
    await select.press("Tab");
    await f.main.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(file).toBeFocused();
    await f.main.getByRole("button", { name: "外观主题", exact: true }).click();
    await f.main.getByRole("menuitemradio", { name: "深色主题" }).click();
    await expect(f.main.locator("html")).toHaveAttribute("data-theme", "dark");
    await f.main.getByRole("button", { name: "外观主题", exact: true }).click();
    await f.main.getByRole("menuitemradio", { name: "跟随系统" }).click();
    expect(
      await f.main.evaluate(() => localStorage.getItem("sodeck.theme")),
    ).toBeNull();
    await f.main.screenshot({ path: ".qa/ui-refinement-native.png" });
  } finally {
    await f.cleanup();
  }
});
