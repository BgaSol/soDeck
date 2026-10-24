import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { root, launch, rpc, until, stop, pluginPages, delay } from './harness.mjs';

// Observation and isolated setup only. Actual keys/clicks come from Computer Use.
// Never reads or writes the system clipboard; no synthetic key/input events.
const output = join(root, '.qa/input-sky', String(Date.now()));
await mkdir(output, { recursive: true });
const python = process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture = spawn(python, ['-u', join(root, 'tests/desktop/input-fixture.py'), join(output, 'fixture')], {
  cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, PYTHONPATH: join(root, '.local/python') },
});
let fixtureOutput = '', fixtureErrors = '', app, baseline;
const pages = {}, views = {}, channels = {}, checks = [], handled = new Set();
fixture.stdout.on('data', data => fixtureOutput += data);
fixture.stderr.on('data', data => fixtureErrors += data);
const hash = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const save = (file, data) => writeFile(join(output, file), JSON.stringify(data, null, 2));
const fixtureEvents = async () => (await readFile(join(output, 'fixture/events.jsonl'), 'utf8')).split('\n').filter(Boolean).map(line => JSON.parse(line));
async function pageFor(viewId) {
  return until(async () => {
    for (const page of await pluginPages(app)) if (await page.evaluate(() => window.__SODECK_CONTEXT__?.viewId) === viewId) return page;
  });
}
async function observePage(page) {
  return page.evaluate(() => ({
    context: window.__SODECK_CONTEXT__,
    events: window.__inputObservation.events,
    renames: window.__inputObservation.renames,
    renameObserverActive: window.__inputObservation.renameObserverActive,
    focus: { tag: document.activeElement?.tagName, className: document.activeElement?.className, id: document.activeElement?.id },
    screen: document.querySelector('.xterm-rows')?.innerText ?? null,
    listText: document.querySelector('.session-list')?.innerText ?? null,
    editing: document.querySelector('.session-rename input')?.value ?? null,
  }));
}
function identities(resources) {
  return { ptys: resources.ptys.map(item => item.ptyId ?? item.id).sort(), leases: resources.leases.map(item => item.leaseId ?? item.id).sort(), connections: resources.connections.map(item => item.connectionId ?? item.id).sort() };
}
async function observation() {
  const events = await fixtureEvents();
  const received = Object.fromEntries(['A', 'B'].map(name => [name, Buffer.concat(events.filter(event => event.event === 'received' && event.channel === channels[name]).map(event => Buffer.from(event.bytes, 'base64'))).toString('hex')]));
  const surfaces = {};
  for (const [name, page] of Object.entries(pages)) surfaces[name] = await observePage(page);
  const resources = await rpc(app.page, 'resources.list');
  const workbench = await rpc(app.page, 'workbench.snapshot');
  const sessions = await rpc(app.page, 'plugin.invoke', { command: 'sessions.list', args: {} });
  return { received, surfaces, resources, workbench, sessions, identities: identities(resources) };
}
function since(current, previous) {
  return {
    received: Object.fromEntries(['A', 'B'].map(name => [name, current.received[name].slice(previous.received[name].length)])),
    events: Object.fromEntries(Object.keys(pages).map(name => [name, current.surfaces[name].events.slice(previous.surfaces[name].events.length)])),
    renames: current.surfaces.list.renames.slice(previous.surfaces.list.renames.length),
  };
}
try {
  await until(() => { if (fixture.exitCode !== null) throw Error(fixtureErrors); return fixtureOutput.includes('\n'); });
  const { port } = JSON.parse(fixtureOutput.split('\n')[0]);
  app = await launch({ data: join(output, 'app'), shellPort: 9413, pluginPort: 9414 });
  const server = await rpc(app.page, 'servers.create', { name: '原生按键验收', host: '127.0.0.1', port, username: 'test-user', authType: 'password' });
  let challenge;
  try { await rpc(app.page, 'workbench.openTerminal', { serverId: server.id }); } catch (error) { challenge = error; }
  assert.equal(challenge?.code, 'HOST_KEY_REQUIRED');
  await rpc(app.page, 'hosts.trust', { serverId: server.id, ...challenge.details });
  await rpc(app.page, 'auth.set', { serverId: server.id, password: 'isolated-input-password' });
  views.A = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  views.B = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  await rpc(app.page, 'workbench.move', { viewId: views.B.viewId, placement: 'center', group: 1 });
  await rpc(app.page, 'workbench.configure', { sessionsVisible: true, sessionsSide: 'left', split: 'horizontal' });
  await rpc(app.page, 'workbench.activate', { viewId: views.A.viewId });
  for (const name of ['A', 'B']) {
    pages[name] = await pageFor(views[name].viewId);
    const text = await until(() => pages[name].locator('.xterm-rows').innerText().then(text => text.includes('input-fixture-ready:') ? text : null));
    channels[name] = Number(text.match(/input-fixture-ready:(\d+)/)[1]);
  }
  const listView = (await rpc(app.page, 'workbench.snapshot')).views.find(view => view.viewType === 'terminal.sessions');
  pages.list = await pageFor(listView.viewId);
  for (const page of Object.values(pages)) await page.evaluate(() => {
    window.__inputObservation = { events: [], renames: [] };
    for (const type of ['keydown', 'keyup', 'beforeinput', 'input', 'compositionstart', 'compositionupdate', 'compositionend']) document.addEventListener(type, event => {
      window.__inputObservation.events.push({ type, key: event.key ?? null, code: event.code ?? null, data: event.data ?? null, ctrlKey: event.ctrlKey ?? false, shiftKey: event.shiftKey ?? false, isTrusted: event.isTrusted, isComposing: event.isComposing ?? false, target: event.target?.tagName, time: performance.now() });
    }, true);
    // Wry freezes window.ipc.postMessage. Observe its writable WebView2 bridge
    // instead and prove installation succeeded before claiming call counts.
    const bridge = window.chrome?.webview ?? window.ipc;
    const original = bridge.postMessage.bind(bridge);
    const observer = body => {
      try { const message = JSON.parse(body); if (message.method === 'plugin.invoke' && message.params?.command === 'sessions.rename') window.__inputObservation.renames.push(message.params.args); } catch { /* Observe valid packets only. */ }
      return original(body);
    };
    bridge.postMessage = observer;
    window.__inputObservation.renameObserverActive = bridge.postMessage === observer;
    if (!window.__inputObservation.renameObserverActive) throw Error('Native bridge is not writable; cannot count rename calls');
  });
  baseline = await observation();
  const initialIdentities = baseline.identities;
  const ready = { output, pid: app.process.pid, shellPort: 9413, pluginPort: 9414, views, channels, commands: join(output, 'command.json'), clipboard: 'not accessed by this observer; clipboard feature verification deferred because full Windows-format restoration is unproven', hashes: { desktop: await hash(join(root, 'target/debug/sodeck-desktop.exe')), terminal: await hash(join(root, 'resources/terminal.sodeck-plugin')) } };
  await save('ready.json', ready);await save('initial.json', baseline);
  console.log(JSON.stringify({ ready: true, ...ready }));
  const deadline = Date.now() + 20 * 60_000;
  let finished = false;
  while (Date.now() < deadline && !finished) {
    let command;
    try { command = JSON.parse(await readFile(join(output, 'command.json'), 'utf8')); } catch { await delay(100);continue; }
    if (!/^[\w-]{1,80}$/.test(command.id ?? '') || handled.has(command.id)) { await delay(100);continue; }
    handled.add(command.id);
    try {
      if (command.action === 'activate') { assert.ok(views[command.target]);await rpc(app.page, 'workbench.activate', { viewId: views[command.target].viewId }); }
      const current = await observation();
      assert.deepEqual(current.identities, initialIdentities, 'Remote resource identities must remain unchanged');
      const delta = since(current, baseline);
      if (command.action === 'mark') baseline = current;
      else if (command.action === 'verify') {
        if (command.expected) assert.deepEqual(delta.received, command.expected, 'Exact bytes since mark, for both PTYs');
        if (command.expectedTitle) for (const [name, title] of Object.entries(command.expectedTitle)) {
          assert.equal(current.sessions.find(session => session.sessionId === views[name].sessionId)?.title, title);
          assert.equal(current.workbench.views.find(view => view.viewId === views[name].viewId)?.title, title);
          assert.ok(current.surfaces.list.listText.includes(title));
        }
        if (command.renameCount !== undefined) {
          assert.equal(current.surfaces.list.renameObserverActive, true, 'Rename observer must be installed');
          assert.equal(delta.renames.length, command.renameCount);
        }
        if (command.requireTrustedTarget) assert.ok(delta.events[command.requireTrustedTarget].some(event => event.isTrusted && ['keydown', 'input'].includes(event.type)), 'Must observe trusted native key/input events');
        checks.push({ id: command.id, expected: command.expected, expectedTitle: command.expectedTitle, renameCount: command.renameCount, delta });
      } else if (!['observe', 'activate', 'finish'].includes(command.action)) throw Error('Unknown command action');
      await save(`observation-${command.id}.json`, current);
      if (command.screenshots) for (const [name, page] of Object.entries(pages)) await page.screenshot({ path: join(output, `${command.id}-${name}.png`) });
      await save(`response-${command.id}.json`, { ok: true, action: command.action, delta, titles: current.sessions.map(session => ({ sessionId: session.sessionId, title: session.title })), editing: current.surfaces.list.editing, identityUnchanged: true });
      console.log(JSON.stringify({ response: command.id, ok: true }));
      finished = command.action === 'finish';
    } catch (error) {
      await save(`response-${command.id}.json`, { ok: false, error: { message: error.message, stack: error.stack } });
      console.log(JSON.stringify({ response: command.id, ok: false, error: error.message }));
    }
  }
  assert.ok(finished, 'Timed out waiting for native input controller');
  assert.ok(checks.length > 0, 'No actual input assertions completed');
  await stop(app);assert.equal(app.process.exitCode, 0);app = null;
  await until(async () => { const events = await fixtureEvents();return events.filter(event => event.event === 'shell-close').length === 2 && events.filter(event => event.event === 'connection-close').length === 2; }, 5000);
  await save('result.json', { passed: true, scope: 'Windows native input via Computer Use; loopback SSH byte echo, not Linux shell', checks, clipboard: { executed: false, reason: 'Full original Windows clipboard formats cannot be guaranteed restorable through browser API' }, normalExit: 0 });
  console.log(JSON.stringify({ passed: true, output, checks: checks.length }));
} catch (error) {
  await save('result.json', { passed: false, checks, error: { message: error.message, stack: error.stack }, fixtureErrors });
  console.error(error);process.exitCode = 1;
} finally {
  await stop(app);fixture.kill();await writeFile(join(output, 'fixture-stderr.log'), fixtureErrors);
}
