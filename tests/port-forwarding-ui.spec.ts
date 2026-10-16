import { expect, test, type Page } from "@playwright/test";

async function setup(page: Page, empty = false) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(
    ({ empty }) => {
      const w = window as any;
      const calls: any[] = [],
        callbacks = new Map<number, Function>();
      let serial = 0,
        output: any,
        revision = 0,
        subscription = "";
      const server = {
        id: "b",
        name: "研发服务器",
        host: "ssh.internal",
        port: 22,
        username: "ops",
        tags: [],
        authType: "password",
        privateKey: "",
        hasSavedPassword: true,
        jumpServerId: null,
        directHost: null,
        directPort: 22,
      };
      const rule = {
        id: "db",
        revision: 1,
        name: "开发数据库",
        webScheme: null,
        settings: {
          serverId: "b",
          listenAddress: "::1",
          listenPort: null,
          targetHost: "db.internal",
          targetPort: 3306,
          autoReconnect: true,
        },
      };
      function ready(rule: any) {
        return {
          runId: "run-" + ++serial,
          phase: "ready",
          actualAddress: "[::1]:43123",
          settings: structuredClone(rule.settings),
          connection: {
            target: server,
            hops: [{ index: 0, role: "target", server }],
            identity: "identity",
          },
          startedAt: Date.now(),
          activeConnections: 2,
          sentBytes: "12582912",
          receivedBytes: "12582912",
          sentPerSecond: 65536,
          receivedPerSecond: 32768,
          attempts: 0,
          retryAt: null,
          error: null,
          prompt: null,
          probeId: null,
          targetResult: null,
          notes: [
            { at: Date.now(), message: "SSH 路径已就绪，可以连接本地端口" },
          ],
        };
      }
      const rules: any[] = empty
        ? []
        : [
            {
              rule,
              runtime: ready(rule),
              pendingChanges: false,
              serverError: null,
            },
          ];
      const frame = (override: any = {}) =>
        output?.onmessage({
          subscriptionId: subscription,
          revision: ++revision,
          rules: structuredClone(rules),
          servers: [server],
          selectedServer: null,
          error: null,
          ...override,
        });
      w.__PF__ = {
        calls,
        rules,
        frame,
        prompt: (kind = "hostKeyRequired") => {
          rules[0].runtime.prompt = {
            attemptId: "attempt-1",
            event: {
              type: kind,
              challenge: {
                id: kind === "hostKeyRequired" ? "key-1" : "auth-1",
                hop: { index: 0, role: "target", server },
                fingerprint: "SHA256:new",
                previous: "SHA256:old",
                message: null,
              },
            },
          };
          rules[0].runtime.phase = "authenticating";
          frame();
        },
      };
      w.isTauri = true;
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "port-forwarding" },
          currentWebview: { label: "port-forwarding" },
        },
        transformCallback: (cb: Function) => {
          callbacks.set(++serial, cb);
          return serial;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
        invoke: async (command: string, args: any = {}) => {
          calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
          if (command === "window_chrome") return "custom";
          if (command === "plugin:window|is_focused") return true;
          if (command.startsWith("plugin:window|is_")) return false;
          if (command === "plugin:event|listen") return ++serial;
          if (command === "port_forward_bootstrap")
            return { generation: 7, servers: [server] };
          if (command === "subscribe_port_forwards") {
            output = args.output;
            subscription = "subscription-" + ++serial;
            revision = 0;
            frame();
            return subscription;
          }
          if (command === "save_port_forward") {
            const saved = {
              ...structuredClone(args.rule),
              id: args.rule.id || "new-rule",
              revision: args.rule.revision + 1,
            };
            const old = rules.find((v) => v.rule.id === saved.id);
            if (old) {
              old.rule = saved;
              old.pendingChanges = true;
            } else
              rules.push({
                rule: saved,
                runtime: null,
                pendingChanges: false,
                serverError: null,
              });
            frame();
            return saved;
          }
          const view = rules.find((v) => v.rule.id === args.id);
          if (command === "start_port_forward") {
            view.runtime = ready(view.rule);
            frame();
            return view.runtime.runId;
          }
          if (command === "stop_port_forward") {
            view.runtime.phase = "stopped";
            frame();
            return;
          }
          if (command === "apply_port_forward") {
            if (args.delete) rules.splice(rules.indexOf(view), 1);
            else {
              view.runtime = ready(view.rule);
              view.pendingChanges = false;
            }
            frame();
            return;
          }
          if (command === "answer_port_forward") {
            if (args.challengeId === "key-1") w.__PF__.prompt("authRequired");
            else {
              view.runtime.prompt = null;
              view.runtime.phase = "ready";
              frame();
            }
          }
          return null;
        },
      };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    },
    { empty },
  );
  await page.goto("/?port-forwarding=1");
  await expect(
    page.getByRole("button", { name: "新建转发", exact: true }),
  ).toBeEnabled();
  return errors;
}

