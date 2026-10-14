// Browser interaction tests with explicit IPC simulation. Real SSH forwarding
// and backend state are tested separately in connection_tests.rs.
import { test, expect, type Page } from "@playwright/test";

async function setup(page: Page, delayed = false) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(
    ({ delayed }) => {
      const w = window as any;
      let serial = 1;
      const calls: any[] = [];
      const callbacks = new Map<number, Function>();
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
        },
      ];
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
          if (command === "list_servers") return structuredClone(servers);
          if (command === "save_server") {
            const { passwordChange, ...server } = args.server;
            server.hasSavedPassword =
              passwordChange.type === "set" ||
              (passwordChange.type === "keep" && server.hasSavedPassword);
            const i = servers.findIndex((s) => s.id === server.id);
            if (i >= 0) servers[i] = server;
            else servers.push({ ...server, id: `new-${serial++}` });
            return structuredClone(server);
          }
          if (command === "open_connection") {
            const id = `connection-${serial++}`;
            channels.set(id, args.events);
            // Exercise events arriving before open_connection returns its ID.
            prompt(id, 0);
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
          if (command === "list_downloads") return [];
          if (
            command === "session_count" ||
            command === "log_session_count" ||
            command === "unsaved_editor_count"
          )
            return 0;
          if (command === "plugin:event|listen") return serial++;
          return null;
        },
      };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    },
    { delayed },
  );
  await page.goto("/");
  await expect(
    page.locator(".server-row").filter({ hasText: "Target B" }),
  ).toBeVisible();
  return errors;
}
async function connect(page: Page) {
  await page.locator(".server-row").filter({ hasText: "Target B" }).dblclick();
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
  await expect(page.locator(".connection-state")).toHaveText("已连接");
}

test("per-hop prompts survive replies delivering the next prompt before IPC acknowledgement", async ({
  page,
}) => {
  const errors = await setup(page);
  await connect(page);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator(".connection-route")).toContainText(
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
  await expect(page.locator(".connection-state")).toHaveText("已断开");
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
  await expect(page.locator(".connection-state")).toHaveText("已断开");
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
  await expect(page.locator(".connection-state")).toHaveText("已断开");
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
