import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile, rm, copyFile } from 'node:fs/promises';
import { join } from 'node:path';
import { root, launch, rpc, until, stop, pluginPages, pluginRpc } from './harness.mjs';

const output = join(root, '.qa/move-faults', String(Date.now()));
await mkdir(output, { recursive: true });
console.log(JSON.stringify({ phase: 'starting', output }));
const python = process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture = spawn(python, ['-u', join(root, 'tests/core/ssh_fixture.py'), join(output, 'fixture')], {
  cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, PYTHONPATH: join(root, '.local/python') },
});
let stdout = '', stderr = '', fixtureError, app, source;
const checks = [], evidence = {};
fixture.stdout.on('data', chunk => stdout += chunk);
fixture.stderr.on('data', chunk => stderr += chunk);
fixture.on('error', error => fixtureError = error);
const invoke = (command, args = {}) => rpc(app.page, 'plugin.invoke', { command, args });
const identity = resources => Object.fromEntries(['connections', 'leases', 'ptys'].map(kind => [kind, resources[kind].map(item => item.id).sort()]));
async function pagesFor(viewId) {
  const matches = [];
  for (const page of await pluginPages(app)) {
    if (await page.evaluate(() => window.__SODECK_CONTEXT__?.viewId).catch(() => undefined) === viewId) matches.push(page);
  }
  return matches;
}
async function terminalPage(viewId) {
  return until(async () => {
    for (const page of await pagesFor(viewId)) {
      if ((await page.locator('.xterm-rows').innerText()).includes('fixture-ready')) return page;
    }
  });
}
async function echo(page, marker) {
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type(marker);
  await until(() => page.locator('.xterm-rows').innerText().then(text => text.replace(/\s/g, '').includes(marker)));
}
async function stable(first, before, placement, generation) {
  const workbench = await rpc(app.page, 'workbench.snapshot');
  const view = workbench.views.find(view => view.viewId === first.viewId);
  assert.equal(view.placement, placement); assert.equal(view.mountGeneration, generation);
  assert.equal(view.sessionId, first.sessionId); assert.equal(view.ptyId, first.ptyId);
  const resources = await rpc(app.page, 'resources.list');
  assert.deepEqual(identity(resources), identity(before.resources));
  const sessions = await invoke('sessions.list');
  assert.deepEqual(sessions.map(session => session.sessionId).sort(), before.sessions.map(session => session.sessionId).sort());
  return { view, resources, sessions };
}
async function remoteEvents() {
  return (await readFile(join(output, 'fixture/events.jsonl'), 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
}

try {
  const hashes = {};
  const artifacts = join(output, 'artifacts'); await mkdir(artifacts);
  const executable = join(artifacts, 'sodeck-desktop.exe'), packagePath = join(artifacts, 'terminal.sodeck-plugin'), keysPath = join(artifacts, 'development-public-key.json');
  for (const [path, destination] of [['target/debug/sodeck-desktop.exe', executable], ['resources/terminal.sodeck-plugin', packagePath], ['resources/development-public-key.json', keysPath]]) {
    await copyFile(join(root, path), destination);
    hashes[path] = createHash('sha256').update(await readFile(destination)).digest('hex');
  }
  evidence.artifacts = { hashes, executable, packagePath, keysPath, node: process.version, platform: process.platform };
  await until(() => {
    if (fixtureError) throw fixtureError;
    if (fixture.exitCode !== null) throw Error('Fixture exited: ' + stderr);
    return stdout.includes('\n');
  });
  const { port } = JSON.parse(stdout.split('\n')[0]);
  app = await launch({ data: join(output, 'app'), shellPort: 9403, pluginPort: 9404, executable, packagePath, keysPath });
  const server = await rpc(app.page, 'servers.create', { name: '迁移失败隔离验证', host: '127.0.0.1', port, username: 'test-user', authType: 'password' });
  let challenge;
  await assert.rejects(rpc(app.page, 'workbench.openTerminal', { serverId: server.id }), error => {
    assert.equal(error.code, 'HOST_KEY_REQUIRED'); challenge = error.details; return true;
  });
  await rpc(app.page, 'hosts.trust', { serverId: server.id, ...challenge });
  await rpc(app.page, 'auth.ssh.set', { serverId: server.id, password: 'isolated-test-password', remember: false });
  const first = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  const peer = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  await terminalPage(peer.viewId);
  await rpc(app.page, 'workbench.activate', { viewId: first.viewId });
  source = await terminalPage(first.viewId);
  await echo(source, 'before-faults;');
  evidence.before = { resources: await rpc(app.page, 'resources.list'), sessions: await invoke('sessions.list') };
  assert.equal(evidence.before.resources.ptys.length, 2);
  assert.equal(evidence.before.resources.connections.length, 1);

  // A real source serializes/finalizes its snapshot; change only its schema on
  // the page-to-native bridge so the actual target must reject restoration.
  await source.evaluate(() => {
    const previous = window.chrome.webview.postMessage.bind(window.chrome.webview);
    window.__moveFaultRestore = () => { window.chrome.webview.postMessage = previous; };
    window.__snapshotFault = null;
    window.chrome.webview.postMessage = body => {
      const request = JSON.parse(body);
      if (request.method === 'view.snapshotChunk') {
        if (request.params.total !== 1) throw new Error('Schema fault fixture requires one small actual snapshot');
        const state = JSON.parse(request.params.data); state.schema = 999;
        request.params.data = JSON.stringify(state);
        window.__snapshotFault = { requestId: request.requestId, operationId: request.params.operationId, schema: state.schema };
        return previous(JSON.stringify(request));
      }
      return previous(body);
    };
  });
  const schemaStarted = Date.now();
  const schemaMove = rpc(app.page, 'workbench.move', { viewId: first.viewId, placement: 'detached' }).then(result => ({ result }), error => ({ error: { code: error.code, message: error.message } }));
  const incompatible = await until(async () => (await pagesFor(first.viewId)).find(page => page !== source));
  evidence.incompatible = { targetUrl: incompatible.url(), injection: await source.evaluate(() => window.__snapshotFault) };
  evidence.incompatible.notice = await until(() => incompatible.locator('[role="alert"]').allTextContents().then(texts => texts.find(text => text.includes('快照版本不兼容'))));
  const schemaResult = await schemaMove;
  assert.equal(schemaResult.error?.code, 'MOVE_TIMEOUT');
  evidence.incompatible.result = schemaResult; evidence.incompatible.elapsedMs = Date.now() - schemaStarted;
  evidence.incompatible.injection = await source.evaluate(() => window.__snapshotFault);
  await source.evaluate(() => window.__moveFaultRestore());
  await until(() => incompatible.isClosed());
  evidence.incompatible.after = await stable(first, evidence.before, 'center', first.mountGeneration);
  assert.equal((await pagesFor(first.viewId)).length, 1); assert.equal(source.isClosed(), false);
  await echo(source, 'schema-failure-recovered;');
  checks.push('actual target DOM reports the incompatible migration snapshot and is destroyed at the unchanged deadline; original surface/session/resources recover and echo');
  console.log(JSON.stringify({ phase: 'schema-passed', output }));

  // New-document hook changes only the next candidate's ready message. This
  // covers a target that exists but becomes ready too late, not a slow native
  // WebView constructor or the native dispatcher queue itself.
  await source.context().addInitScript(() => {
    const install = () => {
      const config = JSON.parse(localStorage.getItem('__sodeckMoveFault') ?? 'null');
      if (!config) return true;
      const ctx = window.__SODECK_CONTEXT__;
      if (!ctx || !window.ipc) return false;
      if (ctx.viewId !== config.viewId || ctx.mountGeneration !== config.generation) return true;
      const previous = window.chrome.webview.postMessage.bind(window.chrome.webview);
      window.__readyFault = { installedAt: Date.now() };
      window.chrome.webview.postMessage = body => {
        const request = JSON.parse(body);
        if (request.method === 'view.ready') {
          window.__readyFault.held = request; window.__readyFault.heldAt = Date.now();
          return;
        }
        return previous(body);
      };
      return true;
    };
    if (!install()) { const timer = setInterval(() => { if (install()) clearInterval(timer); }, 0); }
  });
  await source.evaluate(config => localStorage.setItem('__sodeckMoveFault', JSON.stringify(config)), { viewId: first.viewId, generation: first.mountGeneration + 1 });
  const readyStarted = Date.now();
  const readyMove = rpc(app.page, 'workbench.move', { viewId: first.viewId, placement: 'detached' }).then(result => ({ result }), error => ({ error: { code: error.code, message: error.message } }));
  const late = await until(async () => (await pagesFor(first.viewId)).find(page => page !== source));
  evidence.lateReady = { targetUrl: late.url(), held: await until(() => late.evaluate(() => window.__readyFault?.held && window.__readyFault)) };
  // The old page still exists during preparation, but it has already lost its
  // input authority. A rejected probe must not consume the next input sequence.
  await assert.rejects(pluginRpc(source, 'pty.write', { ptyId: first.ptyId, generation: first.generation, inputSeq: 999999, bytes: Buffer.from('forbidden-old-input').toString('base64') }), error => {
    evidence.lateReady.oldWriteError = { code: error.code, message: error.message }; return error.code === 'MOUNT_EXPIRED';
  });
  const readyResult = await readyMove;
  assert.equal(readyResult.error?.code, 'MOVE_TIMEOUT');
  evidence.lateReady.result = readyResult; evidence.lateReady.elapsedMs = Date.now() - readyStarted;
  await source.evaluate(() => localStorage.removeItem('__sodeckMoveFault'));
  await until(() => late.isClosed());
  evidence.lateReady.after = await stable(first, evidence.before, 'center', first.mountGeneration);
  assert.equal((await pagesFor(first.viewId)).length, 1);
  await echo(source, 'late-ready-recovered;');
  assert.equal((await source.locator('.xterm-rows').innerText()).includes('forbidden-old-input'), false);
  checks.push('held target readiness times out at the original 10-second deadline; stale source writes are rejected, failed candidate is destroyed and original input/echo resumes');
  console.log(JSON.stringify({ phase: 'late-ready-passed', output }));

  // Discard real responses after native execution: the source's final snapshot
  // acknowledgement, and the shell SDK's move response. Do not retry mutation.
  await source.evaluate(() => {
    const previousSend = window.chrome.webview.postMessage.bind(window.chrome.webview), previousReceive = window.__sodeckReceive;
    const dropped = new Set(); window.__snapshotResponseDrops = [];
    localStorage.setItem('__snapshotResponseDrops', '[]');
    window.chrome.webview.postMessage = body => {
      const request = JSON.parse(body);
      if (request.method === 'view.snapshotChunk') dropped.add(request.requestId);
      return previousSend(body);
    };
    window.__sodeckReceive = response => {
      if (dropped.has(response.requestId)) {
        window.__snapshotResponseDrops.push(response.requestId);
        localStorage.setItem('__snapshotResponseDrops', JSON.stringify(window.__snapshotResponseDrops));
        return;
      }
      return previousReceive?.(response);
    };
  });
  await app.page.evaluate(viewId => {
    // Tauri's invoke/runCallback properties are immutable; its callback map is
    // deliberately exposed. Intercept only this view's committed move result.
    const callbacks = window.__TAURI_INTERNALS__.callbacks, originalGet = callbacks.get;
    window.__lostMoveResponse = null;
    window.__lostMoveRequest = { method: 'workbench.move', viewId, observedAt: Date.now(), timeoutMs: 35000 };
    callbacks.get = function (id) {
      const callback = originalGet.call(this, id);
      if (!callback) return callback;
      return response => {
        if (response?.result?.viewId === viewId && response.result.committed === true) {
          callbacks.get = originalGet; callbacks.delete(id);
          window.__lostMoveRequest.requestId = response.requestId;
          window.__lostMoveResponse = { ...response, discardedAt: Date.now() };
          return;
        }
        return callback(response);
      };
    };
  }, first.viewId);
  const movesBefore = (await rpc(app.page, 'calls.list', { method: 'workbench.move', limit: 100 })).records.length;
  await app.page.getByRole('button', { name: first.title + ' 的视图操作', exact: true }).click();
  await app.page.getByRole('button', { name: '独立窗口', exact: true }).click();
  const lostResponse = await until(() => app.page.evaluate(() => window.__lostMoveResponse));
  assert.equal(lostResponse.result?.committed, true);
  evidence.lostResponses = { request: await app.page.evaluate(() => window.__lostMoveRequest), discarded: lostResponse };
  await until(() => source.isClosed());
  const target = await terminalPage(first.viewId);
  evidence.lostResponses.snapshotResponsesDiscarded = await target.evaluate(() => JSON.parse(localStorage.getItem('__snapshotResponseDrops') ?? '[]'));
  assert.ok(evidence.lostResponses.snapshotResponsesDiscarded.length > 0, 'At least one actual source snapshot acknowledgement must have been intercepted');
  evidence.lostResponses.after = await stable(first, evidence.before, 'detached', first.mountGeneration + 1);
  assert.equal((await pagesFor(first.viewId)).length, 1);
  await echo(target, 'committed-without-response;');
  evidence.lostResponses.notice = await until(() => app.page.locator('.app-error').innerText().then(text => text.includes('尚未取得操作结果') && text), 40000);
  evidence.lostResponses.timeoutObservedAt = Date.now();
  assert.ok(evidence.lostResponses.timeoutObservedAt - evidence.lostResponses.request.observedAt >= 34500, 'Keep the actual SDK 35-second deadline');
  const movesAfter = (await rpc(app.page, 'calls.list', { method: 'workbench.move', limit: 100 })).records;
  assert.equal(movesAfter.length, movesBefore + 1, 'Response loss must not trigger another move mutation');
  evidence.lostResponses.moveCalls = movesAfter;
  await stable(first, evidence.before, 'detached', first.mountGeneration + 1);
  assert.equal((await remoteEvents()).filter(event => event.event === 'shell-open').length, 2);
  checks.push('real shell SDK loses the successful move response and reaches its unchanged 35-second result-unknown timeout; queries show one committed target, unchanged sessions/resources and no duplicate move or shell; old surface is destroyed and target echoes');
  console.log(JSON.stringify({ phase: 'lost-response-passed', output }));

  await rpc(app.page, 'workbench.activate', { viewId: peer.viewId });
  await echo(await terminalPage(peer.viewId), 'peer-after-faults;');
  assert.deepEqual(app.errors, []);
  evidence.pluginDebugConnections = app.pluginDebugConnections;
  await stop(app); assert.equal(app.process.exitCode, 0); app = null;
  evidence.finalRemote = await until(async () => {
    const rows = await remoteEvents(), count = name => rows.filter(row => row.event === name).length;
    return count('connection-open') === count('connection-close') && count('shell-open') === count('shell-close') ? rows : false;
  });
  checks.push('peer session remains usable after all failures; normal App exit closes every real remote shell and connection');
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, scope: 'Windows native + loopback Paramiko; CDP page bridge fault injection', checks, evidence, normalExit: 0, untested: ['native WebView constructor completes after dispatcher deadline', 'full native control queue saturation', 'OS/global keyboard path'] }, null, 2));
  console.log(JSON.stringify({ passed: true, output, checks }));
} catch (error) {
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: false, checks, evidence, error: { message: error.message, code: error.code, stack: error.stack }, uiErrors: app?.errors, fixtureErrors: stderr }, null, 2));
  console.error(error); process.exitCode = 1;
} finally {
  if (source && !source.isClosed()) await source.evaluate(() => { localStorage.removeItem('__sodeckMoveFault'); window.__moveFaultRestore?.(); }).catch(() => {});
  await stop(app);
  fixture.kill();
  await until(() => fixture.exitCode !== null || fixture.signalCode !== null || fixtureError, 5000).catch(() => {});
  await writeFile(join(output, 'fixture-stderr.log'), stderr);
  for (const key of ['client-key', 'client-key-encrypted', 'client-openssh', 'client-openssh-encrypted', 'client-ed25519', 'client-ed25519-encrypted']) await rm(join(output, 'fixture', key), { force: true });
}
