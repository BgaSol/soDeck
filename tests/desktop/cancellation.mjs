import {openTerminal,invokePlugin} from './workbench-helpers.mjs';
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { join } from 'node:path';
import { root, launch, rpc, until, stop, pluginPages, delay } from './harness.mjs';

// Actual desktop, worker and SSH boundaries; CDP targets these isolated pages.
// No system-wide keyboard/mouse events or native foreground activation are used.
const output = join(root, '.qa/cancellation-native', String(Date.now()));
await mkdir(output, { recursive: true });
const checks = [], metrics = {}, sockets = new Set();
let acceptedSockets = 0, app, fixture, fixtureOutput = '', fixtureErrors = '';
const hanging = createServer(socket => {
  acceptedSockets++;
  sockets.add(socket);
  socket.on('data', () => {}); // Read the client banner but never send a server banner.
  socket.on('error', () => {});
  socket.on('close', () => sockets.delete(socket));
});
await new Promise((resolve, reject) => { hanging.once('error', reject); hanging.listen(0, '127.0.0.1', resolve); });
const hashes = {};
const hash = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const settle = promise => promise.then(result => ({ result }), error => ({ error }));
const terminalViews = snapshot => snapshot.views.filter(view => view.viewType === 'terminal.session');
async function noResidualResources() {
  return until(async () => {
    const resources = await rpc(app.page, 'resources.list');
    const snapshot = await rpc(app.page, 'workbench.snapshot');
    return resources.ptys.length === 0 && resources.leases.length === 0 && resources.connections.length === 0
      && snapshot.pendingOpens.length === 0 && terminalViews(snapshot).length === 0;
  }, 5000);
}
async function fixtureEvents() {
  return (await readFile(join(output, 'fixture/events.jsonl'), 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
}
async function terminalPage(viewId) {
  const page = await until(async () => {
    for (const page of await pluginPages(app)) {
      try { if (await page.evaluate(() => window.__SODECK_CONTEXT__?.viewId) === viewId) return page; } catch { /* replaced surface */ }
    }
  });
  page.on('pageerror', error => app.errors.push(error.message));
  await page.waitForSelector('.xterm');
  await until(() => page.locator('.xterm-rows').innerText().then(text => text.includes('fixture-ready')));
  return page;
}
try {
  hashes.core = await hash(join(root, 'target/debug/sodeck-desktop.exe'));
  hashes.terminal = await hash(join(root, 'resources/terminal.sodeck-plugin'));
  app = await launch({ data: join(output, 'app'), shellPort: 9373, pluginPort: 9374 });
  const server = await rpc(app.page, 'servers.create', {
    name: '本机取消夹具', host: '127.0.0.1', port: hanging.address().port, username: 'test-user', authType: 'password',
  });
  const operationId = randomUUID();
  const opening = settle(openTerminal(app.page,server.id,operationId));
  await until(async () => sockets.size === 1 && (await rpc(app.page, 'workbench.snapshot')).pendingOpens.some(open => open.operationId === operationId));
  const cancel = app.page.getByRole('button', { name: `取消连接到 ${server.name}`, exact: true });
  await cancel.waitFor({ state: 'visible' });
  await app.page.screenshot({ path: join(output, 'pending-connection.png') });
  const started = performance.now();
  await cancel.click();
  const canceled = await opening;
  metrics.cancelMs = Math.round(performance.now() - started);
  assert.equal(canceled.error?.code, 'CANCELED');
  assert.ok(metrics.cancelMs < 1000, `Native cancellation took ${metrics.cancelMs} ms`);
  await noResidualResources();
  await until(() => sockets.size === 0, 3000);
  assert.deepEqual(await invokePlugin(app,'sessions.list'), []);
  await until(() => cancel.count().then(count => count === 0));
  await app.page.screenshot({ path: join(output, 'canceled-connection.png') });
  checks.push('Actual shell cancel button ends a real no-banner SSH handshake within 1 second; worker sessions, views, PTYs, leases, connections and TCP socket are cleared');

  const beforeSockets = acceptedSockets;
  const beforeId = randomUUID();
  assert.equal((await rpc(app.page, 'workbench.operation.cancel', { operationId: beforeId })).canceled, true);
  const preCanceled = await settle(openTerminal(app.page,server.id,beforeId));
  assert.equal(preCanceled.error?.code, 'CANCELED');
  await noResidualResources();
  await delay(200);
  assert.equal(acceptedSockets, beforeSockets);
  checks.push('Cancellation received before open prevents the operation from creating a TCP socket or view');

  const cancellationRecords = await rpc(app.page, 'calls.list', { method: 'operations.cancel', limit: 100 });
  const acquireRecords = await rpc(app.page, 'calls.list', { method: 'connections.acquire', limit: 100 });
  assert.ok(cancellationRecords.records.some(record => record.caller.pluginId === 'sodeck.terminal' && record.status === 'succeeded'));
  assert.ok(acquireRecords.records.some(record => record.caller.pluginId === 'sodeck.terminal' && record.errorCode === 'CANCELED'));
  await writeFile(join(output, 'cancellation-call-records.json'), JSON.stringify({ cancellationRecords, acquireRecords }, null, 2));
  checks.push('Core records attribute cancellation and the canceled acquire to the actual terminal worker instance');

  const python = process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
  fixture = spawn(python, ['-u', join(root, 'tests/core/ssh_fixture.py'), join(output, 'fixture')], {
    cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, PYTHONPATH: join(root, '.local/python') },
  });
  fixture.stdout.on('data', chunk => fixtureOutput += chunk);
  fixture.stderr.on('data', chunk => fixtureErrors += chunk);
  await until(() => { if (fixture.exitCode !== null) throw new Error(fixtureErrors || 'SSH fixture exited'); return fixtureOutput.includes('\n'); });
  const config = JSON.parse(fixtureOutput.split('\n')[0]);
  const real = await rpc(app.page, 'servers.create', {
    name: '取消后可用会话', host: '127.0.0.1', port: config.port, username: 'test-user', authType: 'password',
  });
  const challenge = await settle(openTerminal(app.page,real.id,randomUUID()));
  assert.equal(challenge.error?.code, 'HOST_KEY_REQUIRED');
  await rpc(app.page, 'hosts.trust', { serverId: real.id, ...challenge.error.details });
  await rpc(app.page, 'auth.set', { serverId: real.id, password: 'isolated-test-password', remember: false });
  const completedId = randomUUID();
  const view = await openTerminal(app.page,real.id,completedId);
  await rpc(app.page, 'views.activate', { viewId: view.viewId });
  const terminal = await terminalPage(view.viewId);
  const before = await rpc(app.page, 'resources.list');
  const lateCancel = await rpc(app.page, 'workbench.operation.cancel', { operationId: completedId });
  assert.equal(lateCancel.canceled, false);
  assert.equal(lateCancel.viewId, view.viewId);
  const after = await rpc(app.page, 'resources.list');
  for (const key of ['ptys', 'leases', 'connections']) {
    assert.deepEqual(after[key].map(item => item.id).sort(), before[key].map(item => item.id).sort());
  }
  assert.ok(terminalViews(await rpc(app.page, 'workbench.snapshot')).some(item => item.viewId === view.viewId));
  await terminal.locator('.xterm-helper-textarea').focus();
  await terminal.keyboard.type('completed-operation-still-alive');
  await until(() => terminal.locator('.xterm-rows').innerText().then(text => text.replaceAll('\n', '').includes('completed-operation-still-alive')));
  await terminal.screenshot({ path: join(output, 'completed-operation-preserved.png') });
  checks.push('Canceling an already completed open returns canceled:false and the original viewId; original SSH/PTY identities and native terminal echo remain usable');
  assert.deepEqual(app.errors, []);
  await stop(app);
  assert.equal(app.process.exitCode, 0, 'Normal app.exit is required; forced termination does not pass');
  app = undefined;
  await until(async () => {
    const events = await fixtureEvents();
    return events.filter(event => event.event === 'connection-open').length === events.filter(event => event.event === 'connection-close').length
      && events.filter(event => event.event === 'shell-open').length === events.filter(event => event.event === 'shell-close').length;
  }, 5000);
  assert.equal(sockets.size, 0);
  checks.push('Normal app.exit returns code 0 and closes the real SSH channel and socket');
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, scope: 'Windows native + loopback SSH protocol; not Linux/OpenSSH', checks, metrics, hashes }, null, 2));
  console.log(JSON.stringify({ passed: true, output, checks, metrics, hashes }));
} catch (error) {
  const diagnostic = {};
  if (app) {
    for (const method of ['workbench.snapshot', 'resources.list', 'calls.list']) {
      try { diagnostic[method] = await rpc(app.page, method); } catch (failure) { diagnostic[method] = { error: failure.message }; }
    }
    try { await app.page.screenshot({ path: join(output, 'failure.png'), timeout: 2000 }); } catch { /* failed app surface */ }
  }
  await writeFile(join(output, 'diagnostic.json'), JSON.stringify(diagnostic, null, 2));
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: false, checks, metrics, hashes, error: { message: error.message, code: error.code, stack: error.stack }, uiErrors: app?.errors, fixtureErrors }, null, 2));
  console.error(error);
  process.exitCode = 1;
} finally {
  await stop(app);
  for (const socket of sockets) socket.destroy();
  await new Promise(resolve => hanging.close(resolve));
  if (fixture && fixture.exitCode === null) {
    const exited = new Promise(resolve => fixture.once('exit', resolve));
    fixture.kill();
    await exited;
  }
  await writeFile(join(output, 'fixture-stderr.log'), fixtureErrors);
  for (const key of ['client-key', 'client-key-encrypted', 'client-openssh', 'client-openssh-encrypted', 'client-ed25519', 'client-ed25519-encrypted']) {
    await rm(join(output, 'fixture', key), { force: true });
  }
}
