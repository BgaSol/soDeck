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
      const servers = [server];
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
          connection: structuredClone({
            target: server,
            hops: [{ index: 0, role: "target", server }],
            identity: "identity",
          }),
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
          servers,
          openRequest: null,
          error: null,
          ...override,
        });
      w.__PF__ = {
        calls,
        rules,
        servers,
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
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: {
          writeText: async (value: string) => {
            w.__PF__.copied = value;
          },
        },
      });
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
            return { generation: 7, servers };
          if (command === "subscribe_port_forwards") {
            output = args.output;
            subscription = "subscription-" + ++serial;
            revision = 0;
            frame();
            return subscription;
          }
          if (command === "save_port_forward") {
            if (w.__PF__.failSave)
              throw { kind: "io", message: "配置无法写入" };
            const saved = {
              ...structuredClone(args.rule),
              id: args.rule.id || "new-rule",
              revision: args.rule.revision + 1,
            };
            const old = rules.find((v) => v.rule.id === saved.id);
            if (old) {
              old.rule = saved;
              old.pendingChanges =
                !!old.runtime &&
                !["stopped", "failed"].includes(old.runtime.phase) &&
                JSON.stringify(old.runtime.settings) !==
                  JSON.stringify(saved.settings);
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
            if (w.__PF__.failStart)
              throw { kind: "conflict", message: "启动请求已失效" };
            view.runtime = ready(view.rule);
            frame();
            return view.runtime.runId;
          }
          if (command === "stop_port_forward") {
            view.runtime.phase = "stopped";
            view.runtime.actualAddress = null;
            view.runtime.activeConnections = 0;
            view.pendingChanges = false;
            frame();
            return;
          }
          if (command === "apply_port_forward") {
            if (w.__PF__.failApply)
              throw { kind: "conflict", message: "应用请求已失效" };
            if (args.delete) rules.splice(rules.indexOf(view), 1);
            else {
              view.runtime = ready(view.rule);
              view.pendingChanges = false;
            }
            frame();
            return;
          }
          if (command === "test_port_forward") {
            view.runtime.probeId = args.cancelProbeId ? null : "probe-1";
            frame();
            return args.cancelProbeId || "probe-1";
          }
          if (command === "delete_port_forward") {
            rules.splice(rules.indexOf(view), 1);
            frame();
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
  await page.locator(".forward-diagnostics > summary").click();
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
  await editor.locator(".forward-advanced > summary").click();
  await editor.getByLabel("网页打开方式").selectOption("http");
  await editor.getByRole("button", { name: "创建并启动" }).click();
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
  await page.getByRole("button", { name: "更多操作", exact: true }).click();
  await page.getByRole("menuitem", { name: "编辑规则", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "编辑端口转发" });
  await editor.getByLabel("目标地址", { exact: true }).fill("db2.internal");
  await page.evaluate(() => (window as any).__PF__.frame());
  await expect(editor.getByLabel("目标地址", { exact: true })).toHaveValue(
    "db2.internal",
  );
  await editor.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.locator(".forward-detail")).toContainText(
    "配置已保存，下次启动时生效",
  );
  await expect(page.locator(".forward-detail .forward-route")).toContainText(
    "db.internal:3306",
  );
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    pf.servers[0].host = "ssh-updated.internal";
    pf.frame();
  });
  await page.locator(".forward-pending summary").click();
  const comparison = page.locator(".forward-comparison dd");
  await expect(comparison.nth(0)).toContainText("[::1]:43123");
  await expect(comparison.nth(0)).toContainText("ssh.internal:22");
  await expect(comparison.nth(1)).toContainText("ssh-updated.internal:22");
  await page.getByRole("button", { name: "重新启动", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "重新启动" })).toContainText(
    "2 条业务连接",
  );
  await page
    .getByRole("dialog", { name: "重新启动" })
    .getByRole("button", { name: "重新启动", exact: true })
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

async function edit(page: Page) {
  await page.getByRole("button", { name: "更多操作", exact: true }).click();
  await page.getByRole("menuitem", { name: "编辑规则", exact: true }).click();
  return page.getByRole("dialog", { name: "编辑端口转发" });
}

