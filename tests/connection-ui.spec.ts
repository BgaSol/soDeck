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
  await expect(page.getByRole("dialog", { name: /^服务器/ })).toHaveCount(0);
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  if (!options.empty)
    await expect(
      page.locator(".server-row").filter({ hasText: "Target B" }),
    ).toBeVisible();
  return errors;
}
async function connect(page: Page) {
  await page.locator(".server-row").filter({ hasText: "Target B" }).dblclick();
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
  await page.locator(".session-information summary").click();
  await expect(page.locator(".session-information-content")).toContainText(
    "Jump A → Target B",
  );
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
  await page.locator(".server-row").filter({ hasText: "Jump A" }).click();
  await page.getByRole("button", { name: "编辑", exact: true }).click();
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
  await page.getByRole("button", { name: "编辑", exact: true }).click();
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
  await page.getByRole("button", { name: "编辑", exact: true }).click();
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
  await page.getByRole("button", { name: "新增", exact: true }).click();
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
    await page.getByRole("button", { name: "编辑", exact: true }).click();
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
  await page.getByRole("button", { name: "编辑", exact: true }).click();
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

test("server picker filters hidden selections, supports Escape, and preserves the active session", async ({
  page,
}) => {
  const errors = await setup(page);
  const picker = page.getByRole("dialog", { name: /^服务器/ });
  await expect(page.locator(".server-panel")).toHaveCount(0);
  await expect(
    page
      .locator(".window-titlebar")
      .getByRole("button", { name: "本地日志", exact: true }),
  ).toHaveCount(1);
  await picker.getByLabel("搜索服务器").fill("no-such-server");
  await expect(
    picker.getByRole("button", { name: "连接", exact: true }),
  ).toBeDisabled();
  await picker.getByLabel("搜索服务器").fill("target.internal");
  await expect(picker.locator(".server-row.selected")).toContainText(
    "Target B",
  );
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "新建连接", exact: true }),
  ).toBeFocused();
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await connect(page);
  const original = await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .getAttribute("id");
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await picker.getByLabel("搜索服务器").fill("");
  await picker.locator(".server-row").filter({ hasText: "Jump A" }).click();
  await page.keyboard.press("Escape");
  await expect(
    page
      .getByRole("tablist", { name: "SSH 会话", exact: true })
      .getByRole("tab"),
  ).toHaveAttribute("id", original!);
  await expect(
    page
      .locator(".session-page[role=tabpanel]:visible")
      .locator(".connection-state"),
  ).toHaveText("已连接");
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
  const picker = page.getByRole("dialog", { name: /^服务器/ });
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
  await connect(page);
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

test("empty inventory can add, edit and delete inside the picker", async ({
  page,
}) => {
  const errors = await setup(page, false, { empty: true });
  const picker = page.getByRole("dialog", { name: /^服务器/ });
  await expect(picker).toContainText("还没有服务器");
  await expect(
    picker.getByRole("button", { name: "连接", exact: true }),
  ).toHaveCount(0);
  await expect(
    picker.getByRole("button", { name: "添加服务器", exact: true }),
  ).toBeFocused();
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
  await picker.getByRole("menuitem", { name: "编辑", exact: true }).click();
  await page
    .getByRole("dialog", { name: "编辑服务器" })
    .getByRole("button", { name: "取消", exact: true })
    .click();
  await picker.getByRole("button", { name: "删除", exact: true }).click();
  await page
    .getByRole("dialog", { name: "删除服务器配置？" })
    .getByRole("button", { name: "确认删除" })
    .click();
  await expect(picker).toContainText("还没有服务器");
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
  await page.getByRole("tab", { name: "总览", exact: true }).click();
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
    collapsed: ["traffic"],
  });
  expect(Math.round(snapshot.layout!.fileWidth)).toBe(expectedWidths[0]);
  expect(Math.round(snapshot.layout!.serviceWidth)).toBe(expectedWidths[1]);
  expect(snapshot.terminal.content).toContain("connected to B");
  const detached = await context.newPage();
  const detachedErrors = await setup(detached, false, { snapshot });
  await expect(
    detached.getByRole("tab", { name: "总览", exact: true }),
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
    .getByRole("button", { name: "检查器" })
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
  const picker = page.getByRole("dialog", { name: /^服务器/ });
  await picker.getByRole("button", { name: "连接", exact: true }).dblclick();
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
    await page
      .locator(".server-row")
      .filter({ hasText: "Target B" })
      .dblclick();
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

test("session-only inspector preferences survive focused window migration without copying monitor data", async ({
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
  await page.getByRole("button", { name: "展开检查器" }).click();
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
  expect(snapshot.view).toEqual({ expanded: true, pane: "services" });
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
    "focus",
  );
  await expect(detached.getByRole("textbox", { name: "搜索容器" })).toHaveValue(
    "api-service",
  );
  await expect(
    detached.getByRole("combobox", { name: "容器状态筛选" }),
  ).toHaveValue("running");
  await detached.getByRole("button", { name: "返回终端", exact: true }).click();
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
    fresh.getByRole("tab", { name: "总览", exact: true }),
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
