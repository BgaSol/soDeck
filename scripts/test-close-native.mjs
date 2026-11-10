// Windows packaged WebView2 + actual core/plugins/SSH, with isolated app data.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium } from 'playwright';
import {fileDocument} from './file-editor-fixture.mjs';

assert.equal(process.platform, 'win32', 'This harness controls native WebView2 on Windows');
const [executable] = process.argv.slice(2);
assert(executable && fs.existsSync(executable), 'Provide the actual packaged executable');
const ssh = JSON.parse(fs.readFileSync(process.env.SODECK_TEST_SSH_CONFIG, 'utf8'));
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-close-native-'));
const output = path.resolve('artifacts/close-lifecycle');
fs.mkdirSync(output, { recursive: true });
fs.rmSync(path.join(output, 'native-close.json'), { force: true });
const records = ['TERMINAL', 'FILES', 'NETWORK', 'SYSTEM_STATUS'].map(name => {
  const root = process.env[`SODECK_TEST_${name}_PLUGIN`];
  assert(root && fs.existsSync(root), `Provide the independent ${name} plugin package`);
  return { directory: path.resolve(root), manifest: JSON.parse(fs.readFileSync(path.join(root, 'plugin.json'), 'utf8')), enabled: true };
});
fs.writeFileSync(path.join(directory, 'plugins.json'), JSON.stringify(records));
const checks = [];
let child, browser, page, held;
let spawnError;
const sockets = new Set();
const service = net.createServer(socket => {
  sockets.add(socket);
  socket.on('data', data => socket.write(data));
  socket.on('error', () => {});
  socket.on('close', () => sockets.delete(socket));
});
const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
async function until(predicate, message, milliseconds = 20000) {
  const deadline = Date.now() + milliseconds;
  while (!await predicate()) { assert(Date.now() < deadline, message); await delay(50); }
}
async function invoke(command, args = {}) {
  return page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
}
async function nativePointer(action, plan) {
  const driver = process.env.SODECK_TEST_NATIVE_POINTER;
  assert(driver && fs.existsSync(driver), 'Provide the same-architecture native-pointer-fixture');
  const helper = spawn(driver, [String(child.pid), action, JSON.stringify(plan)], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '', errors = '';
  helper.stdout.on('data', data => { output += data; });
  helper.stderr.on('data', data => { errors += data; });
  await new Promise((resolve, reject) => { helper.once('error', reject); helper.once('exit', code => code === 0 ? resolve() : reject(new Error(`Native pointer helper failed: ${errors}`))); });
  return output.trim() ? JSON.parse(output) : undefined;
}
async function launch() {
  spawnError = undefined;
  const socket = net.createServer();
  await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
  const port = socket.address().port;
  await new Promise(resolve => socket.close(resolve));
  child = spawn(path.resolve(executable), ['--interactive-test', '--test-data-dir', directory], {
    windowsHide: true, stdio: 'ignore',
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, WEBVIEW2_USER_DATA_FOLDER: path.join(directory, 'webview2') },
  });
  child.on('error', error => { spawnError = error; });
  await until(async () => {
    if (spawnError) throw spawnError;
    assert.equal(child.exitCode, null, 'Native app exited before WebView2 was ready');
    try { return (await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(300) })).ok; } catch { return false; }
  }, 'WebView2 inspector did not open');
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  await until(() => {
    page = browser.contexts().flatMap(context => context.pages()).find(p => p.url().includes('tauri.localhost'));
    return Boolean(page);
  }, 'Packaged main page not found');
  await page.getByRole('button', { name: '关闭窗口', exact: true }).waitFor();
  await until(async () => (await invoke('snapshot')).plugins.every(p => p.status === 'running'), 'Plugins did not start');
  assert.equal((await invoke('snapshot')).dataDirectory.toLowerCase(), directory.toLowerCase());
}
async function connect() {
  let catalog = await invoke('server_catalog');
  let server = catalog.servers[0];
  if (!server) {
    const changed = await invoke('change_catalog', { revision: catalog.revision, change: { op: 'saveServer', draft: {
      name: 'Native close test', host: '127.0.0.1', port: ssh.port, username: ssh.username,
      authentication: { type: 'privateKey', privateKeyPath: ssh.privateKeyPath, secret: { action: 'clear' } }, route: [],
    } } });
    catalog = await invoke('server_catalog');
    server = catalog.servers.find(s => s.id === changed.selectedId);
  }
  await invoke('connection', { command: { op: 'start', serverId: server.id } });
  await until(async () => {
    const [view] = await invoke('connection', { command: { op: 'snapshot' } });
    assert.notEqual(view.state, 'failed', JSON.stringify(view.error));
    if (view.question) await invoke('connection', { command: { op: 'answer', attemptId: view.attemptId, questionId: view.question.id, answer: { kind: 'hostKey', accept: true } } });
    return view.state === 'connected';
  }, 'SSH did not connect');
  await until(async () => {
    const ui = await invoke('component_ui', { operation: 'snapshot', args: {} });
    return Object.values(ui.workspaces).some(w => w.views.some(v => v.pluginId === 'sodeck.system-status'));
  }, 'Hidden background status view did not initialize');
  const projection = await invoke('workspace_snapshot');
  return projection.workspaces[0];
}
async function verifyPanelResize(workspace, terminalFrame, entry) {
  const id=workspace.context.workspaceId;
  await invoke('workspace_layout',{id,side:'left',width:280});await invoke('workspace_layout',{id,side:'right',width:280});
  // Inspector viewport emulation tests the packaged WebView without adding window API permissions.
  await page.setViewportSize({width:1000,height:720});
  const board=page.locator('.server-workbench:visible');
  await until(async()=>board.evaluate(el=>el.classList.contains('left-open')&&el.classList.contains('right-open')), 'Both native sidebars did not open');
  const width=side=>board.evaluate((el,side)=>parseFloat(el.style.getPropertyValue(`--effective-${side}-width`)),side);
  await until(async()=>(await width('left'))===280&&(await width('right'))===280,'Native baseline widths were not applied');
  const before=await board.evaluate(el=>({room:el.getBoundingClientRect().width-[...el.querySelectorAll('.workbench-rail')].reduce((n,r)=>n+r.getBoundingClientRect().width,0)-320,frames:[...el.querySelectorAll('iframe')].length}));
  await page.evaluate(()=>window.resizeFrames=[...document.querySelectorAll('.component-surface:not([hidden]) iframe')]);
  const drag=async(side,delta)=>{const handle=board.locator(`[data-resize="${side}"]`);const r=await handle.boundingBox();await page.mouse.move(r.x+4,r.y+80);await page.mouse.down();await page.mouse.move(r.x+4+delta,r.y+80,{steps:12});};
  const target=Math.min(350,Math.floor(before.room)-280);
  await drag('left',70);await until(async()=>(await width('left'))===target,'Native preview did not follow the pointer');
  await invoke('workspace_activate',{id});
  await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  assert.equal(await width('left'),target,'Native workspace refresh replaced the captured preview');
  await page.mouse.up();
  await until(async()=>(await invoke('workspace_snapshot')).workspaces.find(w=>w.context.workspaceId===id).layout.leftWidth===target,'Native resize did not commit');
  await drag('right',20);await page.mouse.up();
  await until(async()=>(await invoke('workspace_snapshot')).workspaces.find(w=>w.context.workspaceId===id).layout.rightWidth===260,'Native right resize did not commit');
  await drag('left',-20);await page.keyboard.press('Escape');await page.mouse.up();
  await until(async()=>(await width('left'))===target,'Native cancellation did not restore the width');
  assert.equal(await page.evaluate(()=>document.body.classList.contains('panel-resizing')),false);
  assert.equal(await page.evaluate(()=>window.resizeFrames.every(frame=>frame.isConnected)),true);
  await until(async()=>terminalFrame.locator('.xterm-screen').evaluate(el=>{const host=document.querySelector('#terminal'),style=getComputedStyle(host),screen=el.getBoundingClientRect();return screen.width<=host.clientWidth-parseFloat(style.paddingLeft)-parseFloat(style.paddingRight)+1&&screen.height<=host.clientHeight-parseFloat(style.paddingTop)-parseFloat(style.paddingBottom)+1;}), 'Native xterm did not fit the resized iframe');
  const saved=JSON.parse(fs.readFileSync(path.join(directory,'ui-layout.json'),'utf8'));
  assert.equal(saved.leftWidth,target);assert.equal(saved.rightWidth,260);
  const resizeOutput=path.resolve('artifacts/panel-resize');fs.mkdirSync(resizeOutput,{recursive:true});
  const geometry=await terminalFrame.locator('body').evaluate(()=>{
    const describe=el=>{const rect=el.getBoundingClientRect(),css=getComputedStyle(el);return{tag:el.tagName,id:el.id,className:el.className,x:rect.x,y:rect.y,width:rect.width,height:rect.height,display:css.display,background:css.backgroundColor,overflow:css.overflow,clientWidth:el.clientWidth,clientHeight:el.clientHeight,scrollWidth:el.scrollWidth,scrollHeight:el.scrollHeight};};
    return{viewport:{width:innerWidth,height:innerHeight},nodes:[...document.querySelectorAll('body,#terminal,.xterm,.xterm-viewport,.xterm-scrollable-element,.xterm-screen,.scrollbar,.slider')].map(describe)};
  });
  fs.writeFileSync(path.join(resizeOutput,`native-${entry}-geometry.json`),JSON.stringify(geometry,null,2)+'\n');
  const screen=geometry.nodes.find(node=>node.className==='xterm-screen'),root=geometry.nodes.find(node=>node.className.split(' ').includes('xterm'));
  assert(Math.abs(root.height-screen.height)<=1,'Native xterm viewport must end at the last complete row');
  await page.screenshot({path:path.join(resizeOutput,`native-${entry}.png`)});
  checks.push(`${entry}: native left/right drag, refresh during capture, cancel, saved widths and retained terminal iframe`);
  await page.setViewportSize({width:1100,height:720});
}
async function verifyWorkbench(workspace, terminal, marker, entry) {
  const id=workspace.context.workspaceId;
  const snapshot=await invoke('component_ui',{operation:'snapshot',args:{}});
  const originalGroup=Object.values(snapshot.workspaces[id].host.groups).find(g=>g.viewIds.includes(terminal.viewId)).id;
  assert.equal(await page.locator('.tab-menu-button,[data-component-action="group-menu"]').count(),0);
  const pageTab=page.locator(`.component-tab[data-view="${terminal.viewId}"] [role="tab"]`);
  await pageTab.click({button:'right'});
  assert.deepEqual(await page.getByRole('menuitem').allTextContents(),['移到右侧分屏','移到下方分屏','移到新窗口','关闭标签']);
  await page.keyboard.press('Escape');assert(await pageTab.evaluate(el=>el===document.activeElement));
  await page.locator(`[data-group="${originalGroup}"] .component-tabs`).focus();await page.keyboard.press("Shift+F10");
  assert.deepEqual(await page.getByRole('menuitem').allTextContents(),['左右分屏','上下分屏']);
  await page.getByRole('menuitem',{name:'左右分屏',exact:true}).click();
  await until(async()=>page.locator('.server-workbench:visible .view-group').count().then(n=>n===2),'Native split did not render two groups');
  const splitState=await invoke('component_ui',{operation:'snapshot',args:{}});
  const emptyGroup=Object.keys(splitState.workspaces[id].host.groups).find(g=>g!==originalGroup);
  if (entry === 'titlebar') {
    const geometry = await nativePointer('arrange', { width: 1100, height: 720 });
    const display=await page.context().newCDPSession(page);
    await display.send('Emulation.setDeviceMetricsOverride',{width:Math.floor(geometry.width/geometry.scale),height:Math.floor(geometry.height/geometry.scale),deviceScaleFactor:geometry.scale,mobile:false});
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    console.log('Native pointer geometry:', JSON.stringify({ geometry, viewport: await page.evaluate(() => ({ width: innerWidth, height: innerHeight, dpi: devicePixelRatio, outerWidth, outerHeight })) }));
    await invoke('component_ui',{operation:'action',args:{context:workspace.context,action:{op:'select',viewId:terminal.viewId}}});
    const tab = page.locator(`.component-tab[data-view="${terminal.viewId}"] [data-component-action="select"]`);
    await tab.waitFor();
    const rect = await tab.boundingBox();
    // Real system pointers require the native viewport and monitor DPI.
    const from = { x: Math.round(geometry.x + (rect.x + rect.width / 2) * geometry.scale), y: Math.round(geometry.y + (rect.y + rect.height / 2) * geometry.scale) };
    const to = { x: geometry.right + 50, y: from.y + 70 };
    assert(to.x < geometry.workRight, 'Native tear-out needs desktop space beside the test window');
    await page.evaluate(() => document.addEventListener('pointerdown', event => { window.nativeDragStart = { view: event.target.closest('.component-tab')?.dataset.view, x: event.clientX, y: event.clientY }; }, { once: true, capture: true }));
    await nativePointer('drag', { from, to });
    assert.equal((await page.evaluate(() => window.nativeDragStart))?.view, terminal.viewId, 'Native pointer must start on the requested page tab');
    await display.detach();
  } else {
    await invoke('workbench_move',{id,context:workspace.context,requestId:crypto.randomUUID(),scope:{kind:'view',id:terminal.viewId},target:null,destination:null});
  }
  let secondary;
  await until(async()=>{
    for(const candidate of browser.contexts().flatMap(c=>c.pages())) {
      if(candidate===page||candidate.isClosed())continue;
      const state=await candidate.evaluate(()=>window.__TAURI_INTERNALS__?.invoke('workspace_snapshot')).catch(()=>null);
      if(state?.window.id!=='main'&&state?.workspaces.some(w=>w.context.workspaceId===id)){secondary=candidate;return true;}
    }
    return false;
  },'Native detached page was not found');
  const frame=secondary.frameLocator(`.component-surface[data-view-id="${terminal.viewId}"] iframe`);
  await frame.locator('body[data-ready="true"]').waitFor();
  await until(async()=>(await frame.locator('.xterm-rows').innerText()).replace(/\s+/g,'').split(marker).length>=3,'Native transfer did not restore terminal output');
  const movedMarker=marker+'-moved';
  await frame.locator('.xterm-helper-textarea').focus();
  await secondary.keyboard.type('echo '+movedMarker);await secondary.keyboard.press('Enter');
  await until(async()=>(await frame.locator('.xterm-rows').innerText()).replace(/\s+/g,'').split(movedMarker).length>=3,'Native transferred terminal did not accept input');
  const returnRequest=crypto.randomUUID();
  await secondary.evaluate(args=>window.__TAURI_INTERNALS__.invoke('workbench_move',args),{id,context:workspace.context,requestId:returnRequest,scope:{kind:'view',id:terminal.viewId},target:null,destination:{returnToSource:true}}).catch(error=>{if(!secondary.isClosed())throw error;});
  await until(async()=>secondary.isClosed(),'Empty detached native window was not reclaimed');
  await until(async()=>{const state=await invoke('component_ui',{operation:'snapshot',args:{}});return state.transfers.length===0&&state.workspaces[id].host.groups[originalGroup].viewIds.includes(terminal.viewId);},'Native return did not restore the original group');
  await invoke('component_ui',{operation:'action',args:{context:workspace.context,action:{op:'select',viewId:terminal.viewId}}});
  const returned=page.frameLocator(`.component-surface[data-view-id="${terminal.viewId}"] iframe`);
  await until(async()=>(await returned.locator('.xterm-rows').innerText()).replace(/\s+/g,'').split(movedMarker).length>=3,'Native return lost terminal output');
  await invoke('component_ui',{operation:'action',args:{context:workspace.context,action:{op:'mergeGroup',sourceGroupId:emptyGroup,groupId:originalGroup}}});
  checks.push(`${entry}: native nested-layout entry, page tear-out ${entry === 'titlebar' ? 'with OS pointer' : 'via command'}, live PTY input, original-group return and empty-window reclamation`);
}
try {
  await new Promise(resolve => service.listen(0, '127.0.0.1', resolve));
  for (const entry of ['titlebar', 'native']) {
    await launch();
    const workspace = await connect();
    const terminal = await invoke('component_ui', { operation: 'action', args: { context: workspace.context, action: {
      op: 'open', key: 'sodeck.terminal:terminal', requestId: `${entry}-terminal`,
    } } });
    const terminalFrame = page.frameLocator(`.component-surface[data-view-id="${terminal.viewId}"] iframe`);
    await terminalFrame.locator('body[data-ready="true"]').waitFor();
    await terminalFrame.locator('.xterm-helper-textarea').focus();
    const marker = `sodeck-close-pty-${entry}`;
    await page.keyboard.type(`echo ${marker}`);
    await page.keyboard.press('Enter');
    await until(async () => (await terminalFrame.locator('.xterm-rows').innerText()).replace(/\s+/g, '').split(marker).length >= 3, 'Actual PTY did not echo the command result');
    await verifyPanelResize(workspace,terminalFrame,entry);
    await verifyWorkbench(workspace,terminal,marker,entry);
    assert((await terminalFrame.locator('.xterm-rows').innerText()).replace(/\s+/g,'').split(marker).length>=3,'Native resize discarded the existing terminal output');
    const networkEntry=page.getByRole('button', { name: '网络隧道', exact: true });
    if(await networkEntry.getAttribute('aria-expanded')!=='true')await networkEntry.click();
    const sidebar = page.frameLocator('iframe[title^="网络隧道 ·"]');
    await sidebar.getByRole('button', { name: '＋ 新建', exact: true }).click();
    const ruleEditor = page.frameLocator('iframe[title^="编辑网络隧道 ·"]');
    await ruleEditor.locator('input[name=name]').fill(`Native ${entry}`);
    await ruleEditor.locator('input[name=host]').fill('127.0.0.1');
    await ruleEditor.locator('input[name=targetPort]').fill(String(service.address().port));
    await ruleEditor.getByRole('button', { name: '保存', exact: true }).click();
    await page.locator('.plugin-view-dialog').waitFor({ state: 'detached' });
    await sidebar.getByRole('button', { name: `Native ${entry}` }).click();
    await sidebar.locator('.detail').getByRole('button', { name: '启动', exact: true }).click();
    await sidebar.locator('.entry code').waitFor();
    await until(async () => /127\.0\.0\.1:\d+/.test(await sidebar.locator('.entry code').textContent()), 'Native tunnel did not reach the listening state');
    const address = await sidebar.locator('.entry code').textContent();
    assert.match(address, /127\.0\.0\.1:\d+/);
    const tunnelPort = Number(address.split(':').at(-1));
    held = net.connect(tunnelPort, '127.0.0.1');
    held.on('error', () => {});
    await new Promise((resolve, reject) => { held.once('connect', resolve); held.once('error', reject); });
    async function echo() {
      const data = new Promise((resolve, reject) => {
        held.once('data', bytes => { try { assert.equal(bytes.toString(), marker); resolve(); } catch(error) { reject(error); } });
      });
      held.write(marker);
      await Promise.race([data, delay(8000).then(() => { throw new Error('Actual tunnel did not forward data'); })]);
    }
    await echo();
    const opened = await invoke('component_ui', { operation: 'action', args: { context: workspace.context, action: {
      op: 'open', key: 'sodeck.files:editor', requestId: entry, resourceKey: ssh.remoteFile, params: { path: ssh.remoteFile },
    } } });
    const editorFrame = page.frameLocator(`.component-surface[data-view-id="${opened.viewId}"] iframe`);
    const editor = fileDocument(editorFrame);
    await editor.waitFor();
    await until(() => editor.isEnabled(), 'Remote document did not load');
    const draft = `native unsaved draft ${entry}`;
    await editor.fill(draft);
    const board=page.locator('.server-workbench:visible');
    const beforeDock=await invoke('component_ui',{operation:'snapshot',args:{}});
    const networkTool=beforeDock.workspaces[workspace.context.workspaceId].tools.find(tool=>tool.key.startsWith('sodeck.network-proxy:')&&tool.location?.side);
    assert(networkTool,'Native network tool must have a real slot');
    const networkFrame=await page.locator('iframe[title^="网络隧道 ·"]').elementHandle(),fileFrame=await page.locator('iframe[title^="文件 ·"]').elementHandle();
    const geometry=await nativePointer('arrange',{width:1100,height:720});
    const display=await page.context().newCDPSession(page);
    await display.send('Emulation.setDeviceMetricsOverride',{width:Math.floor(geometry.width/geometry.scale),height:Math.floor(geometry.height/geometry.scale),deviceScaleFactor:geometry.scale,mobile:false});
    await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
    for(const side of ['left','right']){
      const button=await board.locator(`[data-tool-key="${networkTool.key}"]`).boundingBox(),rail=await board.locator(`.${side}-rail`).boundingBox();
      const point=(x,y)=>({x:Math.round(geometry.x+x*geometry.scale),y:Math.round(geometry.y+y*geometry.scale)});
      await nativePointer('drag',{from:point(button.x+button.width/2,button.y+button.height/2),to:point(rail.x+rail.width/2,rail.y+140)});
      await until(()=>board.locator(`.${side}-rail [data-tool-key="${networkTool.key}"]`).getAttribute('aria-expanded').then(value=>value==='true'),'Native tool button did not follow its panel');
      assert(await networkFrame.evaluate(el=>el.isConnected));assert(await fileFrame.evaluate(el=>el.isConnected));
      assert.equal(await editor.inputValue(),draft);assert.equal(await sidebar.locator('.entry code').textContent(),address);await echo();
    }
    await display.detach();
    const fileButton=board.locator('[data-tool-key="sodeck.files:files"]');
    if(await fileButton.getAttribute('aria-expanded')!=='true')await fileButton.click();
    assert.equal(await board.locator('.workbench-rail [aria-label="终端"]').count(),0);
    const afterDock=await invoke('component_ui',{operation:'snapshot',args:{}});
    assert.deepEqual(afterDock.workspaces[workspace.context.workspaceId].views.map(v=>v.id).sort(),beforeDock.workspaces[workspace.context.workspaceId].views.map(v=>v.id).sort());
    fs.mkdirSync('artifacts/workbench-layout',{recursive:true});await page.screenshot({path:`artifacts/workbench-layout/native-tools-${entry}.png`});
    checks.push(`${entry}: native system-pointer tool docking kept button/panel side, original file/network iframes, draft and live forwarding`);
    const before = await invoke('component_ui', { operation: 'snapshot', args: {} });
    const identity = before.workspaces[workspace.context.workspaceId].views.map(v => v.id).sort();
    const pids = (await invoke('snapshot')).plugins.map(p => p.pid);
    assert.equal(pids.length, 4);
    assert(pids.every(pid => Number.isInteger(pid) && pid > 0));
    if (entry === 'titlebar') await page.getByRole('button', { name: '关闭窗口', exact: true }).click();
    else await invoke('plugin:window|close', { label: 'main' });
    await page.locator('.component-close-dialog').getByRole('button', { name: '放弃并关闭', exact: true }).waitFor();
    await page.locator('.component-close-dialog').getByRole('button', { name: '取消', exact: true }).click();
    await page.locator('.component-close-dialog').waitFor({ state: 'detached' });
    assert.equal(child.exitCode, null);
    assert.equal(await editor.inputValue(), draft);
    const after = await invoke('component_ui', { operation: 'snapshot', args: {} });
    assert.deepEqual(after.workspaces[workspace.context.workspaceId].views.map(v => v.id).sort(), identity);
    await echo();
    await editor.fill(`${draft} resumed`);
    await editor.fill(draft);
    assert(!((await editorFrame.locator('body').innerText()).includes('正在确认关闭')), 'Cancellation left a lifecycle error in the file page');
    assert.equal(await sidebar.locator('#error:visible').count(), 0, 'Cancellation left a lifecycle error in the network page');
    await page.screenshot({ path: path.join(output, `${entry}-cancel.png`) });
    if (entry === 'titlebar') await page.getByRole('button', { name: '关闭窗口', exact: true }).click();
    else await invoke('plugin:window|close', { label: 'main' });
    await page.locator('.component-close-dialog').getByRole('button', { name: '放弃并关闭', exact: true }).click();
    await until(() => child.exitCode !== null, 'Native app failed to exit', 20000);
    assert.equal(child.exitCode, 0);
    await until(() => pids.every(pid => !alive(pid)), 'Managed plugins remained alive after exit');
    await until(() => held.destroyed && sockets.size === 0, 'Forwarding streams remained open after exit');
    const rebound = net.createServer();
    await new Promise((resolve, reject) => { rebound.once('error', reject); rebound.listen(tunnelPort, '127.0.0.1', resolve); });
    await new Promise(resolve => rebound.close(resolve));
    const logs = fs.readFileSync(path.join(directory, 'logs/current.jsonl'), 'utf8').trim().split('\n').map(line => JSON.parse(line));
    assert(logs.some(log => log.message === '关闭步骤变更' && log.fields?.phase === 'finalizing'));
    assert.equal(fs.readFileSync(ssh.remoteFile, 'utf8'), 'sodeck-native-original\n', 'Discard must not write the draft remotely');
    checks.push(`${entry}: cancel kept original views/draft, PTY and forwarding; discard stopped four plugins, drained SSH/forwarding, released the port and exited`);
    await browser.close(); browser = undefined; child = undefined;
  }
  const report = { completed: true, platform: process.platform, checks, plugins: records.map(r => ({ id: r.manifest.id, version: r.manifest.version })), executable: path.resolve(executable) };
  fs.writeFileSync(path.join(output, 'native-close.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  if (fs.existsSync(path.join(directory, 'logs/current.jsonl'))) fs.copyFileSync(path.join(directory, 'logs/current.jsonl'), path.join(output, 'native-failure-core.jsonl'));
  fs.writeFileSync(path.join(output, 'native-close.json'), JSON.stringify({ completed: false, checks, error: String(error) }, null, 2) + '\n');
  console.error(error);
  if (page && !page.isClosed()) { console.error(await page.locator('body').innerText()); await page.screenshot({ path: path.join(output, 'native-failure.png') }); }
  throw error;
} finally {
  await browser?.close().catch(() => {});
  if (child && child.exitCode === null) { child.kill(); await new Promise(resolve => child.once('exit', resolve)); }
  held?.destroy();
  sockets.forEach(socket => socket.destroy());
  if (service.listening) await new Promise(resolve => service.close(resolve));
  assert.equal(path.dirname(path.resolve(directory)), path.resolve(os.tmpdir()));
  fs.rmSync(directory, { recursive: true, force: true, maxRetries: 50, retryDelay: 100 });
}