test("fixed and automatic ports preserve draft intent and focus inline validation", async ({
  page,
}) => {
  const errors = await setup(page, true);
  await page.getByRole("button", { name: "新建转发", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "新建端口转发" });
  await expect(dialog.getByLabel("目标地址", { exact: true })).toHaveValue(
    "127.0.0.1",
  );
  await expect(dialog.getByLabel("目标端口", { exact: true })).toBeEmpty();
  await dialog.getByRole("button", { name: "创建并启动" }).click();
  await expect(dialog.getByLabel("名称", { exact: true })).toBeFocused();
  await expect(dialog.getByText("请输入名称", { exact: true })).toBeVisible();
  await dialog.getByLabel("名称", { exact: true }).fill("本地服务");
  await dialog.getByLabel("目标端口", { exact: true }).fill("5432");
  await expect(dialog.getByLabel("本地端口", { exact: true })).toHaveValue(
    "5432",
  );
  await dialog.getByLabel("本地端口", { exact: true }).fill("15432");
  await dialog.getByLabel("自动分配", { exact: true }).check();
  await dialog.getByLabel("目标端口", { exact: true }).fill("6432");
  await dialog.getByLabel("固定端口", { exact: true }).check();
  await expect(dialog.getByLabel("本地端口", { exact: true })).toHaveValue(
    "15432",
  );
  await dialog.getByLabel("自动分配", { exact: true }).check();
  await dialog.getByLabel("创建后启动").uncheck();
  await dialog.getByRole("button", { name: "创建", exact: true }).click();
  await expect(page.locator(".forward-endpoint")).toContainText(
    "启动后自动分配",
  );
  await expect(
    page.getByRole("button", { name: "复制地址", exact: true }),
  ).toBeDisabled();
  const calls = await page.evaluate(() => (window as any).__PF__.calls);
  expect(
    calls.find((c: any) => c.command === "save_port_forward").args.rule.settings
      .listenPort,
  ).toBeNull();
  expect(calls.some((c: any) => c.command === "start_port_forward")).toBe(
    false,
  );
  expect(errors).toEqual([]);
});

test("save and restart confirms before persistence, uses saved revision and exposes partial failure", async ({
  page,
}) => {
  const errors = await setup(page);
  const editor = await edit(page);
  await editor.getByLabel("目标地址", { exact: true }).fill("next.internal");
  await editor.getByLabel("应用更改", { exact: true }).selectOption("restart");
  await editor.getByRole("button", { name: "保存并重启", exact: true }).click();
  const confirm = page.getByRole("dialog", { name: "保存并重启", exact: true });
  await expect(confirm).toContainText("2 条业务连接");
  expect(
    await page.evaluate(() =>
      (window as any).__PF__.calls.some(
        (c: any) => c.command === "save_port_forward",
      ),
    ),
  ).toBe(false);
  await confirm.getByRole("button", { name: "取消", exact: true }).click();
  await expect(editor.getByLabel("目标地址", { exact: true })).toHaveValue(
    "next.internal",
  );
  await page.evaluate(() => {
    (window as any).__PF__.failApply = true;
  });
  await editor.getByRole("button", { name: "保存并重启", exact: true }).click();
  await confirm
    .getByRole("button", { name: "保存并重启", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
  await expect(page.locator(".forward-save-error")).toContainText(
    "配置已保存，但重新启动未完成",
  );
  await expect(page.locator(".forward-detail .forward-route")).toContainText(
    "db.internal:3306",
  );
  const calls = await page.evaluate(() => (window as any).__PF__.calls);
  const applied = calls.find(
    (c: any) => c.command === "apply_port_forward",
  ).args;
  expect(applied).toMatchObject({
    revision: 2,
    generation: 7,
    id: "db",
    delete: false,
  });
  expect(applied.runId).toMatch(/^run-/);
  await page.evaluate(() => {
    (window as any).__PF__.failApply = false;
  });
  await page.getByRole("button", { name: "重试启动", exact: true }).click();
  await page
    .getByRole("dialog", { name: "重新启动", exact: true })
    .getByRole("button", { name: "重新启动", exact: true })
    .click();
  await expect(page.locator(".forward-save-error")).toHaveCount(0);
  await expect(page.locator(".forward-detail .forward-route")).toContainText(
    "next.internal:3306",
  );
  expect(errors).toEqual([]);
});

test("metadata saves immediately without restart and failed persistence retains the draft", async ({
  page,
}) => {
  const errors = await setup(page);
  const editor = await edit(page);
  await editor.getByLabel("名称", { exact: true }).fill("新名称");
  await expect(editor.getByLabel("应用更改", { exact: true })).toHaveCount(0);
  await page.evaluate(() => {
    (window as any).__PF__.failSave = true;
  });
  await editor.getByRole("button", { name: "保存", exact: true }).click();
  await expect(editor.getByRole("alert")).toHaveText("配置无法写入");
  await expect(editor.getByLabel("名称", { exact: true })).toHaveValue(
    "新名称",
  );
  await page.evaluate(() => {
    (window as any).__PF__.failSave = false;
  });
  await editor.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.locator(".forward-detail h2")).toHaveText("新名称");
  await expect(page.locator(".forward-pending")).toHaveCount(0);
  expect(
    await page.evaluate(() =>
      (window as any).__PF__.calls.some((c: any) =>
        ["start_port_forward", "apply_port_forward"].includes(c.command),
      ),
    ),
  ).toBe(false);
  expect(errors).toEqual([]);
});

test("server requests filter existing rules, deduplicate frames and wait for a dirty editor", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    const second = {
      ...pf.servers[0],
      id: "c",
      name: "另一台服务器",
      host: "other.internal",
    };
    pf.servers.push(second);
    pf.rules.push({
      ...structuredClone(pf.rules[0]),
      rule: {
        ...structuredClone(pf.rules[0].rule),
        id: "other",
        name: "内部网站",
        settings: { ...pf.rules[0].rule.settings, serverId: "c" },
      },
      runtime: null,
    });
    pf.frame({ openRequest: { id: "entry-1", serverId: "c" } });
  });
  await expect(page.locator(".forward-row")).toHaveCount(1);
  await expect(page.locator(".forward-detail h2")).toHaveText("内部网站");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.getByRole("button", { name: "清除服务器筛选" }).click();
  await page.evaluate(() =>
    (window as any).__PF__.frame({
      openRequest: { id: "entry-1", serverId: "c" },
    }),
  );
  await expect(page.locator(".forward-row")).toHaveCount(2);
  await page.evaluate(() =>
    (window as any).__PF__.frame({
      openRequest: { id: "entry-2", serverId: "b" },
    }),
  );
  const editor = await edit(page);
  await editor.getByLabel("名称", { exact: true }).fill("未保存草稿");
  await page.evaluate(() =>
    (window as any).__PF__.frame({
      openRequest: { id: "global-3", serverId: null },
    }),
  );
  await expect(editor.getByLabel("名称", { exact: true })).toHaveValue(
    "未保存草稿",
  );
  await editor.getByRole("button", { name: "取消", exact: true }).click();
  await page
    .getByRole("dialog", { name: "放弃修改？" })
    .getByRole("button", { name: "放弃修改", exact: true })
    .click();
  await expect(page.locator(".forward-scope")).toHaveCount(0);
  await expect(page.locator(".forward-row")).toHaveCount(2);
  expect(errors).toEqual([]);
});

