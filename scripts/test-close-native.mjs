// Windows packaged WebView2 + actual core/plugins/SSH, with isolated app data.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium } from 'playwright';

assert.equal(process.platform, 'win32', 'This harness controls native WebView2 on Windows');
const [executable] = process.argv.slice(2);
assert(executable && fs.existsSync(executable), 'Provide the actual packaged executable');
const ssh = JSON.parse(fs.readFileSync(process.env.SODECK_TEST_SSH_CONFIG, 'utf8'));
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-close-native-'));
const output = path.resolve('artifacts/close-lifecycle');
fs.mkdirSync(output, { recursive: true });
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
  await page.evaluate(()=>window.resizeFrames=[...document.querySelectorAll('.server-workbench:not([hidden]) iframe')]);
  const drag=async(side,delta)=>{const handle=board.locator(`[data-resize="${side}"]`);const r=await handle.boundingBox();await page.mouse.move(r.x+4,r.y+80);await page.mouse.down();await page.mouse.move(r.x+4+delta,r.y+80,{steps:12});};
  const target=Math.min(350,before.room-280);
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
    assert((await terminalFrame.locator('.xterm-rows').innerText()).replace(/\s+/g,'').split(marker).length>=3,'Native resize discarded the existing terminal output');
    await page.getByRole('button', { name: '网络隧道', exact: true }).click();
    const sidebar = page.frameLocator('iframe[title^="网络隧道 ·"]');
    await sidebar.getByRole('button', { name: '＋ 新建', exact: true }).click();
    const ruleEditor = page.frameLocator('iframe[title^="编辑网络隧道 ·"]');
    await ruleEditor.locator('input[name=name]').fill(`Native ${entry}`);
    await ruleEditor.locator('input[name=host]').fill('127.0.0.1');
    await ruleEditor.locator('input[name=targetPort]').fill(String(service.address().port));
    await ruleEditor.getByRole('button', { name: '保存并启动', exact: true }).click();
    await page.locator('.plugin-view-dialog').waitFor({ state: 'detached' });
    await sidebar.getByRole('button', { name: `Native ${entry}` }).click();
    await sidebar.locator('.entry code').waitFor();
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
    const editor = editorFrame.getByRole('textbox', { name: '远端文件编辑器' });
    await editor.waitFor();
    await until(() => editor.isEnabled(), 'Remote document did not load');
    const draft = `native unsaved draft ${entry}`;
    await editor.fill(draft);
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
