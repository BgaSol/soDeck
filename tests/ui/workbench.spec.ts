import { presentationFixture, chooseMenu } from "./presentation-fixture";
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await presentationFixture(page);
  await page.addInitScript(() => {
    // Browser-only protocol provider. It creates no SSH or product plugin.
    const servers: any[] = [];
    const views: any[] = [];
    const calls: any[] = [];
    const layout = { windowId: "main", windowRole: "main", workspaceIds: [] as string[], workspaces: [] as any[], commands: [{id:"terminal.new",title:"新建终端",scope:"server",primary:true}], viewDeclarations:[{id:"terminal.session",locations:["center","bottom","detached"]}], dockSlots: [] as any[], revision:0, splitRatio:.5, recoveryWarnings:[], views, pendingOpens: [] as any[], activeWorkspaceId: null as string | null, split: "none", sessionsSide: "left", sessionsVisible: true };
    const waiting = new Map<string, (response: any) => void>();
    let trusted = false, authenticated = false;
    const viewResult = (v: any) => ({pluginId:'sodeck.terminal',actions:[],revision:1,stateVersion:1,...v,status:v.status==='active'?'running':v.status});
    const snapshot = () => {
      const {sessionsSide: _side,sessionsVisible: _visible,...state}=layout;
      return JSON.parse(JSON.stringify({...state,schemaVersion:3,views:views.map(viewResult),pendingOpens:layout.pendingOpens.map(v=>({commandId:'terminal.new',cancelling:false,...v}))}));
    };
    const changed = () => (window as any).__sodeckNotify({type:'workbench.changed',payload:{revision:++layout.revision,windowIds:['main'],workspaceIds:layout.workspaceIds,viewIds:views.map(v=>v.viewId),changeKind:'snapshot'}});
    (window as any).__UI_TEST__ = { calls, views, layout, servers, changed };
    (window as any).__TAURI_INTERNALS__ = { invoke: async (_command: string, { request }: any) => {
      calls.push(request);
      const presentation=(window as any).__PRESENTATION__.handle(request);if(presentation)return presentation;let result: any = null;
      let params = request.params; if(request.method === "commands.retry") {params = (window as any).__UI_TEST__.retry[params.operationId]; request.method = "commands.execute";} if(request.method === "commands.execute") {params = {...params,serverId:params.workspaceId}; ((window as any).__UI_TEST__.retry ??= {})[params.operationId]=params;}
      if (request.method === "servers.list") result = { servers: [...servers] };
      if (request.method === "servers.create") { result = { ...params, id: `server-${servers.length + 1}`, revision: 1, hasSecret: false }; servers.push(result); }
      if (request.method === "servers.update") { const server=servers.find(s=>s.id===params.serverId);result={...server,...params,revision:server.revision+1};Object.assign(server,result); }
      if (request.method === "workbench.snapshot") result = snapshot();
      if (request.method === "workspaces.open") { layout.activeWorkspaceId = params.serverId; if(!layout.workspaceIds.includes(params.serverId)){layout.workspaceIds.push(params.serverId);layout.workspaces.push({workspaceId:params.serverId,serverId:params.serverId,ownerWindowId:"main"});} }
      if (request.method === "workbench.configure") Object.assign(layout, params);
      if (request.method === "views.activate") {
        const selected = views.find(view => view.viewId === params.viewId);
        if (selected) for (const view of views) if (view.workspaceId === selected.workspaceId && view.placement === selected.placement && view.group === selected.group) view.active = view === selected;
      }
      if (request.method === "commands.execute") {
        if ((window as any).__UI_TEST__.holdOpen) {
          layout.pendingOpens.push({ operationId: params.operationId, serverId: params.serverId });
          (window as any).__UI_TEST__.changed();
          return new Promise(resolve => waiting.set(params.operationId, error => resolve({ requestId: request.requestId, error })));
        }
        if (!trusted) { const error={ code: "HOST_KEY_REQUIRED", message: "核对主机身份", details: { fingerprint: "SHA256:isolated-test-fixture", algorithm: "ssh-ed25519" } }; queueMicrotask(()=>(window as any).__sodeckNotify({type:"auth.required",payload:{...params,server:servers[0],error}})); return {requestId:request.requestId,error}; }
        if (!authenticated) { const error={ code: "AUTH_REQUIRED", message: "请输入凭据" };queueMicrotask(()=>(window as any).__sodeckNotify({type:"auth.required",payload:{...params,server:servers[0],error}}));return {requestId:request.requestId,error}; }
        const terminals = views.filter(view => view.viewType === "terminal.session").length;
        result = { viewId: `view-${terminals + 1}`, viewType: "terminal.session", workspaceId: params.serverId, serverId: params.serverId, title: `终端 ${terminals + 1}`, placement: "center", group: 0, mountGeneration: 1, status: "active" }; views.push(result);
      }
      if (request.method === "workbench.operation.cancel") {
        if ((window as any).__UI_TEST__.cancelFault) return { requestId: request.requestId, error: { code: "IO_ERROR", message: "取消暂未完成，请重试" } };
        layout.pendingOpens = layout.pendingOpens.filter(open => open.operationId !== params.operationId);
        waiting.get(params.operationId)?.((window as any).__UI_TEST__.lateAuth ? { code: "AUTH_REQUIRED", message: "迟到认证" } : { code: "CANCELED", message: "已取消连接" }); waiting.delete(params.operationId);
        (window as any).__sodeckNotify({ type: "workbench.openCanceled", payload: { operationId: params.operationId } });
        if ((window as any).__UI_TEST__.lateAuth) (window as any).__sodeckNotify({ type: "auth.required", payload: { server: servers[0], operationId: params.operationId, error: { code: "AUTH_REQUIRED", message: "迟到认证" } } });
        (window as any).__UI_TEST__.changed();
        result = { canceled: true };
      }
      if (request.method === "hosts.trust") trusted = true;
      if (request.method === "auth.set") {
        if((window as any).__UI_TEST__.credentialFault)return {requestId:request.requestId,error:{code:'SECRET_STORE_ERROR',message:'测试凭据存储不可用'}};
        if((window as any).__UI_TEST__.holdCredential)await new Promise<void>(resolve=>(window as any).__UI_TEST__.releaseCredential=resolve);
        authenticated = true;result={stored:true,remembered:Boolean(params.remember)};const server=servers.find(s=>s.id===params.serverId);if(server)server.hasSecret=Boolean(params.remember);
      }
      if (request.method === "views.move") { const view = views.find(v => v.viewId === params.viewId); if (view) { Object.assign(view, { placement: params.placement, group: params.group ?? 0 }); if (view.group === 1) layout.split = "horizontal"; } }
      if (request.method === "views.close") { const index = views.findIndex(v => v.viewId === params.viewId); if (index >= 0) views.splice(index, 1); }
      if (request.method === "plugins.list") result = [{ pluginId: "sodeck.terminal", activeVersion: "0.1.0" }];
      if (request.method === "resources.list") result = { connections: [], ptys: [], leases: [] };
      if (request.method === "calls.list") result = { calls: [] };
      if (request.method === "views.recover" && (window as any).__UI_TEST__.recoverFault) return { requestId: request.requestId, error: (window as any).__UI_TEST__.recoverFault };
      if(request.method==='workspaces.open')result={workspaceId:params.serverId,ownerWindowId:'main',created:true,committed:true,persisted:true};
      if(request.method==='workbench.configure')result=snapshot();
      if(request.method==='views.activate'||request.method==='views.move')result=viewResult(views.find(v=>v.viewId===params.viewId));
      return { requestId: request.requestId, result };
    } };
  });
});

