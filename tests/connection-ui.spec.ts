// Browser interaction tests with explicit IPC simulation. Real SSH forwarding
// and backend state are tested separately in connection_tests.rs.
import { test, expect, type Page } from "@playwright/test";
import type { WorkspaceSnapshot } from "../src/SessionWorkspace";

async function setup(
  page: Page,
  delayed = false,
  options: {
    empty?: boolean;
    snapshot?: WorkspaceSnapshot;
    autoConnect?: boolean;
    chrome?: string;
    inventorySize?: number;
    showcase?: boolean;
  } = {},
) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(
    ({ delayed, options }) => {
      const w = window as any;
      let serial = 1;
      const calls: any[] = [];
      const callbacks = new Map<number, Function>();
      const listeners = new Map<number, { event: string; handler: number }>();
      const emitWindow = async (event: string, payload: unknown) => {
        for (const [id, listener] of listeners) {
          if (listener.event === event)
            await callbacks.get(listener.handler)?.({ id, event, payload });
        }
      };
      const channels = new Map<string, any>();
      const servers = [
        {
          id: "a",
          name: "Jump A",
          host: "jump.example",
          port: 22,
          username: "jump-user",
          tags: [],
          authType: "password",
          privateKey: "",
          hasSavedPassword: true,
          jumpServerId: null,
          directHost: null,
          directPort: 22,
        },
        {
          id: "b",
          name: "Target B",
          host: "target.internal",
          port: 22,
          username: "target-user",
          tags: [],
          authType: "password",
          privateKey: "",
          hasSavedPassword: false,
          jumpServerId: "a",
          directHost: null,
          directPort: 22,
        },
      ];
      if (options.inventorySize) {
        for (let i = 2; i < options.inventorySize; i++)
          servers.push({
            ...servers[1],
            id: `fixture-${i}`,
            name: `应用服务器 ${String(i).padStart(3, "0")} / 华东生产集群`,
            host: `node-${i}.internal.example.com`,
            tags: ["生产", "华东", "应用服务"],
          });
      }
      if (options.showcase) {
        const base = {
          ...servers[0],
          hasSavedPassword: false,
          authType: "key",
          privateKey: "C:\\Keys\\workspace_ed25519",
        };
        servers.splice(
          0,
          servers.length,
          {
            ...base,
            id: "a",
            name: "生产网关",
            host: "gateway.example.com",
            username: "admin",
            tags: ["生产环境"],
          },
          {
            ...base,
            id: "b",
            name: "Production API",
            host: "api.internal.example.com",
            username: "deploy",
            jumpServerId: "a",
            tags: ["生产环境", "核心服务"],
          },
          {
            ...base,
            id: "c",
            name: "Staging API",
            host: "staging.example.com",
            username: "deploy",
            tags: ["测试环境"],
          },
          {
            ...base,
            id: "d",
            name: "PostgreSQL",
            host: "db.internal.example.com",
            username: "postgres",
            jumpServerId: "a",
            tags: ["生产环境", "数据库"],
          },
          {
            ...base,
            id: "e",
            name: "开发工作站",
            host: "dev.example.com",
            username: "developer",
            authType: "agent",
            privateKey: "",
            tags: ["开发环境"],
          },
          {
            ...base,
            id: "f",
            name: "备份节点 · 香港",
            host: "backup-hk.example.com",
            username: "backup",
            tags: ["备份"],
          },
        );
      }
      if (options.empty) servers.splice(0);
      const emit = (id: string, event: any) =>
        channels.get(id)?.onmessage(event);
      const hop = (index: number) => ({
        index,
        role: index ? "target" : "jump",
        server: servers[index],
      });
      const prompt = (id: string, index: number) =>
        emit(id, {
          type: "hostKeyRequired",
          challenge: {
            id: `${id}-key-${index}`,
            hop: hop(index),
            fingerprint: `SHA256:key-${index}`,
            previous: null,
          },
        });
      w.__TEST__ = {
        calls,
        servers,
        emit,
        emitWindow,
        ids: () => Array.from(channels.keys()),
      };
      w.isTauri = true;
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback: (callback: Function) => {
          const id = serial++;
          callbacks.set(id, callback);
          return id;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
        invoke: async (command: string, args: any = {}) => {
          calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
          if (command === "window_chrome") return options.chrome ?? "custom";
          if (
            command === "plugin:window|is_maximized" ||
            command === "plugin:window|is_fullscreen"
          )
            return false;
          if (command === "plugin:window|is_focused") return true;
          if (command === "list_servers") return structuredClone(servers);
          if (command === "take_workspace")
            return { payload: options.snapshot, beforeId: null };
          if (command === "terminal_freeze") {
            emit(args.sessionId, { type: "frozen", token: args.token });
            return;
          }
          if (command === "move_workspace") return "fixture-transfer";
          if (command === "delete_server") {
            const index = servers.findIndex((s) => s.id === args.id);
            if (index >= 0) servers.splice(index, 1);
            return;
          }
          if (command === "save_server") {
            const { passwordChange, ...server } = args.server;
            server.hasSavedPassword =
              passwordChange.type === "set" ||
              (passwordChange.type === "keep" && server.hasSavedPassword);
            const i = servers.findIndex((s) => s.id === server.id);
            if (i >= 0) servers[i] = server;
            else {
              server.id = `new-${serial++}`;
              servers.push(server);
            }
            return structuredClone(server);
          }
          if (command === "duplicate_server") {
            const { sourceId, draft, passwordPolicy } = args.request;
            const source = servers.find((s) => s.id === sourceId);
            if (!source)
              throw { kind: "sourceMissing", message: "来源服务器已被删除" };
            if (passwordPolicy.type === "inherit" && !source.hasSavedPassword)
              throw {
                kind: "sourcePasswordMissing",
                message: "来源已保存的密码已被清空",
              };
            const copy = {
              ...draft,
              id: `copy-${serial++}`,
              hasSavedPassword:
                draft.authType === "password" &&
                (passwordPolicy.type === "inherit" ||
                  passwordPolicy.type === "set"),
            };
            servers.push(copy);
            return structuredClone(copy);
          }
          if (command === "open_connection") {
            const id = `connection-${serial++}`;
            channels.set(id, args.events);
            // Exercise events arriving before open_connection returns its ID.
            if (options.autoConnect) {
              emit(id, {
                type: "ready",
                connection: {
                  target: servers[1],
                  hops: [hop(0), hop(1)],
                  identity: "route",
                },
              });
              emit(id, { type: "data", data: btoa("connected to B\r\n") });
            } else prompt(id, 0);
            if (delayed) await new Promise((r) => setTimeout(r, 700));
            return id;
          }
          if (command === "answer_connection") {
            const { connectionId: id, challengeId } = args;
            if (challengeId.endsWith("key-0")) prompt(id, 1);
            else if (challengeId.endsWith("key-1"))
              emit(id, {
                type: "authRequired",
                challenge: { id: `${id}-password`, hop: hop(1), message: null },
              });
            else {
              servers[1].hasSavedPassword = true;
              emit(id, {
                type: "ready",
                connection: {
                  target: servers[1],
                  hops: [hop(0), hop(1)],
                  identity: "route",
                },
              });
              emit(id, { type: "data", data: btoa("connected to B\r\n") });
            }
            return;
          }
          if (command === "close_connection") {
            emit(args.connectionId, {
              type: "closed",
              message: "closed",
              reason: "cancelled",
              error: null,
            });
            return;
          }
          if (command === "list_directory_chain")
            return {
              homePath: "/",
              path: "/",
              directories: [
                { path: "/", directory: { path: "/", entries: [] } },
              ],
            };
          if (command === "detect_docker")
            return { state: "missing", message: "fixture", endpoint: "" };
          if (command === "list_transfers") return [];
          if (command === "list_file_tasks")
            return {
              tasks: [],
              summary: { total: 0, active: 0, attention: 0, completed: 0 },
              nextCursor: null,
            };
          if (
            command === "session_count" ||
            command === "log_session_count" ||
            command === "unsaved_editor_count"
          )
            return 0;
          if (command === "plugin:event|listen") {
            const id = serial++;
            listeners.set(id, { event: args.event, handler: args.handler });
            return id;
          }
          if (command === "plugin:event|unlisten")
            return listeners.delete(args.eventId);
          return null;
        },
      };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    },
    { delayed, options },
  );
  await page.goto(options.snapshot ? "/?workspace=fixture-transfer" : "/");
  if (options.snapshot) return errors;
  await expect(
    page.getByRole("region", { name: "服务器", exact: true }),
  ).toBeVisible();
  if (!options.empty)
    await expect(page.locator('.server-row[data-server-id="b"]')).toBeVisible();
  return errors;
}
async function editSelected(page: Page) {
  await page.locator(".server-row.selected").click({ button: "right" });
  await page.getByRole("menuitem", { name: "编辑", exact: true }).click();
}
async function newSession(page: Page) {
  await page
    .locator(".server-row")
    .filter({ hasText: "Target B" })
    .click({ button: "right" });
  await page.getByRole("menuitem", { name: "新建会话", exact: true }).click();
}
async function connect(page: Page, forceNew = false) {
  if (forceNew) await newSession(page);
  else
    await page
      .locator(".server-row")
      .filter({ hasText: "Target B" })
      .dblclick();
  await expect(
    page.getByRole("dialog", { name: "服务器", exact: true }),
  ).toHaveCount(0);
  let dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("跳板服务器：Jump A");
  await page.screenshot({ path: ".qa/jump-design-fingerprint-light.png" });
  await dialog.getByRole("button", { name: "确认指纹并连接" }).click();
  await expect(dialog).toContainText("目标服务器：Target B");
  await dialog.getByRole("button", { name: "确认指纹并连接" }).click();
  await expect(dialog).toContainText("目标服务器：Target B");
  await expect(dialog.getByLabel("密码", { exact: true })).toBeFocused();
  await page.screenshot({ path: ".qa/jump-design-login-light.png" });
  await dialog.getByLabel("密码", { exact: true }).fill("target-password");
  await dialog.getByRole("button", { name: "连接", exact: true }).click();
  await expect(
    page
      .locator(".session-page[role=tabpanel]:visible")
      .locator(".connection-state"),
  ).toHaveText("已连接");
}