test("search never displays hidden selection and respects running configuration", async ({
  page,
}) => {
  await setup(page);
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    pf.rules[0].rule.settings.targetHost = "next.internal";
    pf.rules[0].pendingChanges = true;
    pf.frame();
  });
  const search = page.getByRole("textbox", { name: "搜索转发规则" });
  await search.fill("next.internal");
  await expect(page.locator(".forward-row")).toHaveCount(0);
  await expect(page.locator(".forward-detail h2")).toHaveText("没有匹配的转发");
  await search.fill("43123");
  await expect(page.locator(".forward-row")).toHaveCount(1);
  await expect(page.locator(".forward-detail .forward-route")).toContainText(
    "db.internal",
  );
  await search.fill("");
  await page.getByRole("button", { name: "停止", exact: true }).click();
  await page
    .getByRole("dialog", { name: "停止转发", exact: true })
    .getByRole("button", { name: "停止转发", exact: true })
    .click();
  await expect(page.locator(".forward-detail .forward-route")).toContainText(
    "next.internal",
  );
  await expect(page.locator(".forward-endpoint")).toContainText(
    "启动后自动分配",
  );
  await expect(
    page.getByRole("button", { name: "复制地址", exact: true }),
  ).toBeDisabled();
});

test("copy feedback is local and menus support keyboard focus and viewport bounds", async ({
  page,
}) => {
  const errors = await setup(page);
  const endpoint = page.locator(".forward-endpoint");
  const before = await endpoint.boundingBox();
  await page.getByRole("button", { name: "复制地址", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "已复制", exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => (window as any).__PF__.copied)).toBe(
    "[::1]:43123",
  );
  expect(await endpoint.boundingBox()).toEqual(before);
  const trigger = page.getByRole("button", {
    name: "更多复制选项",
    exact: true,
  });
  await trigger.focus();
  await page.keyboard.press("ArrowDown");
  await expect(
    page.getByRole("menuitem", { name: "复制主机", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("End");
  await expect(
    page.getByRole("menuitem", { name: "复制端口", exact: true }),
  ).toBeFocused();
  const menu = await page.getByRole("menu").boundingBox();
  expect(menu!.x + menu!.width).toBeLessThanOrEqual(1100);
  await page.keyboard.press("Enter");
  expect(await page.evaluate(() => (window as any).__PF__.copied)).toBe(
    "43123",
  );
  await expect(trigger).toBeFocused();
  await page.getByRole("button", { name: "更多操作", exact: true }).click();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "更多操作", exact: true }),
  ).toBeFocused();
  await trigger.click();
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    pf.rules[0].runtime.phase = "reconnecting";
    pf.frame();
  });
  await expect(page.getByRole("menu")).toHaveCount(0);
  await expect(trigger).toBeDisabled();
  expect(errors).toEqual([]);
});