async function addServer(page: import("@playwright/test").Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "打开服务器", exact: true }).first().click();
  await page.getByRole("button", { name: "添加服务器", exact: true }).click();
  await page.getByLabel("显示名称").fill("隔离测试服务器");
  await page.getByLabel("主机地址").fill("127.0.0.1");
  await page.getByLabel("用户名").fill("test-user");
  await page.getByRole("button", { name: "保存服务器" }).click();
  await expect(page.getByRole("tab", { name: /隔离测试服务器/ })).toBeVisible();
}

async function newServerDraft(page: import("@playwright/test").Page) {
  await page.goto('/');
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
  await page.getByRole('button',{name:'添加服务器',exact:true}).click();
  await page.getByLabel('显示名称').fill('预设密码测试');
  await page.getByLabel('主机地址').fill('127.0.0.1');
  await page.getByLabel('用户名').fill('isolated-user');
}

test('preset password defaults to remembered, saves once and still requires host trust',async({page})=>{
  await newServerDraft(page);
  const password=page.getByLabel(/^登录密码/);
  await expect(page.getByLabel('在此设备上记住密码')).toBeChecked();
  await password.fill('isolated-browser-password');
  await page.getByRole('button',{name:'显示密码',exact:true}).click();await expect(password).toHaveAttribute('type','text');
  await page.getByRole('button',{name:'隐藏密码',exact:true}).click();await expect(password).toHaveAttribute('type','password');
  await page.screenshot({path:'.qa/t007/server-editor.png'});
  await page.evaluate(()=>(window as any).__UI_TEST__.holdCredential=true);
  await page.getByRole('button',{name:'保存服务器'}).click();
  await expect(page.getByRole('button',{name:'正在保存',exact:true})).toBeDisabled();
  await page.keyboard.press('Escape');await page.getByRole('button',{name:'关闭对话框'}).click();await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog')).toHaveCount(1);
  await page.evaluate(()=>(window as any).__UI_TEST__.releaseCredential());
  await expect(page.getByRole('dialog')).toHaveCount(0);
  const calls=await page.evaluate(()=>(window as any).__UI_TEST__.calls);
  expect(calls.filter((c:any)=>c.method==='servers.create')).toHaveLength(1);
  expect(calls.find((c:any)=>c.method==='servers.create').params).not.toHaveProperty('password');
  expect(calls.filter((c:any)=>c.method==='auth.set')).toHaveLength(1);
  expect(calls.find((c:any)=>c.method==='auth.set').params).toMatchObject({serverId:'server-1',password:'isolated-browser-password',remember:true});
  expect(calls.some((c:any)=>['commands.execute','connections.acquire','pty.open'].includes(c.method))).toBe(false);
  await page.getByRole('button',{name:'新建终端',exact:true}).first().click();
  await expect(page.getByRole('heading',{name:'确认服务器身份'})).toBeVisible();
  await page.getByRole('button',{name:'信任并连接',exact:true}).click();
  await expect(page.getByRole('tab',{name:/终端 1/})).toBeVisible();
  await expect(page.getByRole('heading',{name:'连接认证'})).toHaveCount(0);
});