test("per-hop prompts survive replies delivering the next prompt before IPC acknowledgement", async ({
  page,
}) => {
  const errors = await setup(page);
  await connect(page);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(
    page.getByRole("group", { name: "会话连接摘要", exact: true }),
  ).toContainText("Jump A → Target B");
  await expect(page.locator(".terminal-surface")).toContainText(
    "connected to B",
  );
  const calls = await page.evaluate(() => (window as any).__TEST__.calls);
  expect(
    calls.filter((c: any) => c.command === "open_connection"),
  ).toHaveLength(1);
  expect(
    calls.filter((c: any) => c.command === "answer_connection"),
  ).toHaveLength(3);
  expect(errors).toEqual([]);
});

test("closing while open is pending cancels its eventual ID and ignores late Ready", async ({
  page,
}) => {
  const errors = await setup(page, true);
  await page.locator(".server-row").filter({ hasText: "Target B" }).dblclick();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "取消", exact: true })
    .click();
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(0);
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).__TEST__.calls.some(
          (c: any) => c.command === "close_connection",
        ),
      ),
    )
    .toBe(true);
  await page.evaluate(() => {
    const t = (window as any).__TEST__;
    t.emit(t.ids()[0], {
      type: "ready",
      connection: { target: t.servers[1], hops: [], identity: "late" },
    });
  });
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("server editor preserves passwords and explicitly clears or replaces them", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.locator('.server-row[data-server-id="a"]').click();
  await editSelected(page);
  let dialog = page.getByRole("dialog");
  await expect(dialog.getByLabel("密码", { exact: true })).toHaveValue("");
  await expect(
    dialog.getByRole("radio", { name: "通过跳板机", exact: true }),
  ).toBeDisabled();
  await dialog.getByLabel(/^名称/).fill("Renamed A");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls
          .filter((c: any) => c.command === "save_server")
          .at(-1).args.server.passwordChange,
    ),
  ).toEqual({ type: "keep" });
  await editSelected(page);
  await dialog.getByRole("button", { name: "清空已保存密码" }).click();
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls
          .filter((c: any) => c.command === "save_server")
          .at(-1).args.server.passwordChange,
    ),
  ).toEqual({ type: "clear" });
  await editSelected(page);
  await dialog.getByLabel("密码", { exact: true }).fill("replacement");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls
          .filter((c: any) => c.command === "save_server")
          .at(-1).args.server.passwordChange,
    ),
  ).toEqual({ type: "set", value: "replacement" });
  expect(errors).toEqual([]);
});

