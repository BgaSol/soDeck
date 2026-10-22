import { chromium, expect } from "@playwright/test";
import fs from "node:fs";
const cdp = process.env.SODECK_WORKSPACE_CDP_URL || "http://127.0.0.1:9225";
await expect
  .poll(
    () =>
      fetch(`${cdp}/json/version`)
        .then((r) => r.ok)
        .catch(() => false),
    { timeout: 20000 },
  )
  .toBe(true);
const browser = await chromium.connectOverCDP(cdp);
const context = browser.contexts()[0];
const main = context
  .pages()
  .find((p) => !new URL(p.url()).searchParams.has("workspace"));
const ipc = (page, command, args = {}) =>
  page.evaluate(
    ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
    { command, args },
  );
const errors = [];
const observe = (p) => p.on("pageerror", (e) => errors.push(e.message));
context.pages().forEach(observe);
context.on("page", observe);
const checks = [];
const list = async (page) =>
  (await ipc(page, "list_server_workspaces")).workspaces;
const connected = async (page, n) =>
  expect
    .poll(
      async () =>
        (await list(page))[0]?.terminals.filter((t) => t.state === "connected")
          .length,
      { timeout: 25000 },
    )
    .toBe(n);
const top = (page) => page.locator('.tab-select[aria-selected="true"]');
const terminal = (page) => page.locator(".terminal-session:visible");
async function select(page, n) {
  await page
    .getByRole("tablist", { name: "终端标签" })
    .getByRole("tab")
    .nth(n)
    .click();
}
async function command(page, text) {
  const input = terminal(page).locator(".xterm-helper-textarea");
  await input.focus();
  await expect(input).toBeFocused();
  await page.keyboard.insertText(text);
  await input.press("Enter");
}
async function moveOut() {
  const incoming = context.waitForEvent("page");
  await top(main).click({ button: "right" });
  await main
    .getByRole("menuitem", { name: "移到独立窗口", exact: true })
    .click();
  const page = await incoming;
  await page.waitForURL(/workspace=/);
  await page.waitForFunction(
    () =>
      !!window.__TAURI_INTERNALS__ && !!document.querySelector(".app-shell"),
  );
  await expect
    .poll(async () => (await list(page))[0].owner.label, { timeout: 25000 })
    .toMatch(/^workspace-/);
  await expect.poll(async () => (await list(page))[0].lifecycle).toBe("active");
  return page;
}
async function moveBack(page) {
  await top(page).click({ button: "right" });
  await page.getByRole("menuitem", { name: "移回主窗口", exact: true }).click();
  await expect
    .poll(async () => (await list(main))[0].owner.label, { timeout: 25000 })
    .toBe("main");
  await expect.poll(() => page.isClosed()).toBe(true);
}
const events = () => {
  const path =
    process.env.SODECK_WORKSPACE_FIXTURE_LOG ||
    ".qa/workspace-ssh/ssh-events.jsonl";
  return fs.existsSync(path)
    ? fs
        .readFileSync(path, "utf8")
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l))
    : [];
};
const opens = () =>
  events().filter((e) => e.event === "terminal-open" && e.target === "host")
    .length;
