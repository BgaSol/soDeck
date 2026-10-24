import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    // Browser-only protocol provider. It creates no SSH or product plugin.
    const servers: any[] = [];
    const views: any[] = [];
    const calls: any[] = [];
    const layout = { views, pendingOpens: [] as any[], activeWorkspaceId: null as string | null, split: "none", sessionsSide: "left", sessionsVisible: true };
    const waiting = new Map<string, (response: any) => void>();
    let trusted = false, authenticated = false;
    (window as any).__UI_TEST__ = { calls, views, layout, servers };
    (window as any).__TAURI_INTERNALS__ = { invoke: async (_command: string, { request }: any) => {
      calls.push(request);
      let result: any = {};
      const params = request.params;
      if (request.method === "servers.list") result = { servers: [...servers] };
      if (request.method === "servers.create") { result = { ...params, id: `server-${servers.length + 1}`, revision: 1, hasSecret: false }; servers.push(result); }
      if (request.method === "workbench.snapshot") result = JSON.parse(JSON.stringify(layout));
      if (request.method === "workbench.activateWorkspace") { layout.activeWorkspaceId = params.serverId; if (!views.some(v => v.viewType === "terminal.sessions" && v.serverId === params.serverId)) views.push({ viewId: `list-${params.serverId}`, viewType: "terminal.sessions", workspaceId: params.serverId, serverId: params.serverId, title: "会话", placement: "left", group: 0, mountGeneration: 1, status: "active" }); }
      if (request.method === "workbench.configure") Object.assign(layout, params);
      if (request.method === "workbench.activate") {
        const selected = views.find(view => view.viewId === params.viewId);
        if (selected) for (const view of views) if (view.workspaceId === selected.workspaceId && view.placement === selected.placement && view.group === selected.group) view.active = view === selected;
      }
      if (request.method === "workbench.openTerminal") {
        if ((window as any).__UI_TEST__.holdOpen) {
          layout.pendingOpens.push({ operationId: params.operationId, serverId: params.serverId });
          (window as any).__sodeckNotify({ type: "workbench.changed" });
          return new Promise(resolve => waiting.set(params.operationId, error => resolve({ requestId: request.requestId, error })));
        }
        if (!trusted) return { requestId: request.requestId, error: { code: "HOST_KEY_REQUIRED", message: "核对主机身份", details: { fingerprint: "SHA256:isolated-test-fixture", algorithm: "ssh-ed25519" } } };
        if (!authenticated) return { requestId: request.requestId, error: { code: "AUTH_REQUIRED", message: "请输入凭据" } };
        const terminals = views.filter(view => view.viewType === "terminal.session").length;
        result = { viewId: `view-${terminals + 1}`, viewType: "terminal.session", workspaceId: params.serverId, serverId: params.serverId, title: `终端 ${terminals + 1}`, placement: "center", group: 0, mountGeneration: 1, status: "active" }; views.push(result);
      }
      if (request.method === "workbench.cancelOpen") {
        if ((window as any).__UI_TEST__.cancelFault) return { requestId: request.requestId, error: { code: "IO_ERROR", message: "取消暂未完成，请重试" } };
        layout.pendingOpens = layout.pendingOpens.filter(open => open.operationId !== params.operationId);
        waiting.get(params.operationId)?.((window as any).__UI_TEST__.lateAuth ? { code: "AUTH_REQUIRED", message: "迟到认证" } : { code: "CANCELED", message: "已取消连接" }); waiting.delete(params.operationId);
        (window as any).__sodeckNotify({ type: "workbench.openCanceled", payload: { operationId: params.operationId } });
        if ((window as any).__UI_TEST__.lateAuth) (window as any).__sodeckNotify({ type: "auth.required", payload: { server: servers[0], operationId: params.operationId, error: { code: "AUTH_REQUIRED", message: "迟到认证" } } });
        (window as any).__sodeckNotify({ type: "workbench.changed" });
        result = { canceled: true };
      }
      if (request.method === "hosts.trust") trusted = true;
      if (request.method === "auth.set") authenticated = true;
      if (request.method === "workbench.move") { const view = views.find(v => v.viewId === params.viewId); if (view) { Object.assign(view, { placement: params.placement, group: params.group ?? 0 }); if (view.group === 1) layout.split = "horizontal"; } }
      if (request.method === "workbench.close") { const index = views.findIndex(v => v.viewId === params.viewId); if (index >= 0) views.splice(index, 1); }
      if (request.method === "plugins.list") result = [{ pluginId: "sodeck.terminal", activeVersion: "0.1.0" }];
      if (request.method === "resources.list") result = { connections: [], ptys: [], leases: [] };
      if (request.method === "calls.list") result = { calls: [] };
      if (request.method === "workbench.recover" && (window as any).__UI_TEST__.recoverFault) return { requestId: request.requestId, error: (window as any).__UI_TEST__.recoverFault };
      return { requestId: request.requestId, result };
    } };
  });
});