test("jump configuration only offers direct servers and saves the selected route", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.getByRole("button", { name: "新增服务器", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel(/^名称/).fill("New target");
  await dialog.getByLabel("地址", { exact: true }).fill("private.internal");
  await dialog.getByLabel("用户名", { exact: true }).fill("ops");
  await dialog.getByRole("radio", { name: "通过跳板机", exact: true }).check();
  const jump = dialog.getByLabel("跳板服务器", { exact: true });
  await expect(jump.locator("option")).toHaveCount(2);
  await expect(jump).not.toContainText("Target B");
  await jump.selectOption("a");
  await expect(
    dialog.getByRole("list", { name: "连接路径：本机 → Jump A → New target" }),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  const saved = await page.evaluate(
    () =>
      (window as any).__TEST__.calls
        .filter((c: any) => c.command === "save_server")
        .at(-1).args.server,
  );
  expect(saved.jumpServerId).toBe("a");
  expect(saved.host).toBe("private.internal");
  expect(saved).not.toHaveProperty("password");
  expect(errors).toEqual([]);
});

test("connection settings keep actions visible in light, dark and small windows", async ({
  page,
}) => {
  const errors = await setup(page);
  for (const variant of [
    { name: "light", colorScheme: "light" as const, width: 1440, height: 950 },
    { name: "dark", colorScheme: "dark" as const, width: 1440, height: 950 },
    { name: "small", colorScheme: "light" as const, width: 860, height: 580 },
  ]) {
    await page.emulateMedia({
      colorScheme: variant.colorScheme,
      reducedMotion: "reduce",
    });
    await page.setViewportSize({
      width: variant.width,
      height: variant.height,
    });
    await page.locator(".server-row").filter({ hasText: "Target B" }).click();
    await editSelected(page);
    const dialog = page.getByRole("dialog", { name: "编辑服务器" });
    const direct = dialog.getByRole("radio", { name: "直接连接", exact: true });
    await dialog.getByRole("radio", { name: "通过跳板机" }).focus();
    await page.keyboard.press("ArrowLeft");
    await expect(direct).toBeChecked();
    await page.keyboard.press("ArrowRight");
    await expect(
      dialog.getByRole("radio", { name: "通过跳板机" }),
    ).toBeChecked();
    await expect(dialog.getByLabel("跳板服务器", { exact: true })).toHaveValue(
      "a",
    );
    const save = dialog.getByRole("button", { name: "保存", exact: true });
    await expect(save).toBeInViewport();
    expect(
      await dialog.evaluate(
        (element) => element.scrollWidth <= element.clientWidth,
      ),
    ).toBe(true);
    await dialog.locator(".editor-fields").evaluate((e) => (e.scrollTop = 0));
    await page.screenshot({
      path: `.qa/jump-design-editor-${variant.name}.png`,
    });
    await dialog.getByRole("button", { name: "取消", exact: true }).click();
  }
  await page.emulateMedia({
    colorScheme: "dark",
    reducedMotion: "reduce",
    contrast: "more",
  });
  await page.locator(".server-row").filter({ hasText: "Target B" }).dblclick();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toHaveAccessibleName("核对服务器指纹");
  await expect(
    dialog.getByRole("button", { name: "取消", exact: true }),
  ).toBeFocused();
  await page.evaluate(() => {
    const t = (window as any).__TEST__;
    t.emit(t.ids()[0], {
      type: "hostKeyRequired",
      challenge: {
        id: "changed-key",
        hop: {
          index: 1,
          role: "target",
          server: {
            ...t.servers[1],
            name: "production-内网服务-长名称验证",
            host: "target.production.private.internal.example",
          },
        },
        previous: "SHA256:ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghi",
        fingerprint: "SHA256:ZYXWVUTSRQPONMLKJIHGFEDCBA9876543210abcdefghi",
      },
    });
  });
  await expect(dialog).toHaveAccessibleName("服务器指纹已变化");
  await expect(
    dialog.getByRole("button", { name: "更新指纹并连接" }),
  ).toBeInViewport();
  expect(await dialog.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(
    true,
  );
  await page.screenshot({ path: ".qa/jump-design-fingerprint-dark.png" });
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("authentication can be cancelled while its reply is still pending", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.locator(".server-row").filter({ hasText: "Target B" }).dblclick();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "确认指纹并连接" }).click();
  await dialog.getByRole("button", { name: "确认指纹并连接" }).click();
  await page.evaluate(() => {
    const invoke = (window as any).__TAURI_INTERNALS__.invoke;
    (window as any).__TAURI_INTERNALS__.invoke = (
      command: string,
      args: any,
    ) => {
      if (command === "answer_connection") return new Promise(() => {});
      return invoke(command, args);
    };
  });
  await dialog.getByLabel("密码", { exact: true }).fill("pending-password");
  await dialog.getByRole("button", { name: "连接", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "认证中…" })).toBeDisabled();
  await expect(
    dialog.getByRole("button", { name: "取消", exact: true }),
  ).toBeEnabled();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(0);
  expect(
    await page.evaluate(() =>
      (window as any).__TEST__.calls.some(
        (c: any) => c.command === "close_connection",
      ),
    ),
  ).toBe(true);
  expect(errors).toEqual([]);
});

test("unsaved settings can be kept or discarded without native confirmation dialogs", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    window.confirm = () => {
      throw new Error("native confirm unavailable");
    };
  });
  await page.locator(".server-row").filter({ hasText: "Target B" }).click();
  await editSelected(page);
  const dialog = page.getByRole("dialog", { name: "编辑服务器" });
  await dialog.getByLabel(/^名称/).fill("Unsaved target");
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await expect(dialog.getByRole("alert")).toHaveText(
    "有未保存的修改，是否放弃？",
  );
  await expect(dialog.getByRole("button", { name: "继续编辑" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog.getByLabel(/^名称/)).toHaveValue("Unsaved target");
  await expect(
    dialog.getByRole("button", { name: "保存", exact: true }),
  ).toBeEnabled();
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await dialog.getByRole("button", { name: "放弃修改", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(
    await page.evaluate(() =>
      (window as any).__TEST__.calls.some(
        (c: any) => c.command === "save_server",
      ),
    ),
  ).toBe(false);
  expect(errors).toEqual([]);
});

test("server home starts visible, preserves search and sessions, and enters an existing session", async ({
  page,
}) => {
  const errors = await setup(page);
  const home = page.getByRole("region", { name: "服务器", exact: true });
  await expect(home.getByLabel("搜索服务器")).toBeFocused();
  await home.getByLabel("搜索服务器").fill("no-such-server");
  await expect(home.locator(".server-row")).toHaveCount(0);
  await home.getByLabel("搜索服务器").fill("TARGET target-user");
  await expect(home.locator(".server-row.selected")).toContainText("Target B");
  await connect(page);
  await expect(home).toBeHidden();
  const original = await page
    .locator(".session-tab")
    .getAttribute("data-tab-id");
  await page.getByRole("button", { name: "服务器首页", exact: true }).click();
  await expect(home.getByLabel("搜索服务器")).toHaveValue("TARGET target-user");
  await expect(home.locator(".server-row")).toHaveCount(1);
  await expect(
    home.getByRole("button", { name: "进入会话", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".terminal-surface")).toContainText(
    "connected to B",
  );
  await home.getByRole("button", { name: "进入会话", exact: true }).click();
  await expect(home).toBeHidden();
  await expect(page.locator(".session-tab")).toHaveAttribute(
    "data-tab-id",
    original!,
  );
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.filter(
          (c: any) => c.command === "open_connection",
        ).length,
    ),
  ).toBe(1);
  await page
    .getByRole("button", { name: "关闭 Target B 会话", exact: true })
    .click();
  await expect(home).toBeVisible();
  expect(errors).toEqual([]);
});

test("failed connection retries in the same tab", async ({ page }) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI_INTERNALS__.invoke;
    let first = true;
    w.__TAURI_INTERNALS__.invoke = async (command: string, args: any) => {
      if (command === "open_connection" && first) {
        first = false;
        throw { kind: "ssh", message: "fixture connection refused" };
      }
      return original(command, args);
    };
  });
  await page.locator(".server-row").filter({ hasText: "Target B" }).dblclick();
  const picker = page.getByRole("region", { name: "服务器", exact: true });
  await expect(picker.getByRole("alert")).toContainText(
    "fixture connection refused",
  );
  const id = await page.locator(".session-tab").getAttribute("data-tab-id");
  await connect(page);
  await expect(page.locator(".session-tab")).toHaveCount(1);
  await expect(page.locator(".session-tab")).toHaveAttribute(
    "data-tab-id",
    id!,
  );
  expect(errors).toEqual([]);
});

