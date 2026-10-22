import { showServers, connectSelectedServer, serverAction } from "./server-picker";
import { chromium, expect, test, type Page } from "@playwright/test";
import { readFile, unlink, writeFile } from "node:fs/promises";

// Runs against a real Windows Tauri WebView2 + tests/ssh_fixture.py, without mocked IPC.
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
async function closeContainer(page: Page) {
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

async function authenticate(main: Page) {
  await expect(async () => {
    const password = main
      .getByRole("dialog")
      .getByLabel("密码", { exact: true });
    if (await password.isVisible()) {
      await password.fill("fixture-password");
      await main
        .getByRole("dialog")
        .getByRole("button", { name: "连接", exact: true })
        .click();
    }
    const trust = main.getByRole("button", { name: "确认指纹并连接" });
    if (await trust.isVisible()) await trust.click();
    await expect(
      main.locator(".session-page[role=tabpanel]:visible").locator(".connection-state"),
    ).toHaveText("已连接", { timeout: 1000 });
  }).toPass({ timeout: 20000 });
  await selectContainer(main);
}

async function selectContainer(main: Page, name = "web-fixture") {
  const panel = main.locator(".session-page[role=tabpanel]:visible");
  await panel.getByRole("tab", { name: "Docker", exact: true }).click();
  await panel.getByRole("group", { name: "Docker 对象类型" }).getByRole("button", { name: /^容器/ }).click();
  await panel.getByRole("listbox", { name: "容器列表" }).getByRole("option").filter({ hasText: name }).click();
}

test("SQLite CRUD, real SSH/SFTP, Docker tree and independent container windows", async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  const context = browser.contexts()[0];
  const main = context
    .pages()
    .find(
      (page) =>
        !page.url().includes("container=1") &&
        page.url().startsWith("http://127.0.0.1:1420"),
    )!;
  expect(
    main,
    "start the desktop app with WebView2 debugging enabled",
  ).toBeTruthy();
  const suffix = Date.now().toString();
  const name = `QA SSH ${suffix}`,
    other = `QA Other ${suffix}`;
  const created: string[] = [];
  const errors: string[] = [];
  main.on("pageerror", (error) => errors.push(error.message));
  main.on("dialog", (dialog) => dialog.accept());
  let container: Page | undefined;
  try {
    await unlink(".qa/docker-mode").catch(() => {});
    await showServers(main);
    await main.getByRole("button", { name: /^新增服务器$/ }).click();
    let dialog = main.getByRole("dialog");
    await dialog.getByLabel(/^名称/).fill(name);
    await dialog.getByLabel("地址", { exact: true }).fill("127.0.0.1");
    await dialog.getByLabel("端口", { exact: true }).fill("22222");
    await dialog.getByLabel("用户名", { exact: true }).fill("ops");
    await dialog.getByLabel("认证方式").selectOption("password");
    await dialog.getByPlaceholder("选择或输入标签").fill("QA");
    await dialog.getByRole("button", { name: "添加", exact: true }).click();
    await dialog.locator(".editor-save-actions button.primary").click();
    await expect(main.getByRole("region", { name: "服务器", exact: true })).toBeVisible();
    const first = (await ipc<any[]>(main, "list_servers")).find(
      (server) => server.name === name,
    );
    created.push(first.id);
    expect(first.tags).toEqual(["QA"]);
    // Reload the frontend: records are read back from the Rust SQLite backend.
    await main.reload();
    await showServers(main);
    await expect(
      main.getByRole("button").filter({ hasText: name }),
    ).toBeVisible();
    await main.getByRole("button").filter({ hasText: name }).click();
    await main.getByLabel("搜索服务器").fill("no-match-qa");
    await expect(main.getByText("没有匹配的服务器")).toBeVisible();
    await main.getByLabel("搜索服务器").fill("");
    await main.getByLabel("按标签筛选").selectOption("QA");
    await showServers(main);
    await serverAction(main, "编辑");
    dialog = main.getByRole("dialog");
    await dialog.getByPlaceholder("选择或输入标签").fill("qa");
    await dialog.getByRole("button", { name: "添加", exact: true }).click();
    await dialog.locator(".editor-save-actions button.primary").click();
    expect(
      (await ipc<any[]>(main, "list_servers")).find(
        (server) => server.id === first.id,
      ).tags,
    ).toEqual(["QA"]);
    await main.getByRole("button", { name: "连接", exact: true }).click();
    await authenticate(main);
    expect(
      (await ipc<any[]>(main, "list_servers")).find(
        (server) => server.id === first.id,
      ).hasSavedPassword,
    ).toBe(true);
    await expect(main.locator(".terminal-surface")).toContainText(
      "Fixture host ready",
    );
    await main.locator(".terminal-surface textarea").focus();
    await main.keyboard.type("echo main-marker");
    await main.keyboard.press("Enter");
    await expect(main.locator(".terminal-surface")).toContainText(
      /main-marker\s*ops@host/,
    );
    await expect(
      main.locator('.file-row[data-path="/projects"]'),
    ).toBeVisible();
    await main.locator('.file-row[data-path="/projects"]').click();
    await expect(main.getByLabel("远程目录路径")).toHaveValue("/projects");
    await expect(
      main.locator('.file-row[data-path="/projects/hello.txt"]'),
    ).toBeVisible();
    await main.getByRole("button", { name: "上一级目录" }).click();
    await expect(main.getByLabel("远程目录路径")).toHaveValue("/");
    await expect(main.locator(".docker-engine-title")).toContainText("可访问");
    await main.getByRole("button", { name: /^镜像/ }).click();
    await expect(main.getByRole("listbox", { name: "镜像列表" }).getByText("nginx:alpine", { exact: true })).toBeVisible();
    await selectContainer(main, "stopped-fixture");
    await expect(main.getByRole("button", { name: "进入 stopped-fixture 终端" })).toBeDisabled();
    await selectContainer(main, "paused-fixture");
    await expect(main.getByRole("button", { name: "进入 paused-fixture 终端" })).toBeDisabled();
    await selectContainer(main);
    await main.screenshot({ path: ".qa/desktop-connected.png" });
    const pageOpened = context.waitForEvent("page");
    await main
      .getByRole("button", { name: "进入 web-fixture 终端" })
      .click();
    container = await pageOpened;
    await expect(container.locator(".terminal-surface")).toContainText(
      "Fixture container ready",
    );
    const bound = await ipc<any>(container, "container_info");
    expect(bound.serverName).toBe(name);
    expect(
      await container.evaluate(async () => {
        try {
          await (window as any).__TAURI_INTERNALS__.invoke("list_servers");
          return "allowed";
        } catch (error) {
          return (error as any).kind;
        }
      }),
    ).toBe("permission");
    const pageCount = context.pages().length;
    await main
      .getByRole("button", { name: "进入 web-fixture 终端" })
      .click();
    expect(context.pages()).toHaveLength(pageCount);
    expect(await ipc<number>(main, "session_count")).toBe(2);
    // Closing a container must leave the main terminal alive.
    await closeContainer(container);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await expect(main.locator(".connection-state")).toHaveText("已连接");
    const reopened = context.waitForEvent("page");
    await main
      .getByRole("button", { name: "进入 web-fixture 终端" })
      .click();
    container = await reopened;
    await expect(container.locator(".terminal-surface")).toContainText(
      "Fixture container ready",
    );
    // Server-list selection and CRUD must leave the active workspace connected.
    await showServers(main);
    await main.getByRole("button", { name: /^新增服务器$/ }).click();
    dialog = main.getByRole("dialog");
    await dialog.getByLabel(/^名称/).fill(other);
    await dialog.getByLabel("地址", { exact: true }).fill("192.0.2.1");
    await dialog.locator(".editor-save-actions button.primary").click();
    const second = (await ipc<any[]>(main, "list_servers")).find(
      (server) => server.name === other,
    );
    created.push(second.id);
    await expect(main.locator(".terminal-toolbar strong")).toHaveText(name);
    await expect(main.locator(".connection-state")).toHaveText("已连接");
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    expect((await ipc<any>(container, "container_info")).serverName).toBe(name);
    await container.locator(".terminal-surface textarea").focus();
    await container.keyboard.type("echo isolated-marker");
    await container.keyboard.press("Enter");
    await expect(container.locator(".terminal-surface")).toContainText(
      /isolated-marker\s*ops@container/,
    );
    await container.screenshot({ path: ".qa/container-independent.png" });
    await closeContainer(container);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await showServers(main);
    await serverAction(main, "删除服务器");
    await main
      .getByRole("dialog", { name: "删除服务器配置？" })
      .getByRole("button", { name: "确认删除" })
      .click();
    await expect(main.getByRole("region", { name: "服务器", exact: true })).toBeVisible();
    await main.getByRole("button", { name: "关闭服务器窗口" }).click();
    expect(
      (await ipc<any[]>(main, "list_servers")).some(
        (server) => server.id === second.id,
      ),
    ).toBe(false);
    await main
      .getByRole("button", { name: `关闭 ${name} 服务器`, exact: true })
      .click();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    if (container && !container.isClosed())
      await closeContainer(container).catch(() => {});
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    for (const id of created) await ipc(main, "delete_server", { id });
    await unlink(".qa/docker-mode").catch(() => {});
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    await browser.close(); // disconnects CDP; Tauri owns its browser process.
  }
});