test('credential failure retains draft and retries the same saved server, with blank edits preserving credentials',async({page})=>{
  await newServerDraft(page);await page.evaluate(()=>(window as any).__UI_TEST__.credentialFault=true);
  const password=page.getByLabel(/^登录密码/);await password.fill('retry-only-fixture');
  await page.getByRole('button',{name:'保存服务器'}).click();
  await expect(page.getByRole('alert')).toContainText('服务器已保存，密码未保存');await expect(password).toHaveValue('retry-only-fixture');
  await page.getByLabel('显示名称').fill('重试后的服务器');await page.evaluate(()=>(window as any).__UI_TEST__.credentialFault=false);
  await page.getByRole('button',{name:'保存服务器'}).click();await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await page.getByRole('button',{name:'编辑 重试后的服务器',exact:true}).click();
  await expect(page.getByLabel(/^登录密码/)).toHaveValue('');await expect(page.getByLabel(/^登录密码/)).toHaveAttribute('placeholder','已保存密码，留空保持');
  await page.getByLabel('显示名称').fill('保留密码');await page.getByRole('button',{name:'保存服务器'}).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  const state=await page.evaluate(()=>({servers:(window as any).__UI_TEST__.servers,calls:(window as any).__UI_TEST__.calls}));
  expect(state.servers).toHaveLength(1);expect(state.servers[0]).toMatchObject({id:'server-1',name:'保留密码',hasSecret:true});
  expect(state.calls.filter((c:any)=>c.method==='servers.create')).toHaveLength(1);expect(state.calls.filter((c:any)=>c.method==='auth.set')).toHaveLength(2);
});