test("three columns resize independently, survive tab switches, and fit narrow windows", async ({
  page,
}) => {
  const errors = await setup(page);
  await connect(page);
  const first = await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .getAttribute("id");
  const left = page.getByRole("separator", { name: "调整文件区域宽度" });
  const right = page.getByRole("separator", { name: "调整服务区域宽度" });
  const startLeft = Number(await left.getAttribute("aria-valuenow"));
  const startRight = Number(await right.getAttribute("aria-valuenow"));
  await left.press("ArrowRight");
  await right.press("ArrowLeft");
  await expect(left).toHaveAttribute("aria-valuenow", String(startLeft + 10));
  await expect(right).toHaveAttribute("aria-valuenow", String(startRight + 10));
  await page.getByRole("button", { name: "15 分钟", exact: true }).click();
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await connect(page, true);
  await expect(
    page.getByRole("button", { name: "5 分钟", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(2);
  await page.locator(`[id="${first}"]`).click();
  await expect(
    page.getByRole("button", { name: "15 分钟", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(left).toHaveAttribute("aria-valuenow", String(startLeft + 10));
  await expect(right).toHaveAttribute("aria-valuenow", String(startRight + 10));
  for (const [width, height] of [
    [1440, 950],
    [860, 580],
    [650, 480],
  ]) {
    await page.setViewportSize({ width, height });
    await expect
      .poll(async () =>
        page.locator(".session-page[role=tabpanel]:visible").evaluate((el) => {
          const file = el.querySelector(".file-panel")!.getBoundingClientRect();
          const terminal = el
            .querySelector(".terminal-panel")!
            .getBoundingClientRect();
          const service = el
            .querySelector(".resource-service-panel")!
            .getBoundingClientRect();
          return (
            (el.getAttribute("data-layout") === "three"
              ? file.right < terminal.left &&
                terminal.right < service.left &&
                Math.abs(file.top - service.top) < 1
              : el.getAttribute("data-layout") === "two"
                ? file.width === 0 &&
                  terminal.width >= 320 &&
                  terminal.right < service.left
                : file.width === 0 &&
                  service.width === 0 &&
                  terminal.width > 0) && el.scrollWidth <= el.clientWidth
          );
        }),
      )
      .toBe(true);
    await page.screenshot({ path: `.qa/workspace-layout-${width}.png` });
  }
  await page.setViewportSize({ width: 1440, height: 950 });
  await expect(left).toHaveAttribute("aria-valuenow", String(startLeft + 10));
  await expect(right).toHaveAttribute("aria-valuenow", String(startRight + 10));
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await page.screenshot({ path: ".qa/workspace-server-picker.png" });
  await page.locator(".server-row").filter({ hasText: "Target B" }).dblclick();
  await page.keyboard.press("Escape");
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(2);
  await expect(page.locator(`[id="${first}"]`)).toHaveAttribute(
    "aria-selected",
    "true",
  );
  expect(errors).toEqual([]);
});

test("empty inventory can add, edit and delete from home", async ({ page }) => {
  const errors = await setup(page, false, { empty: true });
  const picker = page.getByRole("region", { name: "服务器", exact: true });
  await expect(picker).toContainText("添加第一台服务器");
  await expect(
    picker.getByRole("button", { name: "连接", exact: true }),
  ).toHaveCount(0);
  await expect(picker.getByLabel("搜索服务器")).toBeFocused();
  await picker.getByRole("button", { name: "添加服务器", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "新增服务器" });
  await editor.getByLabel(/^名称/).fill("New server");
  await editor.getByLabel("地址", { exact: true }).fill("fixture.internal");
  await editor.getByLabel("用户名", { exact: true }).fill("ops");
  await editor.getByRole("button", { name: "保存", exact: true }).click();
  await expect(picker.locator(".server-row.selected")).toContainText(
    "New server",
  );
  await picker.locator(".server-row").click({ button: "right" });
  await page.getByRole("menuitem", { name: "编辑", exact: true }).click();
  await page
    .getByRole("dialog", { name: "编辑服务器" })
    .getByRole("button", { name: "取消", exact: true })
    .click();
  await picker.locator(".server-row").click({ button: "right" });
  await page.getByRole("menuitem", { name: "删除服务器", exact: true }).click();
  await page
    .getByRole("dialog", { name: "删除服务器配置？" })
    .getByRole("button", { name: "确认删除" })
    .click();
  await expect(picker).toContainText("添加第一台服务器");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "新建连接", exact: true }),
  ).toBeEnabled();
  expect(errors).toEqual([]);
});

test("moving a workspace carries both widths and restores the same terminal without reconnecting", async ({
  page,
  context,
}) => {
  const errors = await setup(page);
  await connect(page);
  const left = page.getByRole("separator", { name: "调整文件区域宽度" });
  const right = page.getByRole("separator", { name: "调整服务区域宽度" });
  await left.press("ArrowRight");
  await right.press("ArrowLeft");
  const expectedWidths = [
    Number(await left.getAttribute("aria-valuenow")),
    Number(await right.getAttribute("aria-valuenow")),
  ];
  await page.getByRole("tab", { name: "监控", exact: true }).click();
  await page.getByRole("button", { name: "15 分钟", exact: true }).click();
  await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .click({ button: "right" });
  await page
    .getByRole("menuitem", { name: "移到独立窗口", exact: true })
    .click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__TEST__.calls.filter(
            (c: any) => c.command === "move_workspace",
          ).length,
      ),
    )
    .toBe(1);
  const snapshot: WorkspaceSnapshot = await page.evaluate(
    () =>
      (window as any).__TEST__.calls.find(
        (c: any) => c.command === "move_workspace",
      ).args.payload,
  );
  expect(snapshot.layout).toBeTruthy();
  expect(snapshot.resources).toMatchObject({
    tab: "monitor",
    minutes: 15,
    networkInterface: "",
    pinnedMounts: ["/"],
  });
  expect(Math.round(snapshot.layout!.fileWidth)).toBe(expectedWidths[0]);
  expect(Math.round(snapshot.layout!.serviceWidth)).toBe(expectedWidths[1]);
  expect(snapshot.terminal.content).toContain("connected to B");
  const detached = await context.newPage();
  const detachedErrors = await setup(detached, false, { snapshot });
  await expect(
    detached.getByRole("tab", { name: "监控", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(
    detached.getByRole("button", { name: "15 分钟", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    detached
      .locator(".session-page[role=tabpanel]:visible")
      .locator(".connection-state"),
  ).toHaveText("已连接");
  await expect(
    detached.getByRole("separator", { name: "调整文件区域宽度" }),
  ).toHaveAttribute("aria-valuenow", String(expectedWidths[0]));
  await expect(
    detached.getByRole("separator", { name: "调整服务区域宽度" }),
  ).toHaveAttribute("aria-valuenow", String(expectedWidths[1]));
  await expect(detached.locator(".terminal-surface")).toContainText(
    "connected to B",
  );
  const calls = await detached.evaluate(() => (window as any).__TEST__.calls);
  expect(
    calls.filter((c: any) => c.command === "open_connection"),
  ).toHaveLength(0);
  expect(
    calls.filter((c: any) => c.command === "terminal_attach"),
  ).toHaveLength(1);
  await detached.setViewportSize({ width: 650, height: 480 });
  await expect(
    detached.getByRole("navigation", { name: "工作区域" }),
  ).toBeInViewport();
  await expect(detached.locator(".terminal-surface")).toBeVisible();
  await detached
    .getByRole("navigation", { name: "工作区域" })
    .getByRole("button", { name: "监控与服务" })
    .click();
  await expect(
    detached.getByRole("button", { name: "15 分钟", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await detached
    .getByRole("navigation", { name: "工作区域" })
    .getByRole("button", { name: "终端", exact: true })
    .click();
  await expect(detached.locator(".terminal-surface")).toContainText(
    "connected to B",
  );
  await detached.close();
  const legacy = await context.newPage();
  const legacyErrors = await setup(legacy, false, {
    snapshot: {
      ...snapshot,
      layout: undefined,
      resourceWidth: 300,
      fileRatio: 60,
    },
  });
  await expect(
    legacy.getByRole("separator", { name: "调整文件区域宽度" }),
  ).toHaveAttribute("aria-valuenow", "300");
  await expect(
    legacy.getByRole("separator", { name: "调整服务区域宽度" }),
  ).toHaveAttribute("aria-valuenow", "320");
  await legacy.close();
  expect([...errors, ...detachedErrors, ...legacyErrors]).toEqual([]);
});

test("pending connection starts once and cancellation disposes a late connection ID", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = (command: string, args: any) => {
      if (command === "open_connection") {
        w.__TEST__.calls.push({ command, args: { serverId: args.serverId } });
        return new Promise((resolve) => {
          w.__TEST__.finishPending = () => resolve("late-id");
        });
      }
      return original(command, args);
    };
  });
  const picker = page.getByRole("region", { name: "服务器", exact: true });
  await picker
    .locator(".server-row.selected")
    .getByRole("button", { name: "连接", exact: true })
    .dblclick();
  await expect(
    picker.getByRole("button", { name: "连接中…", exact: true }),
  ).toBeDisabled();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.filter(
          (c: any) => c.command === "open_connection",
        ).length,
    ),
  ).toBe(1);
  await picker.getByRole("button", { name: "取消连接", exact: true }).click();
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(0);
  await page.evaluate(() => (window as any).__TEST__.finishPending());
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).__TEST__.calls.some(
          (c: any) =>
            c.command === "close_connection" &&
            c.args.connectionId === "late-id",
        ),
      ),
    )
    .toBe(true);
  expect(errors).toEqual([]);
});

async function createTitlebarTabs(
  page: Page,
  count: number,
  chrome = "custom",
) {
  const errors = await setup(page, false, { autoConnect: true, chrome });
  for (let i = 0; i < count; i++) {
    if (i)
      await page.getByRole("button", { name: "新建连接", exact: true }).click();
    await newSession(page);
    await expect(
      page
        .getByRole("tablist", { name: "SSH 会话", exact: true })
        .getByRole("tab"),
    ).toHaveCount(i + 1);
    await expect(
      page
        .locator(".session-page[role=tabpanel]:visible")
        .locator(".connection-state"),
    ).toHaveText("已连接");
  }
  return errors;
}
async function activeTabVisible(page: Page) {
  await expect
    .poll(() =>
      page.locator('.tab-select[aria-selected="true"]').evaluate((el) => {
        const tab = el.closest(".session-tab")!.getBoundingClientRect();
        const strip = el.closest(".tab-scroll")!.getBoundingClientRect();
        return tab.left >= strip.left - 1 && tab.right <= strip.right + 1;
      }),
    )
    .toBe(true);
}

test("titlebar tabs overflow without consuming the window drag region and remain keyboard accessible", async ({
  page,
}) => {
  test.setTimeout(60000);
  await page.setViewportSize({ width: 860, height: 580 });
  const errors = await createTitlebarTabs(page, 20);
  await expect(page.locator(".window-titlebar .session-tabs")).toHaveCount(1);
  await expect(page.locator(".workspaces .session-tabs")).toHaveCount(0);
  await expect(page.locator(".window-titlebar")).toHaveCSS("height", "48px");
  expect(
    await page
      .locator(".window-drag-region")
      .evaluate((el) => el.getBoundingClientRect().width),
  ).toBeGreaterThanOrEqual(72);
  await expect(
    page.getByRole("button", { name: "全部会话", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "关闭窗口", exact: true }),
  ).toBeInViewport();
  await activeTabVisible(page);
  const oldLeft = await page
    .locator(".tab-scroll")
    .evaluate((el) => el.scrollLeft);
  await page.locator(".tab-scroll").hover();
  await page.mouse.wheel(0, -400);
  await expect
    .poll(() => page.locator(".tab-scroll").evaluate((el) => el.scrollLeft))
    .toBeLessThan(oldLeft - 100);
  // Opening a menu must not snap a manually scrolled strip back to the active tab.
  const manualLeft = await page
    .locator(".tab-scroll")
    .evaluate((el) => el.scrollLeft);
  await page.getByRole("button", { name: "全部会话", exact: true }).click();
  await expect(page.getByRole("menuitemradio")).toHaveCount(20);
  expect(
    await page.locator(".tab-scroll").evaluate((el) => el.scrollLeft),
  ).toBeCloseTo(manualLeft, 0);
  await page.getByRole("menu", { name: "全部会话" }).press("Home");
  await page.keyboard.press("Enter");
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab")
      .first(),
  ).toHaveAttribute("aria-selected", "true");
  await activeTabVisible(page);
  await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .first()
    .press("End");
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab")
      .last(),
  ).toHaveAttribute("aria-selected", "true");
  await activeTabVisible(page);
  await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .last()
    .press("Delete");
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(19);
  await activeTabVisible(page);
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await page.screenshot({
      path: `.qa/titlebar-tabs-many-${colorScheme}.png`,
    });
  }
  await page.setViewportSize({ width: 1360, height: 860 });
  await activeTabVisible(page);
  expect(errors).toEqual([]);
});