const saved = [];
try {
  await main.reload();
  // Remove only workspaces owned by this isolated QA profile from prior runs.
  for (const w of await list(main))
    if (w.owner.label === "main") {
      const impact = await ipc(main, "prepare_workspace_close", {
        workspaceId: w.id,
        ownerEpoch: w.owner.ownerEpoch,
      });
      await ipc(main, "close_server_workspace", {
        workspaceId: w.id,
        ownerEpoch: w.owner.ownerEpoch,
        revision: impact.revision,
      });
    }
  const server = await ipc(main, "save_server", {
    server: {
      id: "",
      name: "Workspace tabs native " + Date.now(),
      host: "127.0.0.1",
      port: 22225,
      username: "ops",
      tags: ["QA"],
      authType: "password",
      privateKey: "",
      jumpServerId: null,
      directHost: null,
      directPort: 22,
      passwordChange: { type: "set", value: "fixture-password" },
    },
  });
  saved.push(server.id);
  const baseline = opens();
  const w = await ipc(main, "open_server_workspace", {
    serverId: server.id,
    newTerminal: false,
    requestId: crypto.randomUUID(),
  });
  await expect(async () => {
    const trust = main.getByRole("button", { name: "确认指纹并连接" });
    if (await trust.isVisible()) await trust.click();
    await connected(main, 1);
  }).toPass({ timeout: 30000 });
  for (let i = 0; i < 2; i++)
    await main.getByRole("button", { name: "新建终端", exact: true }).click();
  await connected(main, 3);
  await expect(main.locator(".session-tab")).toHaveCount(1);
  expect(opens() - baseline).toBe(3);
  let workspace = (await list(main))[0];
  const ids = workspace.terminals.map((t) => t.connectionId);
  const terminalIds = workspace.terminals.map((t) => t.id);
  const oldContext = workspace.tools.context;
  checks.push("one workspace, three independent real SSH/PTTY connections");
  await select(main, 0);
  await command(main, "echo native_terminal_one");
  await expect(terminal(main).locator(".xterm-rows")).toContainText(
    "native_terminal_one",
  );
  await select(main, 1);
  await command(main, "echo native_terminal_two");
  await expect(terminal(main).locator(".xterm-rows")).toContainText(
    "native_terminal_two",
  );
  await expect(terminal(main).locator(".xterm-rows")).not.toContainText(
    "native_terminal_one",
  );
  await select(main, 2);
  await command(main, "cd /var/log");
  const locate = main.getByRole("button", {
    name: "定位到当前终端目录",
    exact: true,
  });
  await expect(locate).toBeEnabled();
  await expect(main.getByLabel("远程目录路径")).toHaveValue("/home/ops");
  await locate.click();
  await expect(main.getByLabel("远程目录路径")).toHaveValue("/var/log");
  checks.push("input/output isolation and explicit CWD locate over real SFTP");
  await main.screenshot({ path: ".qa/server-terminal-tabs-multiple.png" });
  const history = async () =>
    (await ipc(main, "terminal_history_sessions", { query: {} })).filter(
      (s) => s.source.serverId === server.id,
    );
  const originalHistory = await history();
  expect(originalHistory.map((s) => s.workspaceId).sort()).toEqual(
    [...terminalIds].sort(),
  );
  const oversized = await ipc(main, "prepare_workspace_move", {
    workspaceId: w.id,
    ownerEpoch: (await list(main))[0].owner.ownerEpoch,
  });
  const oversizedError = await main.evaluate(async (transferId) => {
    try {
      await window.__TAURI_INTERNALS__.invoke("move_workspace", {
        payload: {
          version: 2,
          transferId,
          padding: "x".repeat(24 * 1024 * 1024),
        },
        target: null,
        beforeId: null,
      });
      return null;
    } catch (error) {
      return error;
    }
  }, oversized.transferId);
  expect(oversizedError).toMatchObject({ kind: "limit" });
  await ipc(main, "cancel_workspace_move", {
    transferId: oversized.transferId,
  });
  expect((await list(main))[0].terminals.map((t) => t.connectionId)).toEqual(
    ids,
  );
  checks.push(
    "oversized collective snapshot is rejected and the original workspace remains intact",
  );
  // Freeze one member then abort the whole transaction; no session is recreated.
  workspace = (await list(main))[0];
  await select(main, 0);
  await command(main, "later output_during_frozen_move");
  const prepared = await ipc(main, "prepare_workspace_move", {
    workspaceId: w.id,
    ownerEpoch: workspace.owner.ownerEpoch,
  });
  await ipc(main, "terminal_freeze", {
    sessionId: ids[0],
    token: "native-partial-freeze",
  });
  await ipc(main, "cancel_workspace_move", { transferId: prepared.transferId });
  await expect.poll(async () => (await list(main))[0].lifecycle).toBe("active");
  expect((await list(main))[0].terminals.map((t) => t.connectionId)).toEqual(
    ids,
  );
  await select(main, 0);
  await command(main, "echo after_native_rollback");
  await expect(terminal(main).locator(".xterm-rows")).toContainText(
    "after_native_rollback",
  );
  await expect(terminal(main).locator(".xterm-rows")).toContainText(
    "output_during_frozen_move",
  );
  checks.push(
    "partial freeze rolls back the complete workspace without reconnecting",
  );
  // Prevent the receiver from mounting, then destroy its real native window.
  // Every source stream must return before a subsequent move is allowed.
  let blockedTarget = false;
  const blockReceiver = async (route) => {
    if (route.request().frame().page().url().includes("workspace=")) {
      blockedTarget = true;
      await route.fulfill({
        status: 200,
        contentType: "text/javascript",
        body: "// receiver failure injection",
      });
    } else await route.continue();
  };
  await context.route("**/src/main.tsx*", blockReceiver);
  try {
    const incoming = context.waitForEvent("page");
    await top(main).click({ button: "right" });
    await main
      .getByRole("menuitem", { name: "移到独立窗口", exact: true })
      .click();
    const failedTarget = await incoming;
    await expect.poll(() => blockedTarget).toBe(true);
    const closed = failedTarget.waitForEvent("close");
    await ipc(failedTarget, "plugin:window|destroy", {
      label: await failedTarget.evaluate(
        () => window.__TAURI_INTERNALS__.metadata.currentWindow.label,
      ),
    }).catch(() => {});
    await closed;
    await expect
      .poll(async () => (await list(main))[0].lifecycle)
      .toBe("active");
    await expect(main.locator(".session-moving")).toHaveCount(0);
    expect((await list(main))[0].terminals.map((t) => t.connectionId)).toEqual(
      ids,
    );
    await command(main, "echo after_target_window_closed");
    await expect(terminal(main).locator(".xterm-rows")).toContainText(
      "after_target_window_closed",
    );
    expect(opens() - baseline).toBe(3);
    checks.push(
      "native target destruction before reception rolls back all terminals without creating another shell",
    );
  } finally {
    await context.unroute("**/src/main.tsx*", blockReceiver);
  }
  // A real native window receives every xterm snapshot and replay before commit.
  const sourceContext = (await list(main))[0].tools.context;
  const incoming = context.waitForEvent("page");
  await top(main).click({ button: "right" });
  await main
    .getByRole("menuitem", { name: "移到独立窗口", exact: true })
    .click();
  const detached = await incoming;
  await detached.waitForURL(/workspace=/);
  await detached.waitForFunction(
    () =>
      !!window.__TAURI_INTERNALS__ && !!document.querySelector(".app-shell"),
  );
  await expect
    .poll(async () => (await list(detached))[0].owner.label, { timeout: 25000 })
    .toMatch(/^workspace-/);
  await expect
    .poll(async () => (await list(detached))[0].lifecycle)
    .toBe("active");
  await connected(detached, 3);
  expect(
    (await list(detached))[0].terminals.map((t) => t.connectionId),
  ).toEqual(ids);
  expect(opens() - baseline).toBe(3);
  await expect(
    detached.getByRole("tablist", { name: "终端标签" }).getByRole("tab"),
  ).toHaveCount(3);
  await select(detached, 0);
  await expect(terminal(detached).locator(".xterm-rows")).toContainText(
    "after_native_rollback",
  );
  await ipc(main, "open_server_workspace", {
    serverId: server.id,
    newTerminal: false,
    requestId: crypto.randomUUID(),
  });
  expect((await list(main)).length).toBe(1);
  await expect(main.locator(".session-tab")).toHaveCount(0);
  await expect(
    ipc(main, "list_directory", { context: oldContext, path: "/" }),
  ).rejects.toBeTruthy();
  await expect(
    ipc(main, "list_directory", { context: sourceContext, path: "/" }),
  ).rejects.toBeTruthy();
  checks.push(
    "collective native window migration preserves session IDs and shell count; global uniqueness and old ownership revoked",
  );
  await top(detached).click({ button: "right" });
  await detached
    .getByRole("menuitem", { name: "移回主窗口", exact: true })
    .click();
  await expect
    .poll(async () => (await list(main))[0].owner.label, { timeout: 25000 })
    .toBe("main");
  await connected(main, 3);
  expect((await list(main))[0].terminals.map((t) => t.connectionId)).toEqual(
    ids,
  );
  expect(opens() - baseline).toBe(3);
  checks.push(
    "native return migration retains all terminals and the same backend connections",
  );
  await select(main, 1);
  await terminal(main)
    .getByRole("button", { name: "断开", exact: true })
    .click();
  await connected(main, 2);
  const mixedIds = (await list(main))[0].terminals.map((t) => t.connectionId);
  const mixed = await moveOut();
  await connected(mixed, 2);
  expect((await list(mixed))[0].terminals.map((t) => t.connectionId)).toEqual(
    mixedIds,
  );
  await moveBack(mixed);
  await connected(main, 2);
  expect(opens() - baseline).toBe(3);
  checks.push(
    "mixed connected and disconnected terminals migrate together without reconnecting",
  );
  workspace = (await list(main))[0];
  for (const id of terminalIds)
    await ipc(main, "close_workspace_terminal", {
      workspaceId: w.id,
      ownerEpoch: workspace.owner.ownerEpoch,
      terminalId: id,
    });
  await expect(
    main.getByText("此服务器暂无终端，文件和工具仍可使用。"),
  ).toBeVisible();
  const empty = (await list(main))[0];
  expect(empty.terminals).toHaveLength(0);
  expect(empty.tools.state).toBe("available");
  await ipc(main, "list_directory", {
    context: empty.tools.context,
    path: "/var/log",
  });
  await expect(
    ipc(main, "delete_server", { id: server.id }),
  ).rejects.toBeTruthy();
  const emptyWindow = await moveOut();
  const movedEmpty = (await list(emptyWindow))[0];
  expect(movedEmpty.terminals).toHaveLength(0);
  await ipc(emptyWindow, "list_directory", {
    context: movedEmpty.tools.context,
    path: "/var/log",
  });
  await moveBack(emptyWindow);
  await main.getByRole("button", { name: "更新工具连接", exact: true }).click();
  await expect
    .poll(async () => (await list(main))[0].tools.context.epoch)
    .toBe(empty.tools.context.epoch + 1);
  expect(opens() - baseline).toBe(3);
  checks.push(
    "zero-terminal native round trip and explicit tool verification create no hidden PTY",
  );
  expect((await history()).map((s) => s.id).sort()).toEqual(
    originalHistory.map((s) => s.id).sort(),
  );
  checks.push(
    "terminal history stays grouped by stable terminal ID and migration/tool verification creates no duplicate history session",
  );
  await main
    .getByRole("button", { name: "新建终端", exact: true })
    .first()
    .click();
  await connected(main, 1);
  expect((await list(main))[0].terminals[0].number).toBe(4);
  checks.push(
    "zero-terminal workspace keeps real SFTP tools, blocks config deletion, and creates terminal 4",
  );
  await expect(top(main)).toBeInViewport();
  await expect
    .poll(async () => (await top(main).boundingBox())?.width ?? 0)
    .toBeGreaterThan(80);
  await main.screenshot({ path: ".qa/server-terminal-tabs-native.png" });
  expect(errors).toEqual([]);
  fs.writeFileSync(
    ".qa/server-terminal-tabs-native-result.json",
    JSON.stringify({ checks, errors }, null, 2),
  );
  console.log(checks.map((c) => "PASS " + c).join("\n"));
} catch (error) {
  console.error("Native verification failed", error);
  console.error("Registry", JSON.stringify(await list(main).catch(() => [])));
  for (const page of context.pages())
    if (!page.isClosed())
      console.error(
        "Page",
        page.url(),
        await page
          .locator("body")
          .innerText()
          .catch(() => ""),
      );
  throw error;
} finally {
  context.off("page", observe);
  for (const page of context.pages())
    if (!page.isClosed() && page.url().includes("workspace="))
      await ipc(page, "plugin:window|destroy", {
        label: await page.evaluate(
          () => window.__TAURI_INTERNALS__.metadata.currentWindow.label,
        ),
      }).catch(() => {});
  if (!main.isClosed()) {
    await main.reload();
    for (const id of saved)
      await ipc(main, "delete_server", { id }).catch(() => {});
  }
  await browser.close();
}