test('preset password can be transient and switching authentication discards the password draft',async({page})=>{
  await newServerDraft(page);await page.getByLabel(/^登录密码/).fill('transient-fixture');
  await page.getByLabel('在此设备上记住密码').uncheck();
  await page.getByRole('button',{name:'保存服务器'}).click();await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(await page.evaluate(()=>(window as any).__UI_TEST__.calls.find((c:any)=>c.method==='auth.set').params.remember)).toBe(false);
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await page.getByRole('button',{name:'添加服务器',exact:true}).click();
  await expect(page.getByLabel(/^登录密码/)).toHaveValue('');await page.getByLabel(/^登录密码/).fill('discard-fixture');
  await page.getByLabel('认证方式').selectOption('key');await expect(page.getByLabel(/^登录密码/)).toHaveCount(0);
  await page.getByLabel('认证方式').selectOption('password');await expect(page.getByLabel(/^登录密码/)).toHaveValue('');
});

test("shows honest empty state and validates server before submitting", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "打开服务器，开始工作。" })).toBeVisible();
  await page.screenshot({ path: ".qa/ui/welcome.png" });
  await page.getByRole("button", { name: "打开服务器", exact: true }).first().click();
  await page.getByRole("button", { name: "添加服务器", exact: true }).click();
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
  await expect(page.getByRole("tablist")).toHaveCount(2);
  await expect(page.getByRole("button", { name: "左右分屏" })).toHaveCount(0);
  await expect(page.locator(".sessions-panel")).toHaveCount(0);
  const terminal=page.getByRole("tab",{name:"终端 1",exact:true});await terminal.click({button:'right'});await chooseMenu(page,'move');
  await expect.poll(()=>page.evaluate(()=>(window as any).__UI_TEST__.views[0]?.placement)).toBe('detached');
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
  await expect(page.getByRole("button", { name: "左右分屏" })).toHaveCount(0);
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
  await expect(page.getByRole("status")).toContainText("正在处理");
  await cancel.focus(); await page.keyboard.press("Enter");
  await expect(cancel).toHaveCount(0);
  await expect(page.locator(".tab-tools > button")).toBeFocused();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const calls = await page.evaluate(() => (window as any).__UI_TEST__.calls);
  expect(calls.find((call: any) => call.method === "workbench.operation.cancel").params.operationId).toBe(calls.find((call: any) => call.method === "commands.execute").params.operationId);
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
    (window as any).__UI_TEST__.changed();
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

test("generic non-terminal bottom tabs follow core activation and keyboard selection", async ({ page }) => {
  await addServer(page);
  await page.evaluate(() => {
    for (let i = 1; i <= 2; i++) (window as any).__UI_TEST__.views.push({ viewId: `bottom-${i}`, viewType: "sample.panel", workspaceId: "server-1", serverId: "server-1", title: `底部 ${i}`, placement: "bottom", group: 0, mountGeneration: 1, status: "active", active: i === 2 });
    (window as any).__UI_TEST__.changed();
  });
  const tabs = page.getByRole("tablist", { name: "底部视图" });
  const first = tabs.getByRole("tab", { name: /底部 1/ });
  const second = tabs.getByRole("tab", { name: /底部 2/ });
  await expect(second).toHaveAttribute("aria-selected", "true");
  await first.click();
  await expect(first).toHaveAttribute("aria-selected", "true");
  await page.evaluate(() => {
    for (const view of (window as any).__UI_TEST__.views) if (view.placement === "bottom") view.active = view.viewId === "bottom-2";
    (window as any).__UI_TEST__.changed();
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
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "views.activate").at(-1)?.params.viewId)).toBe("bottom-1");
});