test("titlebar tabs menus reorder without clipping and sparse tabs stay next to the new button", async ({
  page,
}) => {
  const errors = await createTitlebarTabs(page, 5);
  const workspaceBeforeMenu = await page.locator(".workspaces").boundingBox();
  const tabBounds = await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .last()
    .boundingBox();
  await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .last()
    .click({ button: "right" });
  const menu = page.getByRole("menu", { name: "会话操作" });
  await expect(menu).toBeVisible();
  const menuBounds = (await menu.boundingBox())!;
  // A visible portal can still regress into normal document flow. It must
  // stay beside the clicked tab without shifting or covering the whole app.
  expect(menuBounds.y).toBeGreaterThanOrEqual(tabBounds!.y);
  expect(menuBounds.y).toBeLessThanOrEqual(tabBounds!.y + tabBounds!.height);
  expect(menuBounds.x).toBeGreaterThanOrEqual(8);
  expect(menuBounds.x + menuBounds.width).toBeLessThanOrEqual(
    page.viewportSize()!.width - 7,
  );
  expect(menuBounds.y + menuBounds.height).toBeLessThanOrEqual(
    page.viewportSize()!.height - 7,
  );
  expect(await page.locator(".workspaces").boundingBox()).toEqual(
    workspaceBeforeMenu,
  );
  expect(await menu.evaluate((el) => el.parentElement === document.body)).toBe(
    true,
  );
  await page.getByRole("menuitem", { name: "向左移动", exact: true }).click();
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab")
      .nth(3),
  ).toHaveAttribute("aria-selected", "true");
  const selectedId = await page
    .locator('.tab-select[role="tab"][aria-selected="true"]')
    .getAttribute("id");
  await page
    .locator('.tab-select[role="tab"][aria-selected="true"]')
    .press("Alt+ArrowLeft");
  expect(
    await page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab")
      .nth(2)
      .getAttribute("id"),
  ).toBe(selectedId);
  await page
    .locator('.tab-select[role="tab"][aria-selected="true"]')
    .click({ button: "right" });
  await page.keyboard.press("Escape");
  await expect(
    page.locator('.tab-select[role="tab"][aria-selected="true"]'),
  ).toBeFocused();
  await page
    .locator('.tab-select[role="tab"][aria-selected="true"]')
    .click({ button: "right" });
  await page.getByRole("button", { name: "终端历史", exact: true }).click();
  await expect(menu).toHaveCount(0);
  while (
    (await page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab")
      .count()) > 1
  ) {
    await page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab")
      .last()
      .press("Delete");
  }
  await expect(
    page.getByRole("button", { name: "全部会话", exact: true }),
  ).toHaveCount(0);
  const distance = await page.evaluate(
    () =>
      document.querySelector(".new-session")!.getBoundingClientRect().left -
      document.querySelector(".session-tab")!.getBoundingClientRect().right,
  );
  expect(distance).toBeGreaterThanOrEqual(0);
  expect(distance).toBeLessThan(12);
  await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .press("Delete");
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "新建连接", exact: true }),
  ).toBeEnabled();
  expect(errors).toEqual([]);
});

test("titlebar tab drop targets use live geometry, auto-scroll, and exclude window buttons", async ({
  page,
}) => {
  const errors = await createTitlebarTabs(page, 5);
  await page.setViewportSize({ width: 860, height: 580 });
  await expect(
    page.getByRole("button", { name: "全部会话", exact: true }),
  ).toBeVisible();
  await activeTabVisible(page);
  const emit = (phase: string, x: number, y: number, token = "drop-test") =>
    page.evaluate(
      ({ phase, x, y, token }) =>
        (window as any).__TEST__.emitWindow("desktop-drag", {
          phase,
          x,
          y,
          token,
          kind: "workspaceTab",
        }),
      { phase, x, y, token },
    );
  const bounds = await page.locator(".tab-scroll").boundingBox();
  const before = await page
    .locator(".tab-scroll")
    .evaluate((el) => el.scrollLeft);
  await emit("over", bounds!.x + 2, bounds!.y + 16);
  await expect
    .poll(() => page.locator(".tab-scroll").evaluate((el) => el.scrollLeft))
    .toBeLessThan(before - 50);
  await emit("leave", 0, 0);
  // A single hover event keeps edge scrolling until the drag leaves or ends.
  const first = await page.locator(".session-tab").first().boundingBox();
  await emit(
    "drop",
    Math.max(bounds!.x + 5, first!.x + 5),
    bounds!.y + 16,
    "insert",
  );
  const blank = await page.locator(".window-drag-region").boundingBox();
  await emit("over", blank!.x + 10, blank!.y + 20);
  await expect(page.locator(".window-drag-region")).toHaveAttribute(
    "data-tab-drop",
    "true",
  );
  await emit("drop", blank!.x + 10, blank!.y + 20, "append");
  await expect(page.locator(".window-drag-region")).not.toHaveAttribute(
    "data-tab-drop",
  );
  const button = await page
    .getByRole("button", { name: "关闭窗口", exact: true })
    .boundingBox();
  await emit("drop", button!.x + 10, button!.y + 20, "ignored");
  const drops = await page.evaluate(() =>
    (window as any).__TEST__.calls.filter((c: any) => c.command === "drop_tab"),
  );
  expect(drops).toHaveLength(2);
  expect(drops[0].args.beforeId).toBeTruthy();
  expect(drops[1].args).toEqual({ token: "append", beforeId: null });
  expect(errors).toEqual([]);
});