test("ready rule shows actual IPv6 address, bounded frame acknowledgements and snapshot details", async ({
  page,
}) => {
  const errors = await setup(page);
  await expect(page.locator(".forward-endpoint")).toContainText("[::1]:43123");
  await expect(page.locator(".forward-detail")).toContainText(
    "db.internal:3306",
  );
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__PF__.calls.filter(
            (c: any) => c.command === "ack_port_forward_frame",
          ).length,
      ),
    )
    .toBeGreaterThan(0);
  await page.evaluate(() =>
    (window as any).__PF__.frame({ revision: 0, rules: [] }),
  );
  await expect(page.locator(".forward-detail h2")).toHaveText("开发数据库");
  await page.getByRole("button", { name: "测试目标连接", exact: true }).click();
  const call = await page.evaluate(() =>
    (window as any).__PF__.calls.find(
      (c: any) => c.command === "test_port_forward",
    ),
  );
  expect(call.args.generation).toBe(7);
  expect(call.args.runId).toMatch(/^run-/);
  await page.screenshot({ path: ".qa/port-forwarding-ready.png" });
  expect(errors).toEqual([]);
});

test("new rule validates ports and saves before starting without a terminal", async ({
  page,
}) => {
  const errors = await setup(page, true);
  await page.getByRole("button", { name: "新建转发", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "新建端口转发" });
  await editor.getByLabel("名称", { exact: true }).fill("内网网站");
  await editor.getByLabel("目标地址", { exact: true }).fill("web.internal");
  await editor.getByLabel("目标端口", { exact: true }).fill("8080");
  await editor.getByLabel("网页打开方式").selectOption("http");
  await editor.getByRole("button", { name: "保存并启动" }).click();
  await expect(editor).toHaveCount(0);
  await expect(page.locator(".forward-detail h2")).toHaveText("内网网站");
  await page.getByRole("button", { name: "打开网页" }).click();
  const calls = await page.evaluate(() => (window as any).__PF__.calls);
  expect(
    calls.findIndex((c: any) => c.command === "save_port_forward"),
  ).toBeLessThan(
    calls.findIndex((c: any) => c.command === "start_port_forward"),
  );
  expect(calls.some((c: any) => c.command === "open_connection")).toBe(false);
  expect(
    calls.find((c: any) => c.command === "open_port_forward_web").args,
  ).not.toHaveProperty("url");
  expect(errors).toEqual([]);
});

test("editing survives incoming frames and leaves current target until explicit apply", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.getByRole("button", { name: "编辑规则", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "编辑端口转发" });
  await editor.getByLabel("目标地址", { exact: true }).fill("db2.internal");
  await page.evaluate(() => (window as any).__PF__.frame());
  await expect(editor.getByLabel("目标地址", { exact: true })).toHaveValue(
    "db2.internal",
  );
  await editor.getByRole("button", { name: "保存，下次启动生效" }).click();
  await expect(page.locator(".forward-detail")).toContainText("有修改待应用");
  await expect(page.locator(".forward-detail .forward-route")).toContainText(
    "db.internal:3306",
  );
  await page.getByRole("button", { name: "停止并应用", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "确认操作" })).toContainText(
    "2 条业务连接",
  );
  await page
    .getByRole("dialog", { name: "确认操作" })
    .getByRole("button", { name: "确认", exact: true })
    .click();
  await expect(page.locator(".forward-detail .forward-route")).toContainText(
    "db2.internal:3306",
  );
  expect(errors).toEqual([]);
});

test("per-hop authentication changes during an answer retain exact challenge ownership", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => (window as any).__PF__.prompt());
  await expect(
    page.getByRole("dialog", { name: "服务器指纹已变化" }),
  ).toContainText("SHA256:old");
  await page.getByRole("button", { name: "更新指纹并连接" }).click();
  const login = page.getByRole("dialog", { name: "密码认证" });
  await expect(login).toBeVisible();
  await login.getByLabel("密码", { exact: true }).fill("test-only");
  await login.getByRole("button", { name: "连接", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const calls = await page.evaluate(() =>
    (window as any).__PF__.calls.filter(
      (c: any) => c.command === "answer_port_forward",
    ),
  );
  expect(calls.map((c: any) => c.args.challengeId)).toEqual([
    "key-1",
    "auth-1",
  ]);
  expect(
    calls.every(
      (c: any) => c.args.generation === 7 && c.args.attemptId === "attempt-1",
    ),
  ).toBe(true);
  expect(errors).toEqual([]);
});
