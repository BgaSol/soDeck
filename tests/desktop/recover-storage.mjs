import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, rmdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { root, launch, pluginPages, rpc, stop, until } from './harness.mjs';

const output = join(root, '.qa/recover-storage', String(Date.now()));
const data = join(output, 'app'), artifacts = join(output, 'artifacts');
const storageFault = join(data, 'workbench.pending');
await mkdir(artifacts, { recursive: true });
console.log(JSON.stringify({ phase: 'starting', output }));
const python = process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture = spawn(python, ['-u', join(root, 'tests/core/ssh_fixture.py'), join(output, 'fixture')], {
  cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, PYTHONPATH: join(root, '.local/python') },
});
let stdout = '', stderr = '', fixtureError, app, faultCreated = false;
const checks = [], evidence = {};
fixture.stdout.on('data', chunk => stdout += chunk);
fixture.stderr.on('data', chunk => stderr += chunk);
fixture.on('error', error => fixtureError = error);
const invoke = (command, args = {}) => rpc(app.page, 'plugin.invoke', { command, args });
const hash = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const identity = resources => Object.fromEntries(['connections', 'leases', 'ptys'].map(kind => [kind, resources[kind].map(item => ({ id: item.id, generation: item.generation })).sort((a, b) => a.id.localeCompare(b.id))]));
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
      if (await page.locator('.xterm-helper-textarea').count()) return page;
    }
  });
}
async function echo(page, marker) {
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type(marker);
  await until(() => page.locator('.xterm-rows').innerText().then(text => text.replace(/\s/g, '').includes(marker)));
}
async function snapshot() {
  return { workbench: await rpc(app.page, 'workbench.snapshot'), resources: await rpc(app.page, 'resources.list'), sessions: await invoke('sessions.list') };
}
async function remoteEvents() {
  return (await readFile(join(output, 'fixture/events.jsonl'), 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
}

try {
  const executable = join(artifacts, 'sodeck-desktop.exe'), packagePath = join(artifacts, 'terminal.sodeck-plugin'), keysPath = join(artifacts, 'development-public-key.json');
  const hashes = {};
  for (const [source, destination] of [['target/debug/sodeck-desktop.exe', executable], ['resources/terminal.sodeck-plugin', packagePath], ['resources/development-public-key.json', keysPath]]) {
    await copyFile(join(root, source), destination); hashes[source] = await hash(destination);
  }
  evidence.artifacts = { hashes, executable, packagePath, keysPath, node: process.version, platform: process.platform };
  await until(() => {
    if (fixtureError) throw fixtureError;
    if (fixture.exitCode !== null) throw Error('Fixture exited: ' + stderr);
    return stdout.includes('\n');
  });
  const { port } = JSON.parse(stdout.split('\n')[0]);
  app = await launch({ data, shellPort: 9415, pluginPort: 9416, executable, packagePath, keysPath });
  const server = await rpc(app.page, 'servers.create', { name: '恢复保存隔离验证', host: '127.0.0.1', port, username: 'test-user', authType: 'password' });
  let challenge;
  await assert.rejects(rpc(app.page, 'workbench.openTerminal', { serverId: server.id }), error => {
    assert.equal(error.code, 'HOST_KEY_REQUIRED'); challenge = error.details; return true;
  });
  await rpc(app.page, 'hosts.trust', { serverId: server.id, ...challenge });
  await rpc(app.page, 'auth.set', { serverId: server.id, password: 'isolated-test-password', remember: false });
  const first = await rpc(app.page, 'workbench.openTerminal', { serverId: server.id });
  const original = await terminalPage(first.viewId);
  await until(() => original.locator('.xterm-rows').innerText().then(text => text.includes('fixture-ready')));
  await echo(original, 'before-renderer-crash;');
  evidence.before = await snapshot();
  assert.equal(evidence.before.resources.ptys.length, 1); assert.equal(evidence.before.resources.leases.length, 1); assert.equal(evidence.before.resources.connections.length, 1);
  const beforeView = evidence.before.workbench.views.find(view => view.viewId === first.viewId);
  const crash = await original.context().newCDPSession(original);
  evidence.crash = { method: 'Page.crash', targetUrl: original.url(), startedAt: Date.now() };
  await crash.send('Page.crash').catch(error => { evidence.crash.cdpError = error.message; });
  await crash.detach().catch(() => {});
  await until(async () => (await rpc(app.page, 'workbench.snapshot')).views.find(view => view.viewId === first.viewId)?.status === 'viewFailed');
  evidence.failed = await snapshot();
  assert.deepEqual(identity(evidence.failed.resources), identity(evidence.before.resources));
  assert.deepEqual(evidence.failed.sessions, evidence.before.sessions);
  const diskBeforeFault = await readFile(join(data, 'workbench.json'), 'utf8');
  await mkdir(storageFault); faultCreated = true;
  const recovery = await rpc(app.page, 'workbench.recover', { viewId: first.viewId });
  evidence.recovery = recovery;
  assert.equal(recovery.recovered, true); assert.equal(recovery.viewId, first.viewId);
  assert.equal(recovery.committed, true); assert.equal(recovery.persisted, false);
  assert.equal(recovery.warning?.code, 'STORAGE_ERROR');
  assert.match(recovery.warning.message, /视图已恢复/);
  evidence.warning = await until(() => app.page.locator('.app-error').allTextContents().then(texts => texts.find(text => text.includes('视图已恢复') && text.includes('保存失败'))));
  assert.equal(await app.page.locator('.app-error').isVisible(), true);
  const recovered = await terminalPage(first.viewId);
  evidence.screenGap = await until(() => recovered.locator('[role="alert"]').allTextContents().then(texts => texts.find(text => text.includes('终端显示已重建'))));
  await echo(recovered, 'recovered-with-unsaved-layout;');
  evidence.afterRecovery = await snapshot();
  const recoveredView = evidence.afterRecovery.workbench.views.find(view => view.viewId === first.viewId);
  assert.equal(recoveredView.status, 'running'); assert.equal(recoveredView.mountGeneration, beforeView.mountGeneration + 1);
  assert.equal(recoveredView.sessionId, beforeView.sessionId); assert.equal(recoveredView.ptyId, beforeView.ptyId); assert.equal(recoveredView.generation, beforeView.generation);
  assert.deepEqual(identity(evidence.afterRecovery.resources), identity(evidence.before.resources));
  assert.deepEqual(evidence.afterRecovery.sessions, evidence.before.sessions);
  assert.equal((await pagesFor(first.viewId)).length, 1, 'Recovery must leave exactly one new native surface');
  assert.equal(original.isClosed(), true, 'The crashed source must be removed');
  assert.equal(await readFile(join(data, 'workbench.json'), 'utf8'), diskBeforeFault, 'Failed save must not overwrite the previous disk layout');
  const recoverCalls = (await rpc(app.page, 'calls.list', { method: 'workbench.recover', limit: 100 })).records;
  assert.equal(recoverCalls.length, 1); assert.equal(recoverCalls[0].status, 'succeeded'); evidence.recoverCalls = recoverCalls;
  assert.equal((await remoteEvents()).filter(event => event.event === 'shell-open').length, 1, 'Recovering a renderer must not open another remote shell');
  await app.page.screenshot({ path: join(output, 'recovered-unsaved-shell.png') });
  await recovered.screenshot({ path: join(output, 'recovered-terminal.png') });
  checks.push('actual renderer crash preserves the SSH session; recovery with a real layout-write fault returns committed/persisted=false and a visible warning while exactly one recovered surface continues input on the original PTY');

  await rmdir(storageFault); faultCreated = false;
  await rpc(app.page, 'workbench.configure', { sessionsSide: 'right' });
  const saved = JSON.parse(await readFile(join(data, 'workbench.json'), 'utf8'));
  const savedView = saved.views.find(view => view.viewId === first.viewId);
  assert.equal(saved.workspaces[server.id].sessionsSide, 'right');
  assert.equal(savedView.mountGeneration, recoveredView.mountGeneration); assert.equal(savedView.status, 'running');
  assert.equal(savedView.ptyId, undefined); assert.equal(savedView.sessionId, undefined); assert.equal(savedView.generation, undefined);
  await echo(recovered, 'after-storage-restored;');
  evidence.afterSave = await snapshot(); evidence.savedWorkbench = saved;
  assert.deepEqual(identity(evidence.afterSave.resources), identity(evidence.before.resources));
  assert.deepEqual(evidence.afterSave.sessions, evidence.before.sessions);
  assert.equal((await rpc(app.page, 'calls.list', { method: 'workbench.recover', limit: 100 })).records.length, 1);
  checks.push('restoring storage and configuring layout persists the actual recovered mount without another recovery, session, PTY or saved runtime handle; original input still echoes');
  assert.deepEqual(app.errors, []);
  evidence.pluginDebugConnections = app.pluginDebugConnections;
  await stop(app); assert.equal(app.process.exitCode, 0); app = null;
  evidence.finalRemote = await until(async () => {
    const rows = await remoteEvents(), count = event => rows.filter(row => row.event === event).length;
    return count('connection-open') === count('connection-close') && count('shell-open') === count('shell-close') ? rows : false;
  });
  checks.push('normal App exit closes the remaining real remote shell and SSH connection');
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, scope: 'Windows native renderer crash + loopback SSH + real layout storage fault; CDP input', checks, evidence, normalExit: 0 }, null, 2));
  console.log(JSON.stringify({ passed: true, output, checks }));
} catch (error) {
  const diagnostic = {};
  if (app) for (const method of ['workbench.snapshot', 'resources.list']) {
    try { diagnostic[method] = await rpc(app.page, method); } catch (failure) { diagnostic[method] = { error: failure.message }; }
  }
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: false, checks, evidence, error: { code: error.code, message: error.message, stack: error.stack }, diagnostic, uiErrors: app?.errors, fixtureErrors: stderr }, null, 2));
  console.error(error); process.exitCode = 1;
} finally {
  if (faultCreated) await rmdir(storageFault).catch(() => {});
  await stop(app);
  fixture.kill();
  await until(() => fixture.exitCode !== null || fixture.signalCode !== null || fixtureError, 5000).catch(() => {});
  await writeFile(join(output, 'fixture-stderr.log'), stderr);
  for (const key of ['client-key', 'client-key-encrypted', 'client-openssh', 'client-openssh-encrypted', 'client-ed25519', 'client-ed25519-encrypted']) await rm(join(output, 'fixture', key), { force: true });
}