test("titlebar tabs accept empty-window drops and reserve macOS controls", async ({
  page,
}) => {
  const errors = await setup(page, false, { empty: true, chrome: "overlay" });
  await page.keyboard.press("Escape");
  await expect(page.locator(".window-titlebar")).toHaveCSS(
    "padding-left",
    "88px",
  );
  const blank = await page.locator(".window-drag-region").boundingBox();
  await page.evaluate(
    ({ x, y }) =>
      (window as any).__TEST__.emitWindow("desktop-drag", {
        kind: "workspaceTab",
        phase: "drop",
        token: "empty-target",
        x,
        y,
      }),
    { x: blank!.x + 20, y: blank!.y + 20 },
  );
  const calls = await page.evaluate(() => (window as any).__TEST__.calls);
  expect(calls.find((c: any) => c.command === "drop_tab").args).toEqual({
    token: "empty-target",
    beforeId: null,
  });
  expect(
    await page
      .locator(".new-session")
      .evaluate((el) => el.getBoundingClientRect().left),
  ).toBeGreaterThan(88);
  expect(errors).toEqual([]);
});

test("sidebar and communication preferences survive window migration without copying monitor data", async ({
  page,
  context,
}) => {
  const errors = await setup(page);
  await connect(page);
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  await page.getByRole("textbox", { name: "搜索容器" }).fill("api-service");
  await page
    .getByRole("combobox", { name: "容器状态筛选" })
    .selectOption("running");
  await page
    .getByRole("combobox", { name: "Docker 排序" })
    .selectOption("attention");
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await page
    .getByRole("group", { name: "通信时间范围" })
    .getByRole("button", { name: "15 分钟" })
    .click();
  await page.keyboard.press("Escape");
  await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .click({ button: "right" });
  await page
    .getByRole("menuitem", { name: "移到独立窗口", exact: true })
    .click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__TEST__.calls.filter(
            (c: any) => c.command === "move_workspace",
          ).length,
      ),
    )
    .toBe(1);
  const snapshot: WorkspaceSnapshot = await page.evaluate(
    () =>
      (window as any).__TEST__.calls.find(
        (c: any) => c.command === "move_workspace",
      ).args.payload,
  );
  expect(snapshot.view).toEqual({ pane: "terminal" });
  expect(snapshot.connectionPreferences).toEqual({
    minutes: 15,
    scope: "total",
  });
  expect(snapshot.docker).toMatchObject({
    page: "containers",
    queries: { containers: "api-service" },
    filter: "running",
    sort: "attention",
  });
  expect(snapshot.docker).not.toHaveProperty("containers");
  expect(snapshot.docker).not.toHaveProperty("images");
  expect(snapshot.docker).not.toHaveProperty("status");
  expect(snapshot.resources).not.toHaveProperty("cursor");
  const detached = await context.newPage();
  const detachedErrors = await setup(detached, false, { snapshot });
  await detached.setViewportSize({ width: 650, height: 480 });
  await expect(detached.locator(".responsive-workspace")).toHaveAttribute(
    "data-layout",
    "single",
  );
  await detached
    .getByRole("navigation", { name: "工作区域" })
    .getByRole("button", { name: "监控与服务" })
    .click();
  await expect(detached.getByRole("textbox", { name: "搜索容器" })).toHaveValue(
    "api-service",
  );
  await expect(
    detached.getByRole("combobox", { name: "容器状态筛选" }),
  ).toHaveValue("running");
  await detached
    .getByRole("navigation", { name: "工作区域" })
    .getByRole("button", { name: "终端", exact: true })
    .click();
  await expect(detached.locator(".terminal-surface")).toContainText(
    "connected to B",
  );
  expect(
    await detached.evaluate(() =>
      (window as any).__TEST__.calls.filter(
        (c: any) => c.command === "open_connection",
      ),
    ),
  ).toHaveLength(0);
  await detached.close();
  const fresh = await context.newPage();
  const freshErrors = await setup(fresh);
  await connect(fresh);
  await expect(
    fresh.getByRole("tab", { name: "监控", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(
    fresh.getByRole("button", { name: "5 分钟", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await fresh.getByRole("tab", { name: "Docker", exact: true }).click();
  await expect(fresh.getByRole("textbox", { name: "搜索容器" })).toHaveValue(
    "",
  );
  await expect(
    fresh.getByRole("combobox", { name: "容器状态筛选" }),
  ).toHaveValue("all");
  await fresh.close();
  expect([...errors, ...detachedErrors, ...freshErrors]).toEqual([]);
});

async function copyServer(page: Page, id = "a") {
  const row = page.locator(`.server-row[data-server-id="${id}"]`);
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: "复制配置", exact: true }).click();
  const editor = page.getByRole("dialog", {
    name: "复制服务器配置",
    exact: true,
  });
  await expect(editor).toBeVisible();
  return editor;
}

test("copy cancellation creates nothing; saved copy inherits privately and reveals through a filter", async ({
  page,
}) => {
  const errors = await setup(page);
  let editor = await copyServer(page);
  await expect(editor.getByLabel(/^名称/)).toHaveValue("Jump A - 副本");
  await expect(editor.getByLabel(/^名称/)).toBeFocused();
  expect(
    await editor
      .getByLabel(/^名称/)
      .evaluate(
        (el: HTMLInputElement) => el.selectionEnd! - el.selectionStart!,
      ),
  ).toBe("Jump A - 副本".length);
  await expect(editor.getByLabel("密码", { exact: true })).toHaveValue("");
  await expect(
    editor.getByRole("checkbox", { name: "复用来源已保存的登录密码" }),
  ).toBeChecked();
  await page.screenshot({ path: ".qa/server-copy-editor.png" });
  await editor.getByRole("button", { name: "取消", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.filter((c: any) =>
          ["duplicate_server", "save_server"].includes(c.command),
        ).length,
    ),
  ).toBe(0);
  await page.getByLabel("搜索服务器").fill("jump.example");
  editor = await copyServer(page);
  await editor.getByLabel(/^名称/).fill("Production copy");
  await editor.getByLabel("地址", { exact: true }).fill("production.example");
  await editor.getByRole("button", { name: "保存副本", exact: true }).click();
  await expect(editor).toHaveCount(0);
  await expect(page.getByLabel("搜索服务器")).toHaveValue("jump.example");
  await expect(
    page.getByRole("status").filter({ hasText: "副本已保存" }),
  ).toContainText("当前筛选下不可见");
  await page.getByRole("button", { name: "查看配置", exact: true }).click();
  await expect(page.locator(".server-row.selected")).toContainText(
    "Production copy",
  );
  const state = await page.evaluate(() => ({
    calls: (window as any).__TEST__.calls,
    servers: (window as any).__TEST__.servers,
  }));
  const request = state.calls.find((c: any) => c.command === "duplicate_server")
    .args.request;
  expect(request.sourceId).toBe("a");
  expect(request.passwordPolicy).toEqual({ type: "inherit" });
  expect(request.draft).not.toHaveProperty("id");
  expect(request.draft).not.toHaveProperty("hasSavedPassword");
  expect(request.draft).not.toHaveProperty("passwordChange");
  expect(request.draft).not.toHaveProperty("password");
  expect(state.servers.find((s: any) => s.id === "a")).toMatchObject({
    name: "Jump A",
    host: "jump.example",
  });
  expect(
    state.servers.find(
      (s: any) => s.id !== "a" && s.name === "Production copy",
    ),
  ).toMatchObject({ host: "production.example", hasSavedPassword: true });
  expect(errors).toEqual([]);
});

test("copy preserves key configuration and clears inherited transfer endpoint when host changes", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.locator('.server-row[data-server-id="b"]').click();
  await editSelected(page);
  let editor = page.getByRole("dialog", { name: "编辑服务器" });
  await editor.getByLabel("认证方式").selectOption("key");
  await editor.getByLabel("本地私钥路径").fill("C:\\Keys\\production.pem");
  await editor.getByLabel("真实地址", { exact: true }).fill("10.0.0.8");
  await editor.getByLabel("直连端口", { exact: true }).fill("2200");
  await editor.getByRole("button", { name: "保存", exact: true }).click();
  editor = await copyServer(page, "b");
  await expect(editor.getByLabel("本地私钥路径")).toHaveValue(
    "C:\\Keys\\production.pem",
  );
  await expect(editor.getByLabel("跳板服务器", { exact: true })).toHaveValue(
    "a",
  );
  await expect(editor.getByLabel("真实地址", { exact: true })).toHaveValue(
    "10.0.0.8",
  );
  await editor.getByLabel("地址", { exact: true }).fill("new-target.internal");
  await expect(editor.getByLabel("真实地址", { exact: true })).toHaveValue("");
  await expect(editor.getByLabel("直连端口", { exact: true })).toHaveValue(
    "22",
  );
  await expect(editor.getByRole("status")).toContainText("已清空");
  await editor.getByRole("button", { name: "保存副本", exact: true }).click();
  const request = await page.evaluate(
    () =>
      (window as any).__TEST__.calls.find(
        (c: any) => c.command === "duplicate_server",
      ).args.request,
  );
  expect(request.passwordPolicy).toEqual({ type: "clear" });
  expect(request.draft).toMatchObject({
    authType: "key",
    privateKey: "C:\\Keys\\production.pem",
    jumpServerId: "a",
    directHost: null,
    directPort: 22,
  });
  editor = await copyServer(page, "b");
  await editor.getByLabel("真实地址", { exact: true }).fill("10.0.0.9");
  await editor
    .getByLabel("地址", { exact: true })
    .fill("manually-overridden.internal");
  await expect(editor.getByLabel("真实地址", { exact: true })).toHaveValue(
    "10.0.0.9",
  );
  expect(errors).toEqual([]);
});

test("missing copy source keeps the draft and can save as an ordinary new configuration", async ({
  page,
}) => {
  const errors = await setup(page);
  const editor = await copyServer(page);
  await editor.getByLabel(/^名称/).fill("Draft survives");
  await page.evaluate(() => (window as any).__TEST__.servers.splice(0, 1));
  await editor.getByRole("button", { name: "保存副本", exact: true }).click();
  await expect(editor.getByRole("alert")).toContainText("来源服务器");
  await expect(editor.getByLabel(/^名称/)).toHaveValue("Draft survives");
  await editor.getByRole("button", { name: "保留草稿，改为普通新建" }).click();
  const create = page.getByRole("dialog", { name: "新增服务器", exact: true });
  await expect(create.getByLabel(/^名称/)).toHaveValue("Draft survives");
  await create.getByRole("button", { name: "保存", exact: true }).click();
  await expect(create).toHaveCount(0);
  const saved = await page.evaluate(() =>
    (window as any).__TEST__.servers.find(
      (s: any) => s.name === "Draft survives",
    ),
  );
  expect(saved.hasSavedPassword).toBe(false);
  expect(errors).toEqual([]);
});

test("removed source password keeps copy open and allows a replacement password", async ({
  page,
}) => {
  const errors = await setup(page);
  const editor = await copyServer(page);
  await page.evaluate(
    () => ((window as any).__TEST__.servers[0].hasSavedPassword = false),
  );
  await editor.getByRole("button", { name: "保存副本", exact: true }).click();
  await expect(editor.getByRole("alert")).toContainText("密码已被清空");
  await editor.getByLabel("密码", { exact: true }).fill("replacement-secret");
  await editor.getByRole("button", { name: "保存副本", exact: true }).click();
  await expect(editor).toHaveCount(0);
  const state = await page.evaluate(() => ({
    calls: (window as any).__TEST__.calls,
    count: (window as any).__TEST__.servers.length,
  }));
  expect(state.count).toBe(3);
  expect(
    state.calls.filter((c: any) => c.command === "duplicate_server").at(-1).args
      .request.passwordPolicy,
  ).toEqual({ type: "set", value: "replacement-secret" });
  expect(errors).toEqual([]);
});

test("save and connect saves one copy even when connection fails and retries", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const w = window as any;
    const invoke = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (command: string, args: any) => {
      if (command === "open_connection") {
        w.__TEST__.calls.push({ command, args: { serverId: args.serverId } });
        throw { kind: "ssh", message: "copy connection refused" };
      }
      return invoke(command, args);
    };
  });
  const editor = await copyServer(page, "b");
  await editor.getByRole("button", { name: "保存并连接", exact: true }).click();
  await expect(editor).toHaveCount(0);
  const home = page.getByRole("region", { name: "服务器", exact: true });
  await expect(home.getByRole("alert")).toContainText(
    "copy connection refused",
  );
  const tabId = await page.locator(".session-tab").getAttribute("data-tab-id");
  await home.getByRole("button", { name: "重试", exact: true }).click();
  await expect(home.getByRole("alert")).toContainText(
    "copy connection refused",
  );
  await expect(page.locator(".session-tab")).toHaveAttribute(
    "data-tab-id",
    tabId!,
  );
  const calls = await page.evaluate(() => (window as any).__TEST__.calls);
  expect(
    calls.filter((c: any) => c.command === "duplicate_server"),
  ).toHaveLength(1);
  expect(
    calls.filter((c: any) => c.command === "open_connection"),
  ).toHaveLength(2);
  expect(errors).toEqual([]);
});

