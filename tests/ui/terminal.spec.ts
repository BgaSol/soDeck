import { presentationFixture, chooseMenu } from "./presentation-fixture";
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await presentationFixture(page);
  await page.addInitScript(() => {
    const bytes = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));
    const fixture = { requests: [] as any[], input: [] as { sequence: number; text: string }[], chunks: [] as string[], snapshot: null as any, failInput: "", clipboard: "粘贴🙂", readAcknowledged: 0, generation: 1 };
    (window as any).__TERMINAL_TEST__ = fixture;
    (window as any).__SODECK_CONTEXT__ = { viewId: "fixture-view", viewType: "terminal.session", workspaceId: "fixture-server", workspaceGeneration:"fixture-generation", serverId: "fixture-server", serverName: "隔离浏览器契约", pluginData: {sessionId: "fixture-session"}, pluginVersion: "0.1.0", mountGeneration: 1, ...(location.search.includes("recover") ? { snapshot: { recovered: true, acknowledged: 1, lastInputSeq: 4 } } : {}) };
    (window as any).ipc = { postMessage(encoded: string) {
      const request = JSON.parse(encoded); fixture.requests.push(request);
      const presentation=(window as any).__PRESENTATION__.handle(request);if(presentation){queueMicrotask(()=>(window as any).__sodeckReceive(presentation));return;}
      const params = request.params;
      let result: any = {}, error: any;
      if(request.method==='state.open'&&params.contract==='view.presentation.v1'){const scope={workspaceId:'fixture-server',workspaceGeneration:'fixture-generation'};result={subscriptionId:'presentation',feedEpoch:'epoch',sequence:'0',snapshot:{scope,objectId:'fixture-view',revision:'1',value:{scope,viewId:'fixture-view',visible:true,focused:true,revision:1}}};}
      if(['state.close','state.ack'].includes(request.method))result={accepted:true};
      if(request.method==="view.initialize")result={fontSize:14,session:{sessionId:"fixture-session",ptyId:`fixture-pty-${fixture.generation}`,generation:fixture.generation,status:"active"}};
      if (request.method === "plugin.invoke") {
        if (params.command === "settings.get" || params.command === "settings.set") result = { fontSize: params.args.fontSize ?? 14 };
        if (params.command === "sessions.get" || params.command === "sessions.reconnect") {
          if (params.command === "sessions.reconnect") { fixture.generation++; fixture.readAcknowledged = 0; }
          result = { sessionId: "fixture-session", ptyId: `fixture-pty-${fixture.generation}`, generation: fixture.generation, status: "active" };
        }
      }
      if (request.method === "commands.execute" && params.commandId === "terminal.reconnect") { fixture.generation++; fixture.readAcknowledged=0; result={sessionId:"fixture-session",ptyId:`fixture-pty-${fixture.generation}`,generation:fixture.generation,status:"active"}; }
      if (request.method === "view.ready") result = { ready: true };
      if (request.method === "pty.read" && (fixture as any).output && params.after < 2) {result={frames:[{sequence:2,bytes:bytes((fixture as any).output)}],state:"active",lastSequence:2};} else if (request.method === "pty.read") result = { frames: params.after === 0 ? [{ sequence: 1, bytes: bytes("\u001b[32m浏览器契约 · Unicode 🙂\u001b[0m\r\n$ ") }] : [], state: "active", lastSequence: 1 };
      if (request.method === "pty.ack") { fixture.readAcknowledged = params.sequence; result = { acknowledged: params.sequence }; }
      if (request.method === "pty.write") {
        if (fixture.failInput) { error = { code: fixture.failInput, message: "隔离故障注入" }; fixture.failInput = ""; }
        else { const data = Uint8Array.from(atob(params.bytes), c => c.charCodeAt(0)); fixture.input.push({ sequence: params.inputSeq, text: new TextDecoder().decode(data) }); result = { inputSeq: params.inputSeq, acceptedBytes: data.length }; }
      }
      if (request.method === "clipboard.readText") { if((fixture as any).holdClipboard){(fixture as any).releaseClipboard=()=> (window as any).__sodeckReceive({requestId:request.requestId,result:{text:fixture.clipboard}});return;} result = { text: fixture.clipboard }; }
      if (request.method === "clipboard.writeText") fixture.clipboard = params.text;
      if (request.method === "view.snapshotChunk") { fixture.chunks[params.index] = params.data; if (params.index + 1 === params.total) fixture.snapshot = JSON.parse(fixture.chunks.join("")); result = { accepted: true }; }
      queueMicrotask(() => (window as any).__sodeckReceive({ requestId: request.requestId, ...(error ? { error } : { result }) }));
    } };
  });
  await page.goto("http://127.0.0.1:1432");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.readAcknowledged)).toBe(1);
});

