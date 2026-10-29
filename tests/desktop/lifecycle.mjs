import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile, rm, rmdir } from 'node:fs/promises';
import { join } from 'node:path';
import { root, launch, rpc, until, stop, pluginPages, pluginRpc, nativeWindow } from './harness.mjs';

const output = join(root, '.qa/lifecycle', String(Date.now()));
const data = join(output, 'app');
const externalClose = process.env.SODECK_LIFECYCLE_EXTERNAL_CLOSE === '1';
await mkdir(output, { recursive: true });
console.log(JSON.stringify({ phase: 'starting', output, externalClose }));
const python = process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture = spawn(python, ['-u', join(root, 'tests/core/ssh_fixture.py'), join(output, 'fixture')], {
  cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, PYTHONPATH: join(root, '.local/python') },
});
let fixtureOutput = '', fixtureErrors = '', fixtureError, app, normalExits = 0;
const checks = [], snapshots = {}, uiErrors = [];
fixture.stdout.on('data', chunk => fixtureOutput += chunk);
fixture.stderr.on('data', chunk => fixtureErrors += chunk);
fixture.on('error', error => fixtureError = error);

const invoke = (command, args = {}) => rpc(app.page, 'plugin.invoke', { command, args });
const identity = resources => Object.fromEntries(['connections', 'leases', 'ptys'].map(kind => [kind, resources[kind].map(item => item.id).sort()]));
function counts(resources, connections, leases, ptys) {
  assert.equal(resources.connections.length, connections, 'SSH connection count');
  assert.equal(resources.leases.length, leases, 'Lease count');
  assert.equal(resources.ptys.length, ptys, 'PTY count');
}
async function snapshot(name) {
  const value = {
    resources: await rpc(app.page, 'resources.list'),
    workbench: await rpc(app.page, 'workbench.snapshot'),
    sessions: await invoke('sessions.list'),
    servers: (await rpc(app.page, 'servers.list')).servers,
  };
  snapshots[name] = value;
  await writeFile(join(output, name + '.json'), JSON.stringify(value, null, 2));
  return value;
}
async function terminalPage(viewId) {
  try { return await until(async () => {
    for (const page of await pluginPages(app)) {
      if (await page.evaluate(() => window.__SODECK_CONTEXT__?.viewId) !== viewId) continue;
      if (!(await page.locator('.xterm-rows').innerText()).includes('fixture-ready')) continue;
      return page;
    }
  }); } catch (error) {
    const diagnosis={viewId,connected:app.pluginBrowser?.isConnected(),connections:app.pluginDebugConnections,pages:[]};
    for(const page of app.pluginBrowser?.contexts().flatMap(context=>context.pages())??[]){
      try{diagnosis.pages.push(await page.evaluate(()=>({url:location.href,context:window.__SODECK_CONTEXT__,text:document.body.innerText})));}
      catch(failure){diagnosis.pages.push({url:page.url(),error:failure.message});}
    }
    try{diagnosis.workbench=await rpc(app.page,'workbench.snapshot');diagnosis.resources=await rpc(app.page,'resources.list');}catch{}
    await writeFile(join(output,'terminal-page-failure.json'),JSON.stringify(diagnosis,null,2));
    throw error;
  }
}
async function fixtureCounts() {
  const events = (await readFile(join(output, 'fixture/events.jsonl'), 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  const count = name => events.filter(event => event.event === name).length;
  return { connectionsOpened: count('connection-open'), connectionsClosed: count('connection-close'), shellsOpened: count('shell-open'), shellsClosed: count('shell-close') };
}
async function waitFixture(connections, shells) {
  return until(async () => {
    const current = await fixtureCounts();
    return current.connectionsOpened - current.connectionsClosed === connections && current.shellsOpened - current.shellsClosed === shells ? current : false;
  });
}
async function stopNormally() {
  uiErrors.push(...app.errors);
  snapshots['pluginDebugConnectionsRun'+(normalExits+1)]=app.pluginDebugConnections??[];
  await stop(app);
  assert.equal(app.process.exitCode, 0, 'app.exit must finish normally, without forced termination');
  assert.equal(app.process.signalCode, null);
  await writeFile(join(output, 'app-run-' + (++normalExits) + '.log'), app.output());
  app = null;
}
async function closeDetached(expectedTitle, viewId) {
  if (!externalClose) {
    const window = await until(async () => (await nativeWindow(app, 'list')).find(item => item.title === expectedTitle));
    await writeFile(join(output, 'native-window.json'), JSON.stringify(window, null, 2));
    await nativeWindow(app, 'close', { Title: window.title });
    return;
  }
  // Computer Use sessions own their desktop input. The driver only waits for a
  // real observation/close result and independently verifies lifecycle effects.
  const request = { processId: app.process.pid, viewId, expectedTitle, confirmationPath: join(output, 'close-confirmation.json') };
  await writeFile(join(output, 'close-request.json'), JSON.stringify(request, null, 2));
  console.log(JSON.stringify({ phase: 'awaiting-external-close', output, ...request }));
  const confirmation = await until(async () => {
    try { return JSON.parse(await readFile(request.confirmationPath, 'utf8')); }
    catch (error) { if (error.code === 'ENOENT' || error instanceof SyntaxError) return false; throw error; }
  }, 180000);
  assert.equal(confirmation.observedTitle, expectedTitle, 'The externally observed native title must include the server and renamed session');
  assert.equal(confirmation.closed, true, 'External close must be performed before confirming it');
}

try {
  const hashes = {};
  for (const path of ['target/debug/sodeck-desktop.exe', 'resources/terminal.sodeck-plugin']) hashes[path] = createHash('sha256').update(await readFile(join(root, path))).digest('hex');
  await writeFile(join(output, 'artifacts.json'), JSON.stringify({ hashes, node: process.version, platform: process.platform, externalClose }, null, 2));
  await until(() => {
    if (fixtureError) throw fixtureError;
    if (fixture.exitCode !== null) throw Error('SSH fixture exited: ' + fixtureErrors);
    return fixtureOutput.includes('\n');
  });
  const { port } = JSON.parse(fixtureOutput.split('\n')[0]);
  app = await launch({ data, shellPort: 9395, pluginPort: 9396 });
  const server = await rpc(app.page, 'servers.create', { name: '生命周期隔离验证', host: '127.0.0.1', port, username: 'test-user', authType: 'password' });
  await assert.rejects(rpc(app.page, 'workbench.openTerminal', { serverId: server.id }), error => {
    assert.equal(error.code, 'HOST_KEY_REQUIRED');
    snapshots.hostChallenge = error.details;
    return true;
  });
  await rpc(app.page, 'hosts.trust', { serverId: server.id, ...snapshots.hostChallenge });
  await rpc(app.page, 'auth.ssh.set', { serverId: server.id, password: 'isolated-test-password', remember: false });
  const first = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  const second = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  await terminalPage(second.viewId);
  await rpc(app.page, 'workbench.activate', { viewId: first.viewId });
  await terminalPage(first.viewId);
  const before = await snapshot('before');
  counts(before.resources, 1, 2, 2);
  assert.equal(new Set(before.resources.leases.map(lease => lease.connectionId)).size, 1);
  await waitFixture(1, 2);

  const list = before.workbench.views.find(view => view.viewType === 'terminal.sessions' && view.serverId === server.id);
  assert.ok(list, 'The workspace must contain its session list');
  await rpc(app.page, 'workbench.close', { viewId: list.viewId });
  const listClosed = await snapshot('list-closed');
  assert.deepEqual(identity(listClosed.resources), identity(before.resources));
  assert.deepEqual(listClosed.sessions.map(session => session.sessionId).sort(), before.sessions.map(session => session.sessionId).sort());
  assert.ok(!listClosed.workbench.views.some(view => view.viewId === list.viewId));
  await rpc(app.page, 'workbench.activateWorkspace', { serverId: server.id });
  await rpc(app.page, 'workbench.configure', { sessionsVisible: true });
  assert.equal((await rpc(app.page, 'workbench.snapshot')).views.filter(view => view.viewType === 'terminal.sessions' && view.serverId === server.id).length, 1);
  checks.push('closing and reopening the session list preserves both sessions, PTYs, leases and shared connection');

  const layoutBeforeFault = await rpc(app.page, 'workbench.snapshot');
  const diskBeforeFault = await readFile(join(data, 'workbench.json'), 'utf8');
  const pendingPath = join(data, 'workbench.pending');
  await mkdir(pendingPath);
  try {
    await assert.rejects(rpc(app.page, 'workbench.configure', { sessionsSide: 'right' }), error => error.code === 'STORAGE_ERROR');
    assert.deepEqual(await rpc(app.page, 'workbench.snapshot'), layoutBeforeFault);
    await assert.rejects(rpc(app.page, 'workbench.move', { viewId: first.viewId, placement: 'bottom' }), error => error.code === 'STORAGE_ERROR');
    assert.deepEqual(await rpc(app.page, 'workbench.snapshot'), layoutBeforeFault);
    const moved = await rpc(app.page, 'workbench.move', { viewId: first.viewId, placement: 'detached' });
    assert.equal(moved.committed, true);
    assert.equal(moved.persisted, false);
    assert.equal(moved.warning?.code, 'STORAGE_ERROR');
    assert.equal(moved.placement, 'detached');
    assert.deepEqual(identity(await rpc(app.page, 'resources.list')), identity(before.resources));
    assert.equal(await readFile(join(data, 'workbench.json'), 'utf8'), diskBeforeFault);
    await terminalPage(first.viewId);
    await until(() => app.page.locator('.app-error').innerText().then(text => text.includes('移动已完成') && text.includes('布局未保存')));
    snapshots.storageFault = { moved, layoutBeforeFault, after: await rpc(app.page, 'workbench.snapshot') };
    await writeFile(join(output, 'storage-fault.json'), JSON.stringify(snapshots.storageFault, null, 2));
    checks.push('layout storage failure preserves uncommitted configuration/same-window movement; committed native move returns explicit warning, stays usable and visibly reports unsaved layout');
  } finally {
    await rmdir(pendingPath);
  }
  const renamed = await invoke('sessions.rename', { sessionId: first.sessionId, title: '重命名后的终端' });
  assert.equal(JSON.parse(await readFile(join(data, 'workbench.json'), 'utf8')).views.find(view => view.viewId === first.viewId).placement, 'detached', 'A later successful save must persist the committed move');
  const firstSession = before.sessions.find(session => session.sessionId === first.sessionId);
  for (const key of ['sessionId', 'viewId', 'ptyId', 'leaseId', 'connectionId', 'generation', 'serverId', 'workspaceId']) assert.equal(renamed[key], firstSession[key], key + ' must survive rename');
  const afterRename = await snapshot('renamed');
  assert.equal(afterRename.workbench.views.find(view => view.viewId === first.viewId).title, '重命名后的终端');
  assert.equal(afterRename.sessions.find(session => session.sessionId === first.sessionId).title, '重命名后的终端');
  assert.deepEqual(identity(afterRename.resources), identity(before.resources));
  const detachedShell = await until(async () => {
    for (const page of app.browser.contexts().flatMap(context => context.pages())) {
      if (await page.evaluate(() => window.__SODECK_CONTEXT__?.detachedViewId) === first.viewId) return page;
    }
  });
  await until(() => detachedShell.locator('#tab-' + first.viewId).innerText().then(text => text.includes('重命名后的终端')));
  await closeDetached('生命周期隔离验证 · 重命名后的终端 — SoDeck', first.viewId);
  checks.push('actual worker rename preserves all session/resource identities and updates native Tab and observed window title while retaining server identity');

  await until(async () => {
    const resources = await rpc(app.page, 'resources.list');
    return resources.ptys.length === 1 && resources.leases.length === 1 && resources.connections.length === 1 && detachedShell.isClosed();
  });
  const closed = await snapshot('detached-closed');
  const peerSession = before.sessions.find(session => session.sessionId === second.sessionId);
  assert.deepEqual(identity(closed.resources), { connections: [peerSession.connectionId], leases: [peerSession.leaseId], ptys: [second.ptyId] });
  assert.ok(!closed.workbench.views.some(view => view.viewId === first.viewId));
  assert.deepEqual(closed.sessions.map(session => session.sessionId), [second.sessionId]);
  await rpc(app.page, 'workbench.activate', { viewId: second.viewId });
  const peer = await terminalPage(second.viewId);
  const marker = 'peer-survives-detached-close';
  // No user keystrokes have been sent to this PTY, so its first input is 1.
  // This tests the real mounted bridge/SSH/output path, not keyboard handling.
  await pluginRpc(peer, 'pty.write', { ptyId: second.ptyId, generation: second.generation, inputSeq: 1, bytes: Buffer.from(marker).toString('base64') });
  await until(() => peer.locator('.xterm-rows').innerText().then(text => text.includes(marker)));
  await waitFixture(1, 1);
  checks.push('actual detached close ends its native page, view, session, PTY and lease; original peer and shared connection remain usable');

  const deletionBefore = await snapshot('before-delete');
  await assert.rejects(rpc(app.page, 'servers.delete', { serverId: server.id, stopSessions: false }), error => error.code === 'RESOURCE_IN_USE');
  const refused = await snapshot('delete-refused');
  assert.deepEqual(identity(refused.resources), identity(deletionBefore.resources));
  assert.deepEqual(refused.workbench, deletionBefore.workbench);
  assert.deepEqual(refused.servers, deletionBefore.servers);
  assert.deepEqual(refused.sessions, deletionBefore.sessions);
  await rpc(app.page, 'servers.delete', { serverId: server.id, stopSessions: true });
  const after = await snapshot('deleted');
  counts(after.resources, 0, 0, 0);
  assert.equal(after.servers.length, 0);
  assert.equal(after.sessions.length, 0);
  assert.ok(!after.workbench.views.some(view => view.serverId === server.id));
  const persisted = JSON.parse(await readFile(join(data, 'workbench.json'), 'utf8'));
  assert.ok(!Object.hasOwn(persisted.workspaces, server.id));
  snapshots.fixtureAfterDelete = await waitFixture(0, 0);
  checks.push('server deletion refuses active resources without changing state; explicit deletion clears workspace, sessions and actual remote channels/connection');

  const exitServer = await rpc(app.page, 'servers.create', { name: '活动退出验证', host: '127.0.0.1', port, username: 'test-user', authType: 'password' });
  await rpc(app.page, 'auth.ssh.set', { serverId: exitServer.id, password: 'isolated-test-password', remember: false });
  const exitFirst = await rpc(app.page, 'workbench.openTerminal', { serverId: exitServer.id });
  const exitSecond = await rpc(app.page, 'workbench.openTerminal', { serverId: exitServer.id });
  await terminalPage(exitSecond.viewId);
  await rpc(app.page, 'workbench.activate', { viewId: exitFirst.viewId });
  await terminalPage(exitFirst.viewId);
  await rpc(app.page, 'workbench.move', { viewId: exitFirst.viewId, placement: 'detached' });
  const exitBefore = await snapshot('before-active-exit');
  counts(exitBefore.resources, 1, 2, 2);
  snapshots.fixtureBeforeExit = await waitFixture(1, 2);
  await stopNormally();
  snapshots.fixtureAfterExit = await waitFixture(0, 0);
  assert.equal(snapshots.fixtureAfterExit.shellsClosed - snapshots.fixtureBeforeExit.shellsClosed, 2);
  assert.equal(snapshots.fixtureAfterExit.connectionsClosed - snapshots.fixtureBeforeExit.connectionsClosed, 1);
  checks.push('app.exit with two active PTYs and a detached window exits normally and closes both real remote channels and their SSH connection');

  app = await launch({ data, shellPort: 9395, pluginPort: 9396 });
  const restarted = await snapshot('restarted');
  counts(restarted.resources, 0, 0, 0);
  assert.deepEqual(restarted.sessions, []);
  for (const original of [exitFirst, exitSecond]) {
    const view = restarted.workbench.views.find(item => item.viewId === original.viewId);
    assert.ok(view, 'Restart should retain a layout placeholder for the original view');
    assert.equal(view.status, 'ended');
    for (const key of ['sessionId', 'ptyId', 'generation']) assert.equal(view[key], undefined, 'Restart must clear ' + key);
  }
  assert.deepEqual(await fixtureCounts(), snapshots.fixtureAfterExit, 'Restart must not create an SSH connection or shell');
  await stopNormally();
  assert.deepEqual(await fixtureCounts(), snapshots.fixtureAfterExit);
  assert.deepEqual(uiErrors, []);
  checks.push('restart retains ended layout placeholders without session/PTY handles, worker sessions or SSH reconnection, then exits normally');
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, scope: 'Windows native + loopback Paramiko echo service; not Linux/OpenSSH', externalClose, checks, normalExits, hashes, snapshots, uiErrors }, null, 2));
  console.log(JSON.stringify({ passed: true, output, checks, normalExits }));
} catch (error) {
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: false, externalClose, checks, normalExits, snapshots, error: { code: error.code, message: error.message, stack: error.stack }, uiErrors: [...uiErrors, ...(app?.errors ?? [])], fixtureErrors }, null, 2));
  console.error(error);
  process.exitCode = 1;
} finally {
  await stop(app);
  fixture.kill();
  await until(() => fixture.exitCode !== null || fixture.signalCode !== null || fixtureError, 5000).catch(() => {});
  await writeFile(join(output, 'fixture-stderr.log'), fixtureErrors);
  for (const key of ['client-key', 'client-key-encrypted', 'client-openssh', 'client-openssh-encrypted', 'client-ed25519', 'client-ed25519-encrypted']) await rm(join(output, 'fixture', key), { force: true });
}