test("finishing a background connection respects navigation and entering uses the most recently visited session", async ({
  page,
}) => {
  const errors = await setup(page, false, { autoConnect: true });
  await newSession(page);
  await expect(page.locator('.tab-select[aria-selected="true"]')).toHaveCount(
    1,
  );
  const first = await page.locator(".session-tab").getAttribute("data-tab-id");
  await page.getByRole("button", { name: "服务器首页", exact: true }).click();
  await page.evaluate(() => {
    const w = window as any;
    const invoke = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (command: string, args: any) => {
      if (command === "open_connection") {
        w.__TEST__.finishBackground = () =>
          args.events.onmessage({
            type: "ready",
            connection: {
              target: w.__TEST__.servers[1],
              hops: [],
              identity: "second",
            },
          });
        return "background-id";
      }
      return invoke(command, args);
    };
  });
  await newSession(page);
  await expect(page.locator(".session-tab")).toHaveCount(2);
  await page
    .locator(`.session-tab[data-tab-id="${first}"] .tab-select`)
    .click();
  await page.getByRole("button", { name: "服务器首页", exact: true }).click();
  await page.evaluate(() => (window as any).__TEST__.finishBackground());
  const home = page.getByRole("region", { name: "服务器", exact: true });
  await expect(home).toBeVisible();
  await expect(home.locator('[data-server-id="b"]')).toContainText("2 个会话");
  await home.getByRole("button", { name: "进入会话", exact: true }).click();
  await expect(
    page.locator(`.session-tab[data-tab-id="${first}"] .tab-select`),
  ).toHaveAttribute("aria-selected", "true");
  expect(errors).toEqual([]);
});