test("Docker failures and stopped/shell-less containers preserve the host terminal", async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  const context = browser.contexts()[0];
  const main = context
    .pages()
    .find(
      (page) =>
        page.url().startsWith("http://127.0.0.1:1420") &&
        !page.url().includes("container=1"),
    )!;
  const name = `QA errors ${Date.now()}`;
  const server = await ipc<any>(main, "save_server", {
    server: {
      id: "",
      name,
      host: "127.0.0.1",
      port: 22222,
      username: "ops",
      tags: [],
      authType: "password",
      privateKey: "",
    },
  });
  let container: Page | undefined;
  try {
    await main.reload();
    await showServers(main);
    await main.getByRole("button").filter({ hasText: name }).click();
    await main.getByRole("button", { name: "连接", exact: true }).click();
    await authenticate(main);
    await expect(main.locator(".connection-state")).toHaveText("已连接");
    await expect(
      main.getByRole("button", { name: "进入 web-fixture 终端" }),
    ).toBeEnabled();
    for (const [mode, status] of [
      ["missing", "未找到 Docker 命令"],
      ["permission", "权限不足"],
      ["unreachable", "服务不可达"],
      ["timeout", "检测失败"],
    ]) {
      await writeFile(".qa/docker-mode", mode);
      await main.getByRole("button", { name: "刷新 Docker 状态" }).click();
      await expect(main.locator(".docker-engine-title")).toContainText(status, {
        timeout: mode === "timeout" ? 30000 : 18000,
      });
      await expect(main.locator(".connection-state")).toHaveText("已连接");
      if (
        await main
          .getByRole("button", { name: "进入 web-fixture 终端" })
          .count()
      ) {
        await expect(
          main.getByRole("button", { name: "进入 web-fixture 终端" }),
        ).toBeDisabled();
      }
    }
    for (const [mode, error] of [
      ["no-shell", "没有可用的 /bin/bash 或 /bin/sh"],
      ["stopped", "不处于可进入的运行状态"],
    ]) {
      await writeFile(".qa/docker-mode", mode);
      await main.getByRole("button", { name: "刷新 Docker 状态" }).click();
      await expect(main.locator(".docker-engine-title")).toContainText("可访问");
      await selectContainer(main);
      await expect(
        main.getByRole("button", { name: "进入 web-fixture 终端" }),
      ).toBeEnabled();
      const opened = context.waitForEvent("page");
      await main
        .getByRole("button", { name: "进入 web-fixture 终端" })
        .click();
      container = await opened;
      await expect(container.getByRole("alert")).toContainText(error);
      await expect(main.locator(".connection-state")).toHaveText("已连接");
      await closeContainer(container);
    }
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
  } finally {
    if (container && !container.isClosed())
      await closeContainer(container).catch(() => {});
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    await ipc(main, "delete_server", { id: server.id });
    await unlink(".qa/docker-mode").catch(() => {});
    await main.reload();
    await browser.close();
  }
});