async function addServer(page: import("@playwright/test").Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "添加服务器", exact: true }).first().click();
  await page.getByLabel("显示名称").fill("隔离测试服务器");
  await page.getByLabel("主机地址").fill("127.0.0.1");
  await page.getByLabel("用户名").fill("test-user");
  await page.getByRole("button", { name: "保存服务器" }).click();
  await expect(page.getByRole("heading", { name: "隔离测试服务器" })).toBeVisible();
}

test("shows honest empty state and validates server before submitting", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "从一台服务器开始。" })).toBeVisible();
  await page.screenshot({ path: ".qa/ui/welcome.png" });
  await page.getByRole("button", { name: "添加服务器", exact: true }).first().click();
  await expect(page.getByRole("button", { name: "保存服务器" })).toBeDisabled();
  await page.getByLabel("显示名称").fill("名称"); await page.getByLabel("主机地址").fill("127.0.0.1"); await page.getByLabel("用户名").fill("user");
  await page.getByLabel("端口").fill("65536");
  await expect(page.getByRole("button", { name: "保存服务器" })).toBeDisabled();
  await page.getByLabel("端口").fill("22");
  await expect(page.getByRole("button", { name: "保存服务器" })).toBeEnabled();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("trust precedes credentials and switching layout keeps one logical terminal", async ({ page }) => {
  const errors: string[] = []; page.on("pageerror", error => errors.push(error.message));
  await addServer(page);
  await page.getByRole("button", { name: "新建终端", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "确认服务器身份" })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__UI_TEST__.calls.some((c: any) => c.method === "auth.set"))).toBe(false);
  await page.getByRole("button", { name: "信任并连接" }).click();
  await expect(page.getByRole("heading", { name: "连接认证" })).toBeVisible();
  await page.getByLabel("密码", { exact: true }).fill("isolated-browser-fixture");
  await page.getByRole("button", { name: "连接", exact: true }).click();
  await expect(page.getByRole("tab", { name: /终端 1/ })).toBeVisible();
  await page.getByRole("button", { name: "左右分屏" }).click();
  await expect(page.getByRole("tablist")).toHaveCount(2);
  await page.getByRole("button", { name: "终端 1 的视图操作" }).click();
  await page.getByRole("button", { name: "移到另一区域" }).click();
  await expect(page.getByRole("tablist", { name: "终端区域 2" }).getByRole("tab", { name: /终端 1/ })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__UI_TEST__.views.filter((v: any) => v.viewType === "terminal.session").length)).toBe(1);
  await page.getByRole("button", { name: "上下分屏" }).click();
  await expect(page.locator(".editor-groups.vertical")).toBeVisible();
  await page.screenshot({ path: ".qa/ui/workbench-contract.png" });
  await page.getByRole("button", { name: "插件", exact: true }).first().click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((c: any) => c.method === "workbench.modal").at(-1)?.params.open)).toBe(true);
  await page.keyboard.press("Escape");
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((c: any) => c.method === "workbench.modal").at(-1)?.params.open)).toBe(false);
  expect(errors).toEqual([]);
});

test("small window and reduced motion keep primary operations visible", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 640 }); await page.emulateMedia({ reducedMotion: "reduce", contrast: "more" });
  await addServer(page);
  await expect(page.getByRole("button", { name: "左右分屏" })).toBeVisible();
  await expect(page.getByRole("button", { name: "新建终端", exact: true }).first()).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: ".qa/ui/compact-contrast.png" });
});