test("detached terminal has a drag handle with cancellation and native drop request", async ({ page }) => {
  await page.addInitScript(() => { (window as any).__SODECK_CONTEXT__ = { detachedViewId: "detached-1" }; });
  await page.goto("/");
  await page.evaluate(() => {
    (window as any).__UI_TEST__.views.push({ viewId: "detached-1", viewType: "terminal.session", workspaceId: "server-1", serverId: "server-1", title: "独立测试终端", placement: "detached", group: 0, mountGeneration: 1, status: "active", active: true });
    (window as any).__UI_TEST__.changed();
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
  expect(await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "views.move").length)).toBe(0);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "views.activate").length)).toBe(0);

  await page.mouse.move(rect.x + 16, rect.y + 16); await page.mouse.down();
  await page.mouse.move(1310, rect.y + 50, { steps: 8 }); await page.mouse.up();
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "views.move").at(-1)?.params)).toMatchObject({ viewId: "detached-1", placement: "detached", screenPosition: { x: expect.any(Number), y: expect.any(Number) } });
});

test("drag release consumes its synthetic click without blocking the next normal selection", async ({ page }) => {
  await page.addInitScript(() => { (window as any).__SODECK_CONTEXT__ = { detachedViewId: "drag-view" }; });
  await page.goto("/");
  await page.evaluate(() => {
    (window as any).__UI_TEST__.tabClicks = 0;
    document.addEventListener("click", event => { if ((event.target as HTMLElement).closest(".view-tab")) (window as any).__UI_TEST__.tabClicks++; }, true);
    (window as any).__UI_TEST__.views.push({ viewId: "drag-view", viewType: "terminal.session", workspaceId: "server-1", serverId: "server-1", title: "拖动回归终端", placement: "detached", group: 0, mountGeneration: 1, status: "active", active: true });
    (window as any).__UI_TEST__.changed();
  });
  const tab = page.getByRole("tab", { name: /拖动回归终端/ });
  await expect(tab).toBeVisible();
  const rect = await tab.boundingBox();
  if (!rect) throw Error("No drag source");
  await page.mouse.move(rect.x + 16, rect.y + 16); await page.mouse.down();
  await page.mouse.move(1310, rect.y + 50, { steps: 8 }); await page.mouse.up();
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "views.move").length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.tabClicks)).toBe(1);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "views.activate").length)).toBe(0);

  await tab.click();
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "views.activate").length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__UI_TEST__.tabClicks)).toBe(2);
});

test("a missing-plugin placeholder does not claim a live connection and keeps the recovery error", async ({ page }) => {
  await addServer(page);
  await page.evaluate(() => {
    (window as any).__UI_TEST__.recoverFault = { code: "PLUGIN_UNAVAILABLE", message: "终端插件包不可用，请检查安装状态" };
    (window as any).__UI_TEST__.views.push({ viewId: "missing-view", viewType: "terminal.session", workspaceId: "server-1", serverId: "server-1", title: "缺失插件占位", placement: "center", group: 0, mountGeneration: 1, status: "viewFailed", active: true });
    (window as any).__UI_TEST__.changed();
  });
  const placeholder = page.locator('[data-view-id="missing-view"] .surface-error');
  await expect(placeholder).toContainText("重新加载不会自动连接服务器");
  await expect(placeholder).not.toContainText("连接仍由核心管理");
  await placeholder.getByRole("button", { name: "重新加载视图" }).click();
  await expect(placeholder).toContainText("PLUGIN_UNAVAILABLE：终端插件包不可用，请检查安装状态");
  const before = await page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.surface").length);
  await page.setViewportSize({ width: 1100, height: 720 });
  await expect.poll(() => page.evaluate(() => (window as any).__UI_TEST__.calls.filter((call: any) => call.method === "workbench.surface").length)).toBeGreaterThan(before);
  await expect(placeholder).toContainText("PLUGIN_UNAVAILABLE：终端插件包不可用，请检查安装状态");
  await expect(placeholder.getByRole("button", { name: "重新加载视图" })).toBeEnabled();
});