test("multiple SSH tabs retain their own terminal, file and Docker state", async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  const context = browser.contexts()[0];
  const main = context
    .pages()
    .find(
      (page) =>
        page.url().startsWith("http://127.0.0.1:1420") &&
        !page.url().includes("container=1"),
    )!;
  const suffix = Date.now(),
    nameA = `QA A ${suffix}`,
    nameB = `QA B ${suffix}`;
  const base = {
    id: "",
    host: "127.0.0.1",
    port: 22222,
    username: "ops",
    tags: [],
    authType: "password",
    privateKey: "",
  };
  const serverA = await ipc<any>(main, "save_server", {
    server: { ...base, name: nameA },
  });
  const serverB = await ipc<any>(main, "save_server", {
    server: { ...base, name: nameB },
  });
  const errors: string[] = [];
  main.on("pageerror", (error) => errors.push(error.message));
  let container: Page | undefined;
  const active = () => main.locator(".session-page[role=tabpanel]:visible");
  async function type(command: string) {
    await active().locator(".terminal-surface textarea").focus();
    await main.keyboard.type(command);
    await main.keyboard.press("Enter");
  }
  try {
    await unlink(".qa/docker-mode").catch(() => {});
    await main.reload();
    // Observe shared backend updates; queries still execute through real SSH.
    await main.evaluate(async () => {
      const modulePath = performance.getEntriesByType("resource").map(entry => entry.name)
        .find(url => new URL(url).pathname === "/src/resourceMonitoring.ts") || "/src/resourceMonitoring.ts";
      const { MonitorStore } = await import(modulePath);
      const original = MonitorStore.prototype.receive;
      (window as any).__qaQueries = [];
      MonitorStore.prototype.receive = function(event: any) {
        if (event.type === "docker" && !event.docker.refreshing)
          (window as any).__qaQueries.push({ command: "docker_sample", sessionId: this.sessionId });
        return original.call(this, event);
      };
    });
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: nameA }).dblclick();
    await authenticate(main);
    await expect(
      active().getByRole("button", { name: "进入 web-fixture 终端" }),
    ).toBeEnabled();
    await type("echo marker-A");
    await expect(active().locator(".terminal-surface")).toContainText(
      /marker-A\s*ops@host/,
    );
    await active().getByRole("button", { name: /^镜像/ }).click();
    await expect(
      active().getByRole("listbox", { name: "镜像列表" }).getByText("nginx:alpine", { exact: true }),
    ).toBeVisible();
    await active().locator('.file-row[data-path="/readme.txt"]').click();
    await expect(active().locator(".file-row.selected")).toContainText(
      "readme.txt",
    );
    await type("later background-A");
    // Selecting B does not switch, clear or disconnect A.
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: nameB }).click();
    await expect(active().locator(".terminal-toolbar strong")).toHaveText(
      nameA,
    );
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: nameB }).dblclick();
    await authenticate(main);
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(2);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    await expect(active().getByLabel("远程目录路径")).toHaveValue("/");
    await expect(
      active().getByRole("button", { name: /^镜像/ }),
    ).toHaveAttribute("aria-pressed", "false");
    await expect(active().locator(".terminal-surface")).not.toContainText(
      "marker-A",
    );
    await type("echo marker-B");
    await expect(active().locator(".terminal-surface")).toContainText(
      /marker-B\s*ops@host/,
    );
    await main
      .getByRole("tab", { name: `${nameA}，已连接`, exact: true })
      .click();
    await expect(active().locator(".terminal-surface")).toContainText(
      "background-A",
    );
    await expect(active().locator(".terminal-surface")).not.toContainText(
      "marker-B",
    );
    await expect(active().locator(".file-row.selected")).toContainText(
      "readme.txt",
    );
    await expect(
      active().getByRole("button", { name: /^镜像/ }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(
      active().getByRole("button", { name: "刷新 Docker 状态" }),
    ).toBeEnabled();
    // A directory result arriving after a switch belongs to A, not the active B page.
    await writeFile(".qa/sftp-delay", "1");
    await active().locator('.file-row[data-path="/projects"]').click();
    await main
      .getByRole("tab", { name: `${nameB}，已连接`, exact: true })
      .click();
    await expect(active().getByLabel("远程目录路径")).toHaveValue("/");
    await expect(
      active().getByRole("button", { name: "刷新 Docker 状态" }),
    ).toBeEnabled();
    const ids = await main.evaluate(() => [
      ...new Set(
        (window as any).__qaQueries.map((item: any) => item.sessionId),
      ),
    ]);
    const idA = ids[0] as string;
    const idB = ids[1] as string;
    const countQuery = (id: string) =>
      main.evaluate(
        (id) =>
          (window as any).__qaQueries.filter(
            (item: any) =>
              item.command === "docker_sample" && item.sessionId === id,
          ).length,
        id,
      );
    const countA = await countQuery(idA),
      countB = await countQuery(idB);
    await expect
      .poll(() => countQuery(idB), { timeout: 18000, intervals: [500, 1000] })
      .toBeGreaterThan(countB);
    expect(await countQuery(idA)).toBe(countA);
    await expect(active().getByLabel("远程目录路径")).toHaveValue("/");
    await main
      .getByRole("tab", { name: `${nameA}，已连接`, exact: true })
      .click();
    await expect(active().getByLabel("远程目录路径")).toHaveValue("/projects");
    await expect(
      active().locator('.file-row[data-path="/projects/hello.txt"]'),
    ).toBeVisible();
    await unlink(".qa/sftp-delay");
    // A deliberate duplicate is a third independent connection to the same host.
    await showServers(main);
    await main
      .locator(".server-row")
      .filter({ hasText: nameA })
      .click({ button: "right" });
    await main.getByRole("menuitem", { name: "新建连接", exact: true }).click();
    await authenticate(main);
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(3);
    await expect(
      main.getByRole("tab", { name: `${nameA} · 2，已连接`, exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(3);
    await expect(active().getByLabel("远程目录路径")).toHaveValue("/");
    await expect(active().locator(".terminal-surface")).not.toContainText(
      "marker-A",
    );
    await type("echo duplicate-marker");
    await expect(active().locator(".terminal-surface")).toContainText(
      /duplicate-marker\s*ops@host/,
    );
    await main
      .getByRole("tab", { name: `${nameA}，已连接`, exact: true })
      .click();
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(3);
    await expect(active().getByLabel("远程目录路径")).toHaveValue("/projects");
    await expect(active().locator(".terminal-surface")).not.toContainText(
      "duplicate-marker",
    );
    await main.screenshot({ path: ".qa/multiple-ssh-tabs.png" });
    await expect(
      active().getByRole("button", { name: "进入 web-fixture 终端" }),
    ).toBeEnabled();
    const opened = context.waitForEvent("page");
    await active()
      .getByRole("button", { name: "进入 web-fixture 终端" })
      .click();
    container = await opened;
    await expect(container.locator(".terminal-surface")).toContainText(
      "Fixture container ready",
    );
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(4);
    // Closing a background tab leaves the active tab and container alone.
    await main
      .getByRole("button", { name: `关闭 ${nameB} 服务器`, exact: true })
      .click();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(3);
    await expect(active().locator(".terminal-toolbar strong")).toHaveText(
      nameA,
    );
    await main
      .getByRole("button", { name: `关闭 ${nameA} 服务器`, exact: true })
      .click();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(1);
    expect((await ipc<any>(container, "container_info")).serverName).toBe(
      nameA,
    );
    await type("echo survivor-marker");
    await expect(active().locator(".terminal-surface")).toContainText(
      /survivor-marker\s*ops@host/,
    );
    await active().getByRole("button", { name: "断开", exact: true }).click();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await expect(
      main.getByRole("tab", { name: `${nameA} · 2，已断开`, exact: true }),
    ).toBeVisible();
    await expect(active().locator(".terminal-surface")).toContainText(
      "survivor-marker",
    );
    await active()
      .getByRole("button", { name: "重新连接", exact: true })
      .click();
    await authenticate(main);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    await expect(active().locator(".terminal-surface")).toContainText(
      "survivor-marker",
    );
    await main
      .getByRole("button", { name: `关闭 ${nameA} · 2 服务器`, exact: true })
      .click();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(0);
    await closeContainer(container);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    if (container && !container.isClosed())
      await closeContainer(container).catch(() => {});
    await unlink(".qa/sftp-delay").catch(() => {});
    await unlink(".qa/docker-mode").catch(() => {});
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    await ipc(main, "delete_server", { id: serverA.id });
    await ipc(main, "delete_server", { id: serverB.id });
    await main.reload();
    await browser.close();
  }
});

test("failed, pending and remotely closed tabs do not affect other connections", async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  const main = browser
    .contexts()[0]
    .pages()
    .find(
      (page) =>
        page.url().startsWith("http://127.0.0.1:1420") &&
        !page.url().includes("container=1"),
    )!;
  const name = `QA lifecycle ${Date.now()}`;
  const server = await ipc<any>(main, "save_server", {
    server: {
      id: "",
      name,
      host: "127.0.0.1",
      port: 22222,
      username: "ops",
      tags: [],
      authType: "password",
      privateKey: "",
    },
  });
  const pendingName = `QA pending ${Date.now()}`;
  const pendingServer = await ipc<any>(main, "save_server", {
    server: { ...server, id: "", name: pendingName, username: "ops-delay" },
  });
  const active = () => main.locator(".session-page[role=tabpanel]:visible");
  try {
    await unlink(".qa/docker-mode").catch(() => {});
    await main.reload();
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: name }).dblclick();
    await authenticate(main);
    // Wrong credentials prompt only the duplicate; the original stays connected.
    await showServers(main);
    await serverAction(main, "编辑");
    await main
      .getByRole("dialog")
      .getByLabel("密码", { exact: true })
      .fill("wrong-password");
    await main
      .getByRole("dialog")
      .locator(".editor-save-actions button.primary")
      .click();
    await connectSelectedServer(main);
    // Invalid stored credentials prompt again in the same connection attempt.
    await expect(
      main.getByRole("dialog").getByLabel("密码", { exact: true }),
    ).toBeVisible();
    await expect(
      main.getByRole("tab", { name: `${name}，已连接`, exact: true }),
    ).toBeVisible();
    await authenticate(main);
    expect(
      (await ipc<any[]>(main, "list_servers")).find(
        (value) => value.id === server.id,
      ).hasSavedPassword,
    ).toBe(true);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    await main
      .getByRole("button", { name: `关闭 ${name} · 2 服务器`, exact: true })
      .click();
    // Closing a tab before authentication completes must eventually dispose its session.
    await expect(
      active().getByRole("button", { name: "刷新 Docker 状态" }),
    ).toBeEnabled();
    const countEvent = async (event: string) =>
      (await readFile(".qa/ssh-events.jsonl", "utf8"))
        .split("\n")
        .filter(
          (line) =>
            line.includes(`"event": "${event}"`) &&
            line.includes('"username": "ops-delay"'),
        ).length;
    const finishedBefore = await countEvent("delayed-auth-finished"),
      closedBefore = await countEvent("connection-close");
    await writeFile(".qa/auth-delay", "1");
    await showServers(main);
    await main
      .locator(".server-row")
      .filter({ hasText: pendingName })
      .dblclick();
    await main
      .getByRole("dialog")
      .getByLabel("密码", { exact: true })
      .fill("fixture-password");
    await main
      .getByRole("dialog")
      .getByRole("button", { name: "连接", exact: true })
      .click();
    await expect(
      main.locator(".session-tab").filter({ hasText: pendingName }),
    ).toBeVisible();
    await main
      .getByRole("region", { name: "服务器", exact: true })
      .getByRole("button", { name: "取消连接", exact: true })
      .click();
    await expect
      .poll(() => countEvent("delayed-auth-finished"), { timeout: 8000 })
      .toBeGreaterThan(finishedBefore);
    await expect
      .poll(() => countEvent("connection-close"), { timeout: 8000 })
      .toBeGreaterThan(closedBefore);
    await unlink(".qa/auth-delay");
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(1);
    // Remote exit keeps the output and tab, and reconnecting does not add a new tab.
    await active().locator(".terminal-surface textarea").focus();
    await main.keyboard.type("echo retained-before-exit");
    await main.keyboard.press("Enter");
    await expect(active().locator(".terminal-surface")).toContainText(
      /retained-before-exit\s*ops@host/,
    );
    await main.keyboard.type("exit");
    await main.keyboard.press("Enter");
    await expect(
      main.getByRole("tab", { name: `${name}，已断开`, exact: true }),
    ).toBeVisible();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    await expect(active().locator(".terminal-surface")).toContainText(
      "retained-before-exit",
    );
    await active()
      .getByRole("button", { name: "重新连接", exact: true })
      .click();
    await authenticate(main);
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(1);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await expect(active().locator(".terminal-surface")).toContainText(
      "retained-before-exit",
    );
    // Keyboard navigation and closing the final tab work after reconnect.
    await main
      .getByRole("tab", { name: `${name}，已连接`, exact: true })
      .focus();
    await main.keyboard.press("Delete");
    await expect(main.getByRole("tablist", { name: "服务器标签", exact: true }).getByRole("tab")).toHaveCount(0);
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
  } finally {
    await unlink(".qa/auth-delay").catch(() => {});
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    await ipc(main, "delete_server", { id: server.id });
    await ipc(main, "delete_server", { id: pendingServer.id });
    await main.reload();
    await browser.close();
  }
});