test("target failures are visible with diagnostics collapsed without marking the tunnel failed", async ({
  page,
}) => {
  await setup(page);
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    pf.rules[0].runtime.targetResult = {
      at: Date.now(),
      success: false,
      source: "probe",
      message: "目标拒绝连接",
    };
    pf.frame();
  });
  await expect(page.locator(".forward-error")).toContainText("目标连接失败");
  await expect(page.locator(".forward-detail-head .forward-status")).toHaveText(
    "已启动",
  );
  await expect(page.locator(".forward-diagnostics")).not.toHaveAttribute(
    "open",
  );
  await page.getByRole("button", { name: "重新测试", exact: true }).click();
  await page.locator(".forward-diagnostics > summary").click();
  await page.getByRole("button", { name: "取消测试", exact: true }).click();
  const cancelled = await page.evaluate(() =>
    (window as any).__PF__.calls
      .filter((c: any) => c.command === "test_port_forward")
      .at(-1),
  );
  expect(cancelled.args.cancelProbeId).toBe("probe-1");
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    pf.rules[0].runtime.targetResult = {
      at: Date.now(),
      success: true,
      source: "connection",
      message: "业务连接成功",
    };
    pf.frame();
  });
  await expect(page.locator(".forward-error")).toHaveCount(0);
  await expect(page.locator(".forward-diagnostics")).toContainText(
    "业务连接结果",
  );
});

test("actual UI fits minimum size with long IPv6 paths, dark theme and a fixed editor footer", async ({
  page,
}, testInfo) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    const jump = {
      ...pf.servers[0],
      id: "jump",
      name: "堡垒机",
      host: "jump.internal",
    };
    pf.servers.push(jump);
    pf.rules[0].runtime.connection.hops.unshift({
      index: 0,
      role: "jump",
      server: jump,
    });
    pf.rules[0].runtime.startedAt = Date.now() - 24 * 60000;
  });
  await page.evaluate(() => (window as any).__PF__.frame());
  await page.screenshot({
    path: `.qa/forwarding-actual-${testInfo.project.name}-light.png`,
  });
  await page.getByRole("button", { name: "黑夜主题" }).click();
  await expect(page.locator('.forward-row[aria-pressed="true"]')).toHaveCSS(
    "background-color",
    "rgb(41, 63, 96)",
  );
  await page.screenshot({
    path: `.qa/forwarding-actual-${testInfo.project.name}-dark.png`,
  });
  await page.setViewportSize({ width: 800, height: 560 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.evaluate(() => {
    const pf = (window as any).__PF__;
    pf.rules[0].rule.name =
      "用于很长名称验证的远程服务转发配置用于很长名称验证的远程服务转发配置";
    pf.rules[0].runtime.actualAddress = "[::1]:49152";
    pf.rules[0].runtime.settings.targetHost =
      "2001:db8:abcd:abcd:abcd:abcd:abcd:1234";
    pf.frame();
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(800);
  expect(
    await page
      .locator(".forward-detail")
      .evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await page.screenshot({
    path: `.qa/forwarding-actual-${testInfo.project.name}-minimum.png`,
  });
  await page.getByRole("button", { name: "新建转发", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "新建端口转发" });
  await editor.getByLabel("名称", { exact: true }).fill("内网网站");
  await editor.getByLabel("目标端口", { exact: true }).fill("8080");
  await editor.locator(".forward-advanced > summary").click();
  await editor.getByLabel("网页打开方式").selectOption("https");
  const footer = editor.locator(".forward-editor-footer");
  const box = await footer.boundingBox();
  expect(box!.y + box!.height).toBeLessThanOrEqual(560);
  await expect(
    editor.getByRole("button", { name: "创建并启动" }),
  ).toBeInViewport();
  expect(await editor.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(
    true,
  );
  await page.screenshot({
    path: `.qa/forwarding-actual-${testInfo.project.name}-editor-minimum.png`,
  });
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.screenshot({
    path: `.qa/forwarding-actual-${testInfo.project.name}-editor-dark.png`,
  });
  await editor.getByRole("button", { name: "取消", exact: true }).click();
  await page
    .getByRole("dialog", { name: "放弃修改？" })
    .getByRole("button", { name: "放弃修改", exact: true })
    .click();
  await page.getByRole("button", { name: "白天主题" }).click();
  await page.getByRole("button", { name: "新建转发", exact: true }).click();
  await page.screenshot({
    path: `.qa/forwarding-actual-${testInfo.project.name}-editor-light.png`,
  });
  expect(errors).toEqual([]);
});