test("large server home remains readable and scrollable in light, dark and scaled windows", async ({
  page,
}) => {
  const errors = await setup(page, false, { inventorySize: 500 });
  const home = page.getByRole("region", { name: "服务器", exact: true });
  await expect(home.locator(".server-row")).toHaveCount(500);
  for (const variant of [
    { width: 1360, height: 860, dark: false },
    { width: 1360, height: 860, dark: true },
    { width: 860, height: 580, dark: false },
    { width: 688, height: 464, dark: true },
  ]) {
    await page.setViewportSize({
      width: variant.width,
      height: variant.height,
    });
    await page
      .getByRole("button", {
        name: variant.dark ? "黑夜主题" : "白天主题",
        exact: true,
      })
      .click();
    await expect(
      home.getByRole("button", { name: "新增服务器", exact: true }),
    ).toBeInViewport();
    const homeTab = await page
      .getByRole("button", { name: "服务器首页", exact: true })
      .boundingBox();
    const plus = await page
      .getByRole("button", { name: "新建连接", exact: true })
      .boundingBox();
    expect(homeTab!.y).toBeGreaterThanOrEqual(0);
    expect(Math.abs(homeTab!.y - plus!.y)).toBeLessThan(3);
    expect(homeTab!.x + homeTab!.width).toBeLessThanOrEqual(plus!.x);

    await expect(home.locator(".catalog-name").first()).toHaveCSS(
      "font-size",
      "16px",
    );
    expect(await home.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(
      true,
    );
    expect(
      await home
        .locator(".server-catalog")
        .evaluate(
          (el) =>
            el.scrollWidth <= el.clientWidth &&
            el.clientHeight > 180 &&
            el.scrollHeight > el.clientHeight,
        ),
    ).toBe(true);
    await page.screenshot({
      path: `.qa/server-home-${variant.width}-${variant.dark ? "dark" : "light"}.png`,
    });
  }
  await home.locator(".server-catalog").evaluate((el) => (el.scrollTop = 1600));
  const scroll = await home
    .locator(".server-catalog")
    .evaluate((el) => el.scrollTop);
  await home.getByLabel("搜索服务器").fill("node-499 生产");
  await expect(home.locator(".server-row")).toHaveCount(1);
  await home.getByLabel("搜索服务器").press("ArrowDown");
  await expect(home.locator(".server-row.selected")).toContainText("499");
  expect(scroll).toBeGreaterThan(0);
  expect(errors).toEqual([]);
});

test("server details follow keyboard selection and copy the selected configuration", async ({
  page,
}) => {
  const errors = await setup(page);
  const home = page.getByRole("region", { name: "服务器", exact: true });
  const details = page.getByRole("complementary", { name: "服务器详情" });
  await expect(
    details.getByRole("heading", { name: "Jump A", exact: true }),
  ).toBeVisible();
  await home.getByLabel("搜索服务器").press("ArrowDown");
  await expect(
    details.getByRole("heading", { name: "Target B", exact: true }),
  ).toBeVisible();
  await expect(
    details.getByRole("region", { name: "连接路径", exact: true }),
  ).toContainText("Jump A");
  await details.getByRole("button", { name: "复制配置", exact: true }).click();
  const editor = page.getByRole("dialog", {
    name: "复制服务器配置",
    exact: true,
  });
  await expect(editor.getByLabel(/^名称/)).toHaveValue("Target B - 副本");
  await expect(editor.getByLabel("跳板服务器", { exact: true })).toHaveValue(
    "a",
  );
  await editor.getByRole("button", { name: "取消", exact: true }).click();
  await details.getByRole("button", { name: "编辑", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "编辑服务器" }).getByLabel(/^名称/),
  ).toHaveValue("Target B");
  expect(errors).toEqual([]);
});

test("compact server details preserve list context, focus, and scroll when returning", async ({
  page,
}) => {
  const errors = await setup(page, false, { inventorySize: 500 });
  await page.setViewportSize({ width: 860, height: 580 });
  const home = page.getByRole("region", { name: "服务器", exact: true });
  const details = page.getByRole("complementary", { name: "服务器详情" });
  await expect(details).toBeHidden();
  await home.getByLabel("搜索服务器").fill("生产");
  await home.locator(".server-catalog").evaluate((el) => (el.scrollTop = 900));
  const row = home.locator('.server-row[data-server-id="fixture-12"]');
  await row.locator(".catalog-name").click();
  const before = await home
    .locator(".server-catalog")
    .evaluate((el) => el.scrollTop);
  await page
    .getByRole("button", { name: "查看服务器详情", exact: true })
    .click();
  await expect(details).toBeVisible();
  await expect(
    details.getByRole("heading", { name: /应用服务器 012/ }),
  ).toBeFocused();
  await expect(home.locator(".server-catalog")).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(details).toBeHidden();
  await expect(home.getByLabel("搜索服务器")).toHaveValue("生产");
  await expect(
    page.getByRole("button", { name: "查看服务器详情", exact: true }),
  ).toBeFocused();
  await expect(row).toHaveClass(/selected/);
  expect(
    await home.locator(".server-catalog").evaluate((el) => el.scrollTop),
  ).toBe(before);
  expect(errors).toEqual([]);
});

test("refined server library visual review with realistic configurations and accessible states", async ({
  page,
}) => {
  const errors = await setup(page, false, {
    showcase: true,
    autoConnect: true,
  });
  const home = page.getByRole("region", { name: "服务器", exact: true });
  await home.locator('[data-server-id="b"] .catalog-connect').click();
  await expect(page.locator('.tab-select[aria-selected="true"]')).toHaveCount(
    1,
  );
  await page.getByRole("button", { name: "服务器首页", exact: true }).click();
  const details = page.getByRole("complementary", { name: "服务器详情" });
  await expect(
    details.getByRole("heading", { name: "Production API", exact: true }),
  ).toBeVisible();
  for (const dark of [false, true]) {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page
      .getByRole("button", {
        name: dark ? "黑夜主题" : "白天主题",
        exact: true,
      })
      .click();
    await expect(home.locator(".server-row.selected")).toHaveCSS(
      "background-color",
      dark ? "rgb(48, 52, 60)" : "rgb(255, 255, 255)",
    );
    await expect(
      details.getByRole("button", { name: "复制配置", exact: true }),
    ).toHaveCSS(
      "background-color",
      dark ? "rgb(36, 37, 39)" : "rgb(255, 255, 255)",
    );
    await page.screenshot({
      path: `.qa/server-library-v2-${dark ? "dark" : "light"}.png`,
    });
    await page.setViewportSize({ width: 860, height: 580 });
    await expect(details).toBeHidden();
    await page.screenshot({
      path: `.qa/server-library-v2-small-${dark ? "dark" : "light"}.png`,
    });
    await page
      .getByRole("button", { name: "查看服务器详情", exact: true })
      .click();
    await expect(
      details.getByRole("button", { name: "进入已有会话", exact: true }),
    ).toBeInViewport();
    expect(
      await details.evaluate((el) => el.scrollWidth <= el.clientWidth),
    ).toBe(true);
    await page.screenshot({
      path: `.qa/server-library-v2-detail-${dark ? "dark" : "light"}.png`,
    });
    await details
      .getByRole("button", { name: "返回列表", exact: true })
      .click();
  }
  await page.setViewportSize({ width: 1360, height: 900 });
  await page.getByRole("button", { name: "白天主题", exact: true }).click();
  await details.getByRole("button", { name: "复制配置", exact: true }).click();
  const editor = page.getByRole("dialog", {
    name: "复制服务器配置",
    exact: true,
  });
  for (const width of [1360, 860]) {
    await page.setViewportSize({ width, height: width === 860 ? 580 : 900 });
    await expect(
      editor.getByRole("button", { name: "保存并连接", exact: true }),
    ).toBeInViewport();
    await expect(editor.locator(".dialog-actions .primary")).toHaveCount(1);
    expect(
      await editor.evaluate((el) => el.scrollWidth <= el.clientWidth),
    ).toBe(true);
    await page.screenshot({ path: `.qa/server-library-v2-copy-${width}.png` });
  }
  await editor.getByRole("button", { name: "取消", exact: true }).click();
  await page.emulateMedia({ reducedMotion: "reduce", forcedColors: "active" });
  await home.getByLabel("搜索服务器").focus();
  await home.getByLabel("搜索服务器").press("ArrowDown");
  await expect(home.locator(".server-row.selected")).toHaveCSS(
    "outline-style",
    "solid",
  );
  await expect(home.locator(".server-row.selected")).toHaveCSS(
    "transition-duration",
    "0s",
  );
  await page.screenshot({ path: ".qa/server-library-v2-high-contrast.png" });
  expect(errors).toEqual([]);
});

test("returning home restores keyboard focus to the open compact details", async ({
  page,
}) => {
  const errors = await setup(page, false, { autoConnect: true });
  await page.setViewportSize({ width: 860, height: 580 });
  const home = page.getByRole("region", { name: "服务器", exact: true });
  await home.locator('[data-server-id="b"] .catalog-connect').click();
  await expect(home).toBeHidden();
  await page.getByRole("button", { name: "服务器首页", exact: true }).click();
  await page
    .getByRole("button", { name: "查看服务器详情", exact: true })
    .click();
  const details = page.getByRole("complementary", { name: "服务器详情" });
  await expect(
    details.getByRole("heading", { name: "Target B", exact: true }),
  ).toBeFocused();
  await details
    .getByRole("button", { name: "进入已有会话", exact: true })
    .click();
  await expect(home).toBeHidden();
  await page.getByRole("button", { name: "服务器首页", exact: true }).click();
  await expect(details).toBeVisible();
  await expect(
    details.getByRole("heading", { name: "Target B", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(home.getByLabel("搜索服务器")).toBeVisible();
  expect(errors).toEqual([]);
});