test("pending connection exposes keyboard cancellation and allows a new attempt", async ({ page }) => {
  await addServer(page);
  await page.evaluate(() => { (window as any).__UI_TEST__.holdOpen = true; (window as any).__UI_TEST__.lateAuth = true; });
  await page.getByRole("button", { name: "新建终端", exact: true }).first().click();
  const cancel = page.getByRole("button", { name: "取消连接到 隔离测试服务器", exact: true });
  await expect(cancel).toBeVisible();
  await expect(page.getByRole("status")).toContainText("正在连接");
  await cancel.focus(); await page.keyboard.press("Enter");
  await expect(cancel).toHaveCount(0);
  await expect(page.locator(".workspace-tools > button.primary")).toBeFocused();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const calls = await page.evaluate(() => (window as any).__UI_TEST__.calls);
  expect(calls.find((call: any) => call.method === "workbench.cancelOpen").params.operationId).toBe(calls.find((call: any) => call.method === "workbench.openTerminal").params.operationId);
  await page.evaluate(() => { (window as any).__UI_TEST__.holdOpen = false; });
  await page.getByRole("button", { name: "新建终端", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "确认服务器身份" })).toBeVisible();
});

test("external pending attempts remain independently cancellable and a failed cancel can retry", async ({ page }) => {
  await addServer(page);
  await page.setViewportSize({ width: 900, height: 640 }); await page.emulateMedia({ reducedMotion: "reduce", contrast: "more" });
  await page.evaluate(() => {
    const test = (window as any).__UI_TEST__;
    test.layout.pendingOpens = [{ operationId: "session-list-open", serverId: "server-1" }, { operationId: "placeholder-open", serverId: "server-1" }];
    test.cancelFault = true;
    (window as any).__sodeckNotify({ type: "workbench.changed" });
  });
  const cancel = page.getByRole("button", { name: "取消连接到 隔离测试服务器", exact: true });
  await expect(cancel).toHaveCount(2);await cancel.first().click();
  await expect(page.getByRole("alert")).toContainText("取消暂未完成，请重试");await expect(cancel.first()).toBeEnabled();
  await page.evaluate(() => { (window as any).__UI_TEST__.cancelFault = false; });
  await cancel.first().click();await expect(cancel).toHaveCount(1);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.layout.pendingOpens[0].operationId)).toBe("placeholder-open");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: ".qa/ui/pending-connection-cancel.png" });
});

test("bottom tabs follow core activation and support keyboard selection", async ({ page }) => {
  await addServer(page);
  await page.evaluate(() => {
    for (let i = 1; i <= 2; i++) (window as any).__UI_TEST__.views.push({ viewId: `bottom-${i}`, viewType: "terminal.session", workspaceId: "server-1", serverId: "server-1", title: `底部 ${i}`, placement: "bottom", group: 0, mountGeneration: 1, status: "active", active: i === 2 });
    (window as any).__sodeckNotify({ type: "workbench.changed" });
  });
  const tabs = page.getByRole("tablist", { name: "底部终端" });
  const first = tabs.getByRole("tab", { name: /底部 1/ });
  const second = tabs.getByRole("tab", { name: /底部 2/ });
  await expect(second).toHaveAttribute("aria-selected", "true");
  await first.click();
  await expect(first).toHaveAttribute("aria-selected", "true");
  await page.evaluate(() => {
    for (const view of (window as any).__UI_TEST__.views) if (view.placement === "bottom") view.active = view.viewId === "bottom-2";
    (window as any).__sodeckNotify({ type: "workbench.changed" });
  });
  await expect(second).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('.bottom-panel .native-surface')).toHaveAttribute("data-view-id", "bottom-2");

  await second.focus();
  await page.keyboard.press("ArrowRight");
  await expect(first).toBeFocused();
  await expect(first).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("End");
  await expect(second).toBeFocused();
  await expect(second).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Home");
  await expect(first).toBeFocused();
  await expect(first).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.activate").at(-1)?.params.viewId)).toBe("bottom-1");
});