test("xterm consumes bytes, fits its container and sends exact UTF-8 paste", async ({ page }) => {
  await expect(page.getByText("已连接", { exact: true })).toBeVisible();
  await page.locator(".xterm-helper-textarea").focus(); await page.keyboard.press("Control+v");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input.map((item: any) => item.text).join(""))).toBe("粘贴🙂");
  const dimensions = await page.evaluate(() => (window as any).__TERMINAL_TEST__.requests.filter((item: any) => item.method === "pty.resize").at(-1)?.params);
  expect(dimensions.cols).toBeGreaterThan(2); expect(dimensions.rows).toBeGreaterThan(1);
  await page.locator(".xterm-helper-textarea").press("Shift+F10");await chooseMenu(page,"larger");await expect(page.locator(".terminal-status")).toContainText("字号 15");
  await page.screenshot({ path: ".qa/ui/terminal-contract.png" });
});

test('live palettes preserve terminal instance, selection, composition, output and input sequence',async({page})=>{
 await page.evaluate(()=>(window as any).__TERMINAL_TEST__.output=Array.from({length:100},(_,i)=>`selection-row ${i} Unicode\r\n`).join(''));
 await expect.poll(()=>page.evaluate(()=>(window as any).__TERMINAL_TEST__.readAcknowledged)).toBe(2);
 const firstRow=page.locator('.xterm-rows>div').first();const bottomRow=await firstRow.innerText();await page.locator('.xterm-screen').hover();await page.mouse.wheel(0,-400);await expect.poll(()=>firstRow.innerText()).not.toBe(bottomRow);
 // Let xterm finish the wheel gesture before recording the preserved viewport.
 await page.waitForTimeout(200);
 const row=await page.locator('.xterm-rows>div').first().boundingBox();await page.mouse.dblclick(row!.x+20,row!.y+7);await page.keyboard.press('Control+Shift+c');
 const selected=await page.evaluate(()=>(window as any).__TERMINAL_TEST__.clipboard);expect(selected).toContain('selection');const visibleFirstRow=await firstRow.innerText();
 const textarea=page.locator('.xterm-helper-textarea');await textarea.focus();await page.evaluate(()=>(window as any).__terminalNode=document.querySelector('.xterm'));
 const before=await page.evaluate(()=>(window as any).__TERMINAL_TEST__.requests.filter((r:any)=>['view.initialize','pty.resize'].includes(r.method)).length);
 await page.evaluate(()=>(window as any).__PRESENTATION__.change('light'));await expect(page.locator('html')).toHaveAttribute('data-theme','soft-light');
 expect(await page.evaluate(()=>(window as any).__terminalNode===document.querySelector('.xterm'))).toBe(true);await expect(textarea).toBeFocused();
 expect(await page.evaluate(()=>(window as any).__TERMINAL_TEST__.requests.filter((r:any)=>['view.initialize','pty.resize'].includes(r.method)).length)).toBe(before);
 await page.keyboard.press('Control+Shift+c');expect(await page.evaluate(()=>(window as any).__TERMINAL_TEST__.clipboard)).toBe(selected);expect(await firstRow.innerText()).toBe(visibleFirstRow);
 await page.evaluate(()=>(window as any).__TERMINAL_TEST__.clipboard='粘贴🙂');
 await page.evaluate(()=>{document.querySelector('textarea')!.dispatchEvent(new CompositionEvent('compositionstart',{data:'中',bubbles:true}));(window as any).__PRESENTATION__.change('dark');});await expect(page.locator('html')).toHaveAttribute('data-theme','soft-dark');await expect(textarea).toBeFocused();
 await page.evaluate(()=>document.querySelector('textarea')!.dispatchEvent(new CompositionEvent('compositionend',{data:'',bubbles:true})));await page.keyboard.press('Control+v');await expect.poll(()=>page.evaluate(()=>(window as any).__TERMINAL_TEST__.input.map((i:any)=>i.text).join(''))).toBe('粘贴🙂');
 await expect(page.locator('.xterm-rows')).toContainText('selection-row');
});

