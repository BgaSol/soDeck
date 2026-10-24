import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { root, launch, rpc, until, stop, pluginPages, pluginRpc } from './harness.mjs';

const output = join(root, '.qa/move-close', String(Date.now()));
await mkdir(output, { recursive: true });
console.log(JSON.stringify({ phase: 'starting', output }));
const python = process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture = spawn(python, ['-u', join(root, 'tests/core/ssh_fixture.py'), join(output, 'fixture')], {
  cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, PYTHONPATH: join(root, '.local/python') },
});
let stdout = '', stderr = '', fixtureError, app, source, moveTask;
const checks = [], evidence = {};
fixture.stdout.on('data', chunk => stdout += chunk);
fixture.stderr.on('data', chunk => stderr += chunk);
fixture.on('error', error => fixtureError = error);
const invoke = (command, args = {}) => rpc(app.page, 'plugin.invoke', { command, args });
const identity = resources => Object.fromEntries(['connections', 'leases', 'ptys'].map(kind => [kind, resources[kind].map(item => item.id).sort()]));
async function terminalPage(viewId) {
  return until(async () => {
    for (const page of await pluginPages(app)) {
      if (await page.evaluate(() => window.__SODECK_CONTEXT__?.viewId) === viewId && (await page.locator('.xterm-rows').innerText()).includes('fixture-ready')) return page;
    }
  });
}
async function echo(page, view, inputSeq, marker) {
  await pluginRpc(page, 'pty.write', { ptyId: view.ptyId, generation: view.generation, inputSeq, bytes: Buffer.from(marker).toString('base64') });
  await until(() => page.locator('.xterm-rows').innerText().then(text => text.replace(/\s/g, '').includes(marker)));
}
async function signal(name, timeout) {
  return until(async () => {
    try { return JSON.parse(await readFile(join(output, name), 'utf8')); }
    catch (error) { if (error.code === 'ENOENT' || error instanceof SyntaxError) return false; throw error; }
  }, timeout);
}
async function events() {
  return (await readFile(join(output, 'fixture/events.jsonl'), 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
}
async function waitRemote(connections, shells) {
  return until(async () => {
    const rows = await events();
    const count = name => rows.filter(row => row.event === name).length;
    return count('connection-open') - count('connection-close') === connections && count('shell-open') - count('shell-close') === shells ? rows : false;
  });
}

try {
  const hashes = {};
  for (const path of ['target/debug/sodeck-desktop.exe', 'resources/terminal.sodeck-plugin']) hashes[path] = createHash('sha256').update(await readFile(join(root, path))).digest('hex');
  evidence.artifacts = { hashes, node: process.version, platform: process.platform };
  await until(() => {
    if (fixtureError) throw fixtureError;
    if (fixture.exitCode !== null) throw Error('Fixture exited: ' + stderr);
    return stdout.includes('\n');
  });
  const { port } = JSON.parse(stdout.split('\n')[0]);
  app = await launch({ data: join(output, 'app'), shellPort: 9397, pluginPort: 9398 });
  const server = await rpc(app.page, 'servers.create', { name: '并发关闭隔离验证', host: '127.0.0.1', port, username: 'test-user', authType: 'password' });
  let challenge;
  await assert.rejects(rpc(app.page, 'workbench.openTerminal', { serverId: server.id }), error => {
    assert.equal(error.code, 'HOST_KEY_REQUIRED'); challenge = error.details; return true;
  });
  await rpc(app.page, 'hosts.trust', { serverId: server.id, ...challenge });
  await rpc(app.page, 'auth.set', { serverId: server.id, password: 'isolated-test-password', remember: false });
  const first = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  const second = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  await terminalPage(second.viewId);
  await rpc(app.page, 'workbench.activate', { viewId: first.viewId });
  await terminalPage(first.viewId);
  evidence.before = { resources: await rpc(app.page, 'resources.list'), sessions: await invoke('sessions.list') };
  assert.equal(evidence.before.resources.ptys.length, 2);
  assert.equal(evidence.before.resources.leases.length, 2);
  assert.equal(evidence.before.resources.connections.length, 1);
  await waitRemote(1, 2);
  let inputSeq = 0;
  for (let round = 1; round <= 10; round++) {
    for (const placement of ['detached', 'center']) {
      const moved = await rpc(app.page, 'workbench.move', { viewId: first.viewId, placement });
      assert.equal(moved.committed, true); assert.equal(moved.persisted, true);
      assert.equal(moved.sessionId, first.sessionId); assert.equal(moved.ptyId, first.ptyId);
      assert.deepEqual(identity(await rpc(app.page, 'resources.list')), identity(evidence.before.resources));
      await echo(await terminalPage(first.viewId), first, ++inputSeq, 'r' + round + placement + ';');
    }
  }
  evidence.roundTrips = 10;
  checks.push('ten detach/return round trips preserve session/PTY/lease/connection identities and echo a unique marker from every mounted surface');
  await rpc(app.page, 'workbench.move', { viewId: first.viewId, placement: 'detached' });
  source = await terminalPage(first.viewId);
  await source.evaluate(() => {
    const previous = window.__sodeckNotify;
    window.__moveCloseHeld = null;
    window.__sodeckNotify = notification => {
      if (notification.type === 'view.prepareMove') { window.__moveCloseHeld = notification; return; }
      return previous?.(notification);
    };
    window.__releaseMoveClose = () => {
      window.__sodeckNotify = previous;
      const notification = window.__moveCloseHeld;
      window.__moveCloseHeld = null;
      if (notification) previous?.(notification);
      return Boolean(notification);
    };
  });
  const ready = { phase: 'ready', output, processId: app.process.pid, viewId: first.viewId, expectedTitle: server.name + ' · ' + first.title + ' — SoDeck', startPath: join(output, 'start.json'), heldPath: join(output, 'move-held.json'), releasePath: join(output, 'release.json') };
  await writeFile(join(output, 'ready.json'), JSON.stringify(ready, null, 2));
  console.log(JSON.stringify(ready));
  await signal('start.json', 180000);
  evidence.moveStarted = Date.now();
  moveTask = rpc(app.page, 'workbench.move', { viewId: first.viewId, placement: 'center' }).then(result => ({ result }), error => ({ error: { code: error.code, message: error.message } }));
  const held = await until(() => source.evaluate(() => window.__moveCloseHeld), 3000);
  evidence.held = { heldAt: Date.now(), operationId: held.payload.operationId, viewId: first.viewId, deadlineMs: 10000 };
  await writeFile(join(output, 'move-held.json'), JSON.stringify(evidence.held, null, 2));
  console.log(JSON.stringify({ phase: 'move-held', output, ...evidence.held }));
  evidence.release = await signal('release.json', 7000);
  evidence.releasedAt = Date.now();
  assert.equal(await source.evaluate(() => window.__releaseMoveClose()), true);
  const movement = await moveTask;
  assert.equal(movement.error, undefined, 'Held move must complete within its unchanged 10-second deadline: ' + JSON.stringify(movement.error));
  evidence.moved = movement.result;
  assert.equal(movement.result.committed, true); assert.equal(movement.result.placement, 'center');
  await until(async () => {
    const layout = await rpc(app.page, 'workbench.snapshot');
    const resources = await rpc(app.page, 'resources.list');
    return !layout.views.some(view => view.viewId === first.viewId) && resources.ptys.length === 1 && resources.leases.length === 1 && resources.connections.length === 1;
  });
  const peerSession = evidence.before.sessions.find(session => session.sessionId === second.sessionId);
  evidence.after = { resources: await rpc(app.page, 'resources.list'), sessions: await invoke('sessions.list'), workbench: await rpc(app.page, 'workbench.snapshot') };
  assert.deepEqual(identity(evidence.after.resources), { connections: [peerSession.connectionId], leases: [peerSession.leaseId], ptys: [second.ptyId] });
  assert.deepEqual(evidence.after.sessions.map(session => session.sessionId), [second.sessionId]);
  const closeCalls = (await rpc(app.page, 'calls.list', { method: 'pty.close', limit: 100 })).records.filter(record => record.summary.generation === first.generation);
  assert.equal(closeCalls.length, 1, 'The moved PTY must be closed exactly once');
  assert.equal(closeCalls[0].status, 'succeeded'); evidence.closeCalls = closeCalls;
  const remote = await waitRemote(1, 1);
  assert.equal(remote.filter(event => event.event === 'shell-close' && event.channel === 0).length, 1);
  await rpc(app.page, 'workbench.activate', { viewId: second.viewId });
  await echo(await terminalPage(second.viewId), second, 1, 'peer-survives-queued-close');
  checks.push('actual native close during held move waits for committed handoff, then closes the original view/session/PTY exactly once while its peer and shared connection stay usable');
  evidence.pluginDebugConnections = app.pluginDebugConnections;
  assert.deepEqual(app.errors, []);
  await stop(app); assert.equal(app.process.exitCode, 0); app = null;
  evidence.finalRemote = await waitRemote(0, 0);
  checks.push('normal App exit closes the remaining real remote channel and connection');
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, scope: 'Windows native + loopback Paramiko echo service', checks, evidence, normalExit: 0 }, null, 2));
  console.log(JSON.stringify({ passed: true, output, checks }));
} catch (error) {
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: false, checks, evidence, error: { message: error.message, code: error.code, stack: error.stack }, uiErrors: app?.errors, fixtureErrors: stderr }, null, 2));
  console.error(error); process.exitCode = 1;
} finally {
  if (source && !source.isClosed()) await source.evaluate(() => window.__releaseMoveClose?.()).catch(() => {});
  await stop(app);
  if (moveTask) await moveTask;
  fixture.kill();
  await until(() => fixture.exitCode !== null || fixture.signalCode !== null || fixtureError, 5000).catch(() => {});
  await writeFile(join(output, 'fixture-stderr.log'), stderr);
  for (const key of ['client-key', 'client-key-encrypted', 'client-openssh', 'client-openssh-encrypted', 'client-ed25519', 'client-ed25519-encrypted']) await rm(join(output, 'fixture', key), { force: true });
}