test("detached terminal has a drag handle with cancellation and native drop request", async ({ page }) => {
  await page.addInitScript(() => { (window as any).__SODECK_CONTEXT__ = { detachedViewId: "detached-1" }; });
  await page.goto("/");
  await page.evaluate(() => {
    (window as any).__UI_TEST__.views.push({ viewId: "detached-1", viewType: "terminal.session", workspaceId: "server-1", serverId: "server-1", title: "独立测试终端", placement: "detached", group: 0, mountGeneration: 1, status: "active", active: true });
    (window as any).__sodeckNotify({ type: "workbench.changed" });
  });
  const tab = page.getByRole("tab", { name: /独立测试终端/ });
  await expect(tab).toBeVisible();
  const rect = await tab.boundingBox();
  if (!rect) throw Error("No detached drag handle");
  await page.mouse.move(rect.x + 16, rect.y + 16); await page.mouse.down();
  await page.mouse.move(rect.x + 60, rect.y + 55, { steps: 4 });
  await expect(page.locator(".drag-preview")).toBeVisible();
  await page.keyboard.press("Escape"); await page.mouse.up();
  await expect(page.locator(".drag-preview")).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.move").length)).toBe(0);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.activate").length)).toBe(0);

  await page.mouse.move(rect.x + 16, rect.y + 16); await page.mouse.down();
  await page.mouse.move(1310, rect.y + 50, { steps: 8 }); await page.mouse.up();
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.move").at(-1)?.params)).toMatchObject({ viewId: "detached-1", placement: "detached", screenPosition: { x: expect.any(Number), y: expect.any(Number) } });
});

test("drag release consumes its synthetic click without blocking the next normal selection", async ({ page }) => {
  await page.addInitScript(() => { (window as any).__SODECK_CONTEXT__ = { detachedViewId: "drag-view" }; });
  await page.goto("/");
  await page.evaluate(() => {
    (window as any).__UI_TEST__.tabClicks = 0;
    document.addEventListener("click", event => { if ((event.target as HTMLElement).closest(".view-tab")) (window as any).__UI_TEST__.tabClicks++; }, true);
    (window as any).__UI_TEST__.views.push({ viewId: "drag-view", viewType: "terminal.session", workspaceId: "server-1", serverId: "server-1", title: "拖动回归终端", placement: "detached", group: 0, mountGeneration: 1, status: "active", active: true });
    (window as any).__sodeckNotify({ type: "workbench.changed" });
  });
  const tab = page.getByRole("tab", { name: /拖动回归终端/ });
  await expect(tab).toBeVisible();
  const rect = await tab.boundingBox();
  if (!rect) throw Error("No drag source");
  await page.mouse.move(rect.x + 16, rect.y + 16); await page.mouse.down();
  await page.mouse.move(1310, rect.y + 50, { steps: 8 }); await page.mouse.up();
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.move").length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.tabClicks)).toBe(1);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.activate").length)).toBe(0);

  await tab.click();
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.activate").length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.tabClicks)).toBe(2);
});

test("a missing-plugin placeholder does not claim a live connection and keeps the recovery error", async ({ page }) => {
  await addServer(page);
  await page.evaluate(() => {
    (window as any).__UI_TEST__.recoverFault = { code: "PLUGIN_UNAVAILABLE", message: "终端插件包不可用，请检查安装状态" };
    (window as any).__UI_TEST__.views.push({ viewId: "missing-view", viewType: "terminal.session", workspaceId: "server-1", serverId: "server-1", title: "缺失插件占位", placement: "center", group: 0, mountGeneration: 1, status: "viewFailed", active: true });
    (window as any).__sodeckNotify({ type: "workbench.changed" });
  });
  const placeholder = page.locator('[data-view-id="missing-view"] .surface-error');
  await expect(placeholder).toContainText("此位置没有恢复运行会话");
  await expect(placeholder).not.toContainText("连接仍由核心管理");
  await placeholder.getByRole("button", { name: "重新加载视图" }).click();
  await expect(placeholder).toContainText("PLUGIN_UNAVAILABLE：终端插件包不可用，请检查安装状态");
  const before = await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.surface").length);
  await page.setViewportSize({ width: 1100, height: 720 });
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.surface").length)).toBeGreaterThan(before);
  await expect(placeholder).toContainText("PLUGIN_UNAVAILABLE：终端插件包不可用，请检查安装状态");
  await expect(placeholder.getByRole("button", { name: "重新加载视图" })).toBeEnabled();
});