test("snapshot freezes input and resumes only after native cancellation", async ({ page }) => {
  await page.evaluate(() => (window as any).__sodeckNotify({ type: "view.prepareMove", payload: { operationId: "fixture-move" } }));
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.snapshot?.schema)).toBe(1);
  const snapshot = await page.evaluate(() => (window as any).__TERMINAL_TEST__.snapshot);
  expect(snapshot.cursor).toBe(1); expect(snapshot.screen).toContain("Unicode");
  await page.locator(".xterm-helper-textarea").focus();await page.keyboard.press("Control+v");expect(await page.evaluate(()=>(window as any).__TERMINAL_TEST__.input.length)).toBe(0);
  await page.evaluate(() => (window as any).__sodeckNotify({ type: "view.moveCancelled", payload: { operationId: "fixture-move" } }));

  await page.locator(".xterm-helper-textarea").focus(); await page.keyboard.press("Control+v");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input.length)).toBe(1);
});

test("backpressure does not skip input sequence; an unknown result blocks further input", async ({ page }) => {
  await page.evaluate(() => { (window as any).__TERMINAL_TEST__.failInput = "BACKPRESSURE"; });
  await page.locator(".xterm-helper-textarea").focus(); await page.keyboard.press("Control+v");
  await expect(page.getByRole("alert")).toContainText("剩余输入未发送");
  await page.locator(".xterm-helper-textarea").focus(); await page.keyboard.press("Control+v");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input[0]?.sequence)).toBe(1);
  await page.evaluate(() => { (window as any).__TERMINAL_TEST__.failInput = "RESULT_UNKNOWN"; });
  await page.locator(".xterm-helper-textarea").focus(); await page.keyboard.press("Control+v");
  await expect(page.getByText("输入已暂停，请先核对远端执行结果")).toBeVisible();
  await page.locator(".xterm-helper-textarea").focus(); await page.keyboard.press("Control+v");
  expect(await page.evaluate(() => (window as any).__TERMINAL_TEST__.input.length)).toBe(1);
});

test("a recovered renderer uses authoritative output and input cursors", async ({ page }) => {
  await page.goto("http://127.0.0.1:1432/?recover");
  await expect(page.getByText("已连接", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toContainText("旧屏幕无法恢复");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.requests.find((item: any) => item.method === "pty.read")?.params.after)).toBe(1);
  await page.locator(".xterm-helper-textarea").focus(); await page.keyboard.press("Control+v");
  await expect.poll(() => page.evaluate(() => (window as any).__TERMINAL_TEST__.input[0]?.sequence)).toBe(5);
});

test('late clipboard reply after blur is discarded and one key plus DOM paste reads only once',async({page})=>{
 await page.evaluate(()=>(window as any).__TERMINAL_TEST__.holdClipboard=true);await page.locator('.xterm-helper-textarea').focus();await page.keyboard.press('Control+v');await page.locator('.terminal-grid').dispatchEvent('paste');
 expect(await page.evaluate(()=>(window as any).__TERMINAL_TEST__.requests.filter((r:any)=>r.method==='clipboard.readText').length)).toBe(1);
 await page.evaluate(()=>{window.dispatchEvent(new Event('blur'));(window as any).__TERMINAL_TEST__.releaseClipboard();});
 await page.waitForTimeout(50);expect(await page.evaluate(()=>(window as any).__TERMINAL_TEST__.input.length)).toBe(0);
 await page.evaluate(()=>(window as any).__TERMINAL_TEST__.holdClipboard=false);await page.locator('.xterm-helper-textarea').focus();await page.keyboard.press('Control+Shift+v');await expect.poll(()=>page.evaluate(()=>(window as any).__TERMINAL_TEST__.input.length)).toBe(1);
});
test('xterm bracketed paste and remote mouse mode keep their protocol semantics',async({page})=>{
 await page.evaluate(()=>(window as any).__TERMINAL_TEST__.output='\x1b[?2004h\x1b[?1000h');await expect.poll(()=>page.evaluate(()=>(window as any).__TERMINAL_TEST__.readAcknowledged)).toBe(2);
 await page.locator('.xterm-helper-textarea').focus();await page.keyboard.press('Shift+Insert');await expect.poll(()=>page.evaluate(()=>(window as any).__TERMINAL_TEST__.input.map((i:any)=>i.text).join(''))).toBe('\x1b[200~粘贴🙂\x1b[201~');
 await page.locator('.terminal-grid').dispatchEvent('contextmenu',{clientX:50,clientY:50,bubbles:true});expect(await page.evaluate(()=>(window as any).__PRESENTATION__.menu)).toBeNull();
 await page.evaluate(()=>document.querySelector('.terminal-grid')!.dispatchEvent(new MouseEvent('contextmenu',{clientX:50,clientY:50,shiftKey:true,bubbles:true,cancelable:true})));await chooseMenu(page,null);
});