test("saved passwords survive reload, connect automatically and can be cleared", async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  const main = browser
    .contexts()[0]
    .pages()
    .find(
      (page) =>
        page.url().startsWith("http://127.0.0.1:1420") &&
        !page.url().includes("container=1"),
    )!;
  const name = `QA password ${Date.now()}`;
  let serverId = "";
  const active = () => main.locator(".session-page[role=tabpanel]:visible");
  async function storedPassword() {
    return (await ipc<any[]>(main, "list_servers")).find(
      (server) => server.id === serverId,
    ).hasSavedPassword;
  }
  async function waitForAutoConnection() {
    await expect(async () => {
      const trust = main.getByRole("button", { name: "确认指纹并连接" });
      if (await trust.isVisible()) await trust.click();
      await expect(active().locator(".connection-state")).toHaveText("已连接", {
        timeout: 1000,
      });
    }).toPass({ timeout: 20000 });
    await expect(main.getByRole("dialog")).toHaveCount(0);
  }
  try {
    await main.reload();
    await showServers(main);
    await main.getByRole("button", { name: /^新增服务器$/ }).click();
    const editor = main.getByRole("dialog");
    await editor.getByLabel(/^名称/).fill(name);
    await editor.getByLabel("地址", { exact: true }).fill("127.0.0.1");
    await editor.getByLabel("端口", { exact: true }).fill("22222");
    await editor.getByLabel("用户名", { exact: true }).fill("ops");
    await editor.getByLabel("认证方式").selectOption("password");
    await expect(editor.getByLabel("密码", { exact: true })).toHaveAttribute(
      "type",
      "password",
    );
    await editor.getByLabel("密码", { exact: true }).fill("fixture-password");
    await editor.locator(".editor-save-actions button.primary").click();
    serverId = (await ipc<any[]>(main, "list_servers")).find(
      (server) => server.name === name,
    ).id;
    await main.reload();
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: name }).click();
    await showServers(main);
    await serverAction(main, "编辑");
    await expect(editor.getByLabel("密码", { exact: true })).toHaveValue("");
    await expect(editor.getByLabel("密码", { exact: true })).toHaveAttribute(
      "placeholder",
      /已保存/,
    );
    await editor.getByRole("button", { name: "取消", exact: true }).click();
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: name }).dblclick();
    await waitForAutoConnection();
    await connectSelectedServer(main);
    await waitForAutoConnection();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    await active().getByRole("button", { name: "断开", exact: true }).click();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(1);
    await active()
      .getByRole("button", { name: "重新连接", exact: true })
      .click();
    await waitForAutoConnection();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    // Clearing the configuration must not disconnect either already established SSH.
    await showServers(main);
    await serverAction(main, "编辑");
    await editor.getByRole("button", { name: "清空已保存密码" }).click();
    await editor.locator(".editor-save-actions button.primary").click();
    expect(await storedPassword()).toBe(false);
    await main.getByRole("button", { name: "关闭服务器窗口" }).click();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(2);
    await active().getByRole("button", { name: "断开", exact: true }).click();
    await active()
      .getByRole("button", { name: "重新连接", exact: true })
      .click();
    await expect(
      main.getByRole("dialog").getByLabel("密码", { exact: true }),
    ).toHaveValue("");
    await authenticate(main);
    expect(await storedPassword()).toBe(true);
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    await showServers(main);
    await main.locator(".server-row").filter({ hasText: name }).dblclick();
    await waitForAutoConnection();
    await expect(active().locator(".terminal-surface")).toContainText(
      "Fixture host ready",
    );
  } finally {
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    if (serverId) await ipc(main, "delete_server", { id: serverId });
    await main.reload();
    await expect.poll(() => ipc<number>(main, "session_count")).toBe(0);
    await browser.close();
  }
});
