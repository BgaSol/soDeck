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

const active = (page: Page) => page.getByRole("tabpanel");
const node = (page: Page, path: string) =>
  active(page).locator(`[role="treeitem"][data-path=${JSON.stringify(path)}]`);

async function authenticate(page: Page) {
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
  await expect(active(page).getByLabel("远程目录路径")).toHaveValue(home);
  await expect(node(page, home)).toHaveAttribute("aria-selected", "true");
}

async function command(page: Page, value: string) {
  await active(page).locator(".terminal-surface textarea").focus();
  await page.keyboard.insertText(value);
  await page.keyboard.press("Enter");
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
  sessionId: string;
  cleanup: () => Promise<void>;
}

async function fixture(label: string): Promise<Fixture> {
  const browser = await chromium.connectOverCDP(cdp);
  const context = browser.contexts()[0];
  const main = context.pages().find((page) => {
    if (
      !page.url().startsWith("http://127.0.0.1:1420") &&
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
    await main.locator(".server-row").filter({ hasText: name }).dblclick();
    await authenticate(main);
    const sessionId = await active(main)
      .getByRole("region", { name: "远程文件" })
      .getAttribute("data-session-id");
    expect(sessionId).toBeTruthy();
    return { browser, context, main, name, sessionId: sessionId!, cleanup };
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
    const split = active(f.main).locator(".horizontal-resizer");
    await split.focus();
    for (let step = 0; step < 10; step++)
      await f.main.keyboard.press("ArrowDown");
    await active(f.main)
      .getByRole("tree", { name: "远程文件树" })
      .evaluate((tree) => {
        tree.scrollTop = 0;
      });
    await f.main.screenshot({ path: ".qa/tree-connected.png" });
    for (let step = 0; step < 10; step++)
      await f.main.keyboard.press("ArrowUp");
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
      sessionId: f.sessionId,
      path: "/",
    });
    expect(
      root.entries.find((entry: any) => entry.path === "/tree-link"),
    ).toMatchObject({ isDir: true, isSymlink: true });
    const link = await ipc<any>(f.main, "list_directory", {
      sessionId: f.sessionId,
      path: "/tree-link",
    });
    expect(link.path).toBe("/tree-link");
    expect(link.resolvedPath).toBe("/var");
    expect(link.entries.map((entry: any) => entry.path)).toContain(
      "/tree-link/log",
    );
    const linkedChild = await ipc<any>(f.main, "list_directory", {
      sessionId: f.sessionId,
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
      sessionId: f.sessionId,
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

test("successful shell cwd changes reveal ancestors; failed cd and late reads do not move the selection", async () => {
  const f = await fixture("shell follow");
  try {
    await command(f.main, "cd projects");
    await atDirectory(f.main, `${home}/projects`);
    await expect(node(f.main, `${home}/projects/home-note.txt`)).toBeVisible();
    await expect(
      active(f.main).locator(".terminal-surface textarea"),
    ).toBeFocused();
    await command(f.main, "cd ..");
    await atDirectory(f.main, home);
    await command(f.main, "cd /var/log");
    await atDirectory(f.main, "/var/log");
    await expect(node(f.main, "/var")).toHaveAttribute("aria-expanded", "true");
    await command(f.main, "cd -");
    await atDirectory(f.main, home);
    await command(f.main, "cd '/中文 空格/子目录'");
    await atDirectory(f.main, "/中文 空格/子目录");
    await expect(node(f.main, "/中文 空格/子目录/中文.txt")).toBeVisible();
    await command(f.main, "cd /does-not-exist");
    await expect(active(f.main).locator(".terminal-surface")).toContainText(
      "No such file or directory",
    );
    await atDirectory(f.main, "/中文 空格/子目录");
    await command(f.main, "cd ~");
    await atDirectory(f.main, home);
    await writeFile(join(qa, "sftp-delay"), "1");
    const pendingBefore = await listCount("/projects");
    const requestsBefore = (await events()).filter(
      (event) =>
        event.event === "sftp-list-start" && event.path === "/projects",
    ).length;
    await command(f.main, "cd /projects");
    await expect
      .poll(
        async () =>
          (await events()).filter(
            (event) =>
              event.event === "sftp-list-start" && event.path === "/projects",
          ).length,
      )
      .toBeGreaterThan(requestsBefore);
    await expect(node(f.main, "/projects")).toHaveAttribute(
      "aria-busy",
      "true",
    );
    await command(f.main, "cd /var/log");
    await atDirectory(f.main, "/var/log");
    await expect
      .poll(() => listCount("/projects"))
      .toBeGreaterThan(pendingBefore);
    await expect(node(f.main, "/projects")).not.toHaveAttribute(
      "aria-busy",
      "true",
    );
    await atDirectory(f.main, "/var/log");
    await expect(node(f.main, `${home}/projects`)).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await f.main.screenshot({ path: ".qa/file-tree-terminal-follow.png" });
  } finally {
    await unlink(join(qa, "sftp-delay")).catch(() => {});
    await f.cleanup();
  }
});

test("tree expansion and cwd remain per-session through detached window transfers", async () => {
  const f = await fixture("window migration");
  let detached: Page | undefined;
  const title = `${f.name}，已连接`,
    duplicateTitle = `${f.name} · 2，已连接`;
  try {
    await command(f.main, "cd projects");
    await atDirectory(f.main, `${home}/projects`);
    await node(f.main, "/var").click();
    await expect(node(f.main, "/var/log")).toBeVisible();
    await command(f.main, "cd '/中文 空格/子目录'");
    await atDirectory(f.main, "/中文 空格/子目录");
    await f.main.getByRole("button", { name: "新建连接", exact: true }).click();
    await authenticate(f.main);
    await expect(f.main.getByRole("tab")).toHaveCount(2);
    await expect(node(f.main, "/var")).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    await expect(node(f.main, "/中文 空格")).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    await f.main.getByRole("tab", { name: title, exact: true }).click();
    await atDirectory(f.main, "/中文 空格/子目录");
    await expect(node(f.main, `${home}/projects`)).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    const connections = await terminalCount();
    const opened = f.context.waitForEvent("page");
    await f.main
      .getByRole("tab", { name: title, exact: true })
      .click({ button: "right" });
    await f.main
      .getByRole("menuitem", { name: "移到独立窗口", exact: true })
      .click();
    detached = await opened;
    await expect(active(detached).locator(".connection-state")).toHaveText(
      "已连接",
    );
    await atDirectory(detached, "/中文 空格/子目录");
    await expect(node(detached, `${home}/projects`)).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await expect(node(detached, "/var")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await expect(node(detached, "/var/log")).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(await terminalCount()).toBe(connections);
    await atDirectory(f.main, home);
    await command(detached, "cd /var/log");
    await atDirectory(detached, "/var/log");
    await atDirectory(f.main, home);
    await detached
      .getByRole("tab", { name: title, exact: true })
      .click({ button: "right" });
    await detached
      .getByRole("menuitem", { name: "移回主窗口", exact: true })
      .click();
    await expect.poll(() => detached!.isClosed()).toBe(true);
    await expect(f.main.getByRole("tab")).toHaveCount(2);
    await f.main.getByRole("tab", { name: title, exact: true }).click();
    await atDirectory(f.main, "/var/log");
    await expect(node(f.main, `${home}/projects`)).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(await terminalCount()).toBe(connections);
    await expect.poll(() => ipc<number>(f.main, "session_count")).toBe(2);
    await f.main
      .getByRole("tab", { name: duplicateTitle, exact: true })
      .click();
    await atDirectory(f.main, home);
    await expect(node(f.main, "/var")).toHaveAttribute(
      "aria-expanded",
      "false",
    );
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
        await node(f.main, path).click();
        await active(f.main)
          .getByRole("button", { name: "下载选中项目", exact: true })
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
      f.main.getByRole("region", { name: "下载任务" }),
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

test("unsupported or failed shell probes fall back to a usable shell, and reconnect restores cwd following", async () => {
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
      await authenticate(f.main);
      await expect(f.main.getByRole("tab")).toHaveCount(1);
      await command(f.main, "cd projects");
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
