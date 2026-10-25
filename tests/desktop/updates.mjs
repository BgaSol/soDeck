import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { cp, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { chromium } from 'playwright';
import { root, launch, rpc, until, stop, pluginPages, pluginRpc } from './harness.mjs';

// Uses actual signed terminal packages and the actual Tauri/QuickJS processes.
// The loopback SSH provider exercises real protocol/PTY bytes; it is not a Linux shell.
const output = join(root, '.qa/update-native', String(Date.now()));
const data = join(output, 'app');
await mkdir(output, { recursive: true });
const checks = [];
let normalStops = 0;
const hashes = {};
const hash = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const corePath = join(root, 'target/debug/sodeck-desktop.exe');
const factoryPath = join(output, 'terminal-0.2.0.sodeck-plugin');
const updatePath = join(output, 'terminal-0.2.1.sodeck-plugin');
const statusPath = join(root, '.qa/plugin-platform-status.txt');
const recordStatus = text => writeFile(statusPath, `${new Date().toISOString()} ${text}\nOutput: ${output}\n`);
const build = args => {
  const result = spawnSync(process.execPath, [join(root, 'scripts/pack-plugin.mjs'), ...args], {
    cwd: root, windowsHide: true, encoding: 'utf8', timeout: 120000,
  });
  if (result.status !== 0) throw new Error(`Plugin build failed: ${result.stderr}\n${result.stdout}`);
  return result.stdout;
};
const invoke = async (app, command, args = {}) => { const page=await until(async()=>{for(const page of await pluginPages(app)){try{if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewType)==='terminal.sessions')return page;}catch{}}}); return pluginRpc(page,'plugin.invoke',{command,args}); };
const openTerminal=async(app,serverId)=>{const operationId=crypto.randomUUID();try{return await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:serverId,operationId});}catch(error){error.operationId=operationId;throw error;}};
const versions = async app => {
  const items = await rpc(app.page, 'plugins.list');
  const terminal = items.find(item => item.pluginId === 'sodeck.terminal');
  assert.ok(terminal, 'Actual terminal package must be installed');
  return terminal;
};
const stopNormally = async app => {
  await stop(app);
  assert.equal(app.process.exitCode, 0, 'Update restart must use normal app.exit, not forced termination');
  await writeFile(join(output, `app-run-${++normalStops}.log`), app.output());
};
async function terminalPage(app, viewId) {
  const page = await until(async () => {
    for (const page of await pluginPages(app)) {
      try { if (await page.evaluate(() => window.__SODECK_CONTEXT__?.viewId) === viewId) return page; } catch { /* replaced surface */ }
    }
  }, 20000);
  page.on('pageerror', error => app.errors.push(error.message));
  await page.waitForSelector('.xterm');
  await until(() => page.locator('.xterm-rows').innerText().then(text => text.includes('fixture-ready')), 20000);
  return page;
}
async function echo(page, marker) {
  await page.locator('.xterm-helper-textarea').focus();
  // Start a fresh visual line so a marker cannot be split by xterm wrapping.
  await page.keyboard.type('\r\n');
  await page.keyboard.type(marker);
  await until(() => page.locator('.xterm-rows').innerText().then(text => text.replaceAll('\n','').includes(marker)), 15000);
}
async function expectFailure(action, code, contains) {
  let failure;
  try { await action(); } catch (error) { failure = error; }
  assert.ok(failure, `Expected ${code}`);
  assert.equal(failure.code, code);
  if (contains) assert.match(failure.message, contains);
  return failure;
}

// The shared harness intentionally rejects startupError. This one isolated
// launch expects a missing installed package, so retain the real shell to test
// its degraded behavior instead of treating the expected error as launch failure.
async function launchMissingPackage() {
  const child = spawn(corePath, [], { cwd: root, windowsHide: true, stdio: ['ignore','pipe','pipe'],
    env: { ...process.env, SODECK_DATA_DIR:data, SODECK_SHELL_DEBUG_PORT:'9353',
      SODECK_WEBVIEW_ARGS:'--remote-debugging-port=9354', SODECK_PLUGIN_PACKAGE:factoryPath,
      SODECK_PLUGIN_KEYS:join(root,'resources/development-public-key.json') },
  });
  let outputText='';child.stdout.on('data',chunk=>outputText+=chunk);child.stderr.on('data',chunk=>outputText+=chunk);
  let browser;
  try {
    await until(async()=>{if(child.exitCode!==null)throw new Error(`Missing-package App exited ${child.exitCode}: ${outputText}`);return fetch('http://127.0.0.1:9353/json/version').then(response=>response.ok);},30000);
    browser=await chromium.connectOverCDP('http://127.0.0.1:9353');
    const page=await until(()=>browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes('tauri.localhost')||page.url().includes('tauri://')));
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.waitForSelector('#root > *');
    const info=await rpc(page,'app.info');
    assert.ok(info.startupError,'Missing installed active package must expose a startup reason');
    return {process:child,browser,page,data,pluginPort:9354,errors,output:()=>outputText,startupError:info.startupError};
  } catch(error) {child.kill();await browser?.close().catch(()=>{});await writeFile(join(output,'missing-package-launch.log'),outputText);throw error;}
}

// Preserve ZIP validity so rejection proves content verification, not only CRC checking.
function tamperEntry(archive, name, mutate) {
  const bytes = Buffer.from(archive);
  let offset = 0, crc;
  const table = Array.from({ length: 256 }, (_, index) => {
    for (let bit = 0; bit < 8; bit++) index = index & 1 ? (index >>> 1) ^ 0xedb88320 : index >>> 1;
    return index >>> 0;
  });
  while (bytes.readUInt32LE(offset) === 0x04034b50) {
    const size = bytes.readUInt32LE(offset + 18), nameSize = bytes.readUInt16LE(offset + 26), extraSize = bytes.readUInt16LE(offset + 28);
    const entryName = bytes.toString('utf8', offset + 30, offset + 30 + nameSize);
    const start = offset + 30 + nameSize + extraSize;
    if (entryName === name) {
      assert.equal(bytes.readUInt16LE(offset + 8), 0, 'Test packer uses stored ZIP entries');
      mutate(bytes.subarray(start, start + size));
      let value = 0xffffffff;
      for (const byte of bytes.subarray(start, start + size)) value = table[(value ^ byte) & 255] ^ (value >>> 8);
      crc = (value ^ 0xffffffff) >>> 0;
      bytes.writeUInt32LE(crc, offset + 14);
    }
    offset = start + size;
  }
  assert.notEqual(crc, undefined, `${name} entry must be present`);
  while (bytes.readUInt32LE(offset) === 0x02014b50) {
    const nameSize = bytes.readUInt16LE(offset + 28), extraSize = bytes.readUInt16LE(offset + 30), commentSize = bytes.readUInt16LE(offset + 32);
    if (bytes.toString('utf8', offset + 46, offset + 46 + nameSize) === name) bytes.writeUInt32LE(crc, offset + 16);
    offset += 46 + nameSize + extraSize + commentSize;
  }
  return bytes;
}

const tamperWorker = archive => tamperEntry(archive, 'worker/main.js', bytes => { bytes[bytes.length - 1] ^= 1; });
const tamperSignature = archive => tamperEntry(archive, 'signature.json', bytes => {
  const text = bytes.toString('utf8');
  const match = /"signature"\s*:\s*"([A-Za-z0-9+/]+)/.exec(text);
  assert.ok(match, 'Signature field must be present');
  const index = match.index + match[0].length - match[1].length;
  bytes[index] = bytes[index] === 65 ? 66 : 65;
});

let app, fixture, removedActive;
let fixtureOutput = '', fixtureErrors = '';
try {
  await recordStatus('RUNNING native update test on ports 9353/9354. Do not rebuild core until it finishes.');
  hashes.coreBefore = await hash(corePath);
  await writeFile(join(output,'build-baseline.log'),build(['--plugin','terminal','--version','0.2.0','--out',factoryPath]));
  hashes.factoryBefore = await hash(factoryPath);
  await writeFile(join(output, 'build-update.log'), build(['--plugin', 'terminal', '--version', '0.2.1', '--out', updatePath]));
  assert.equal(await hash(factoryPath), hashes.factoryBefore, 'Building the update must not replace factory 0.2.0');
  hashes.update = await hash(updatePath);

  const faultSource = join(output, 'failing-terminal-source');
  await cp(join(root, 'plugins/terminal/dist'), faultSource, { recursive: true });
  const workerPath = join(faultSource, 'worker/main.js');
  await writeFile(workerPath, `${await readFile(workerPath, 'utf8')}\n// Test-only failure derived from the actual terminal package.\nglobalThis.sodeckPlugin.prepare = async () => {
    const result = await sodeck.call('servers.list', {});
    const server = result.servers.find(item => item.name === '更新隔离验证');
    if (!server) throw new Error('UPDATE_FIXTURE_SERVER_MISSING');
    for (const [method, params] of [['servers.delete',{serverId:server.id,stopSessions:true}],['resources.stop',{resourceId:'UPDATE_FIXTURE_RESOURCE'}]]) {
      let denied;
      try { await sodeck.call(method,params); } catch(error) { denied=error; }
      if (denied?.code !== 'INVALID_STATE') throw new Error('UPDATE_FIXTURE_SHARED_MUTATION_NOT_DENIED:' + method + ':' + (denied?.code ?? 'allowed'));
    }
    await sodeck.call('storage.set', {key:'fontSize',value:23});
    throw new Error('UPDATE_FIXTURE_INITIALIZATION_FAILED');
  };\n`);
  const faultyPath = join(output, 'terminal-0.2.2-initialization-failure.sodeck-plugin');
  await writeFile(join(output, 'build-fault.log'), build(['--source', faultSource, '--version', '0.2.2', '--out', faultyPath]));
  const incompatibleSource=join(output,'incompatible-terminal-source');
  await cp(join(root,'plugins/terminal/dist'),incompatibleSource,{recursive:true});
  const incompatibleManifestPath=join(incompatibleSource,'manifest.json');
  const incompatibleManifest=JSON.parse(await readFile(incompatibleManifestPath,'utf8'));
  incompatibleManifest.requires.core='>=99.0.0';
  await writeFile(incompatibleManifestPath,JSON.stringify(incompatibleManifest,null,2));
  const incompatiblePath=join(output,'terminal-0.2.3-incompatible.sodeck-plugin');
  await writeFile(join(output,'build-incompatible.log'),build(['--source',incompatibleSource,'--version','0.2.3','--out',incompatiblePath]));
  const tamperedPath = join(output, 'terminal-tampered.sodeck-plugin');
  await writeFile(tamperedPath, tamperWorker(await readFile(updatePath)));
  const wrongSignaturePath = join(output, 'terminal-wrong-signature.sodeck-plugin');
  await writeFile(wrongSignaturePath, tamperSignature(await readFile(updatePath)));

  const python = process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
  fixture = spawn(python, ['-u', join(root, 'tests/core/ssh_fixture.py'), join(output, 'fixture')], {
    cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, PYTHONPATH: join(root, '.local/python') },
  });
  fixture.stdout.on('data', chunk => fixtureOutput += chunk);
  fixture.stderr.on('data', chunk => fixtureErrors += chunk);
  await until(() => { if (fixture.exitCode !== null) throw new Error(`SSH fixture stopped: ${fixtureErrors}`); return fixtureOutput.includes('\n'); }, 15000);
  const config = JSON.parse(fixtureOutput.split('\n')[0]);
  const launchSameData = () => launch({ data, shellPort: 9353, pluginPort: 9354, packagePath:factoryPath });
  app = await launchSameData();
  assert.equal((await versions(app)).activeVersion, '0.2.0');
  const server = await rpc(app.page, 'servers.create', { name: '更新隔离验证', host: '127.0.0.1', port: config.port, username: 'test-user', authType: 'password' });
  await rpc(app.page,'workspaces.open',{serverId:server.id});
  await invoke(app,'settings.set',{fontSize:19});
  assert.equal((await invoke(app,'settings.get')).fontSize,19);
  const challenge = await expectFailure(() => openTerminal(app,server.id), 'HOST_KEY_REQUIRED');
  await rpc(app.page, 'hosts.trust', { serverId: server.id, ...challenge.details });
  await app.page.evaluate(()=>{const notify=window.__sodeckNotify;window.__sodeckNotify=n=>{if(n.type==='auth.required')window.__retryAuth=n.payload;return notify(n);};});
  await expectFailure(() => rpc(app.page,'commands.retry',{operationId:challenge.operationId}), 'AUTH_REQUIRED');
  const auth=await until(()=>app.page.evaluate(()=>window.__retryAuth));assert.equal(auth.server.id,server.id);assert.equal(auth.commandId,'terminal.new');
  await expectFailure(() => rpc(app.page,'commands.retry',{operationId:challenge.operationId}), 'CANCELED');
  await app.page.getByLabel('密码',{exact:true}).fill('isolated-test-password');
  await app.page.getByRole('button',{name:'连接',exact:true}).click();
  const oldView = await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewType==='terminal.session'&&v.status==='running'));
  assert.equal((await rpc(app.page,'resources.list')).ptys.length,1);
  checks.push('ST-16: commands.retry preserves host/auth command context, consumes each challenge once, and creates exactly one PTY');
  const oldTerminal = await terminalPage(app, oldView.viewId);
  assert.equal(await oldTerminal.getByRole('button', { name: '恢复默认字号' }).count(), 0);
  await echo(oldTerminal, 'before-plugin-update');
  await oldTerminal.screenshot({ path: join(output, 'terminal-0.2.0.png') });
  const before = await rpc(app.page, 'resources.list');

  await rpc(app.page, 'plugins.prepareUpdate', { path: updatePath });
  const pending = await versions(app);
  assert.equal(pending.activeVersion, '0.2.0'); assert.equal(pending.pendingVersion, '0.2.1');
  await expectFailure(() => invoke(app, 'settings.reset'), 'METHOD_NOT_FOUND');
  assert.equal(await oldTerminal.getByRole('button', { name: '恢复默认字号' }).count(), 0);
  await echo(oldTerminal, '-pending-still-running');
  const after = await rpc(app.page, 'resources.list');
  assert.deepEqual(after.ptys.map(item => item.id), before.ptys.map(item => item.id));
  assert.deepEqual(after.connections.map(item => item.id), before.connections.map(item => item.id));
  assert.equal(await hash(corePath), hashes.coreBefore);
  checks.push('UPD-01: pending 0.2.1 keeps actual 0.2.0 UI/worker, same real SSH PTY and core binary');

  await expectFailure(() => rpc(app.page, 'plugins.prepareUpdate', { path: tamperedPath }), 'PACKAGE_INVALID', /hash mismatch/i);
  assert.equal((await versions(app)).pendingVersion, '0.2.1');
  await echo(oldTerminal, '-tamper-rejected');
  checks.push('UPD-03 content subcase: CRC-valid changed worker is rejected by signed inventory hash; active and pending remain intact');
  await expectFailure(() => rpc(app.page, 'plugins.prepareUpdate', { path: wrongSignaturePath }), 'PACKAGE_INVALID', /signature/i);
  const wrongSignatureStatus = await versions(app);
  assert.equal(wrongSignatureStatus.activeVersion, '0.2.0'); assert.equal(wrongSignatureStatus.pendingVersion, '0.2.1');
  const afterWrongSignature = await rpc(app.page, 'resources.list');
  assert.deepEqual(afterWrongSignature.ptys.map(item => item.id), before.ptys.map(item => item.id));
  assert.deepEqual(afterWrongSignature.connections.map(item => item.id), before.connections.map(item => item.id));
  await echo(oldTerminal, '-wrong-signature-rejected');
  checks.push('UPD-03 signature subcase: CRC-valid package with unchanged trusted key ID/content/inventory and altered signature is rejected; active, pending and SSH resources remain intact');
  await expectFailure(()=>rpc(app.page,'plugins.prepareUpdate',{path:incompatiblePath}),'PACKAGE_INVALID',/incompatible core or API/i);
  const rejected=await versions(app);
  assert.equal(rejected.activeVersion,'0.2.0');assert.equal(rejected.pendingVersion,'0.2.1');
  await echo(oldTerminal,'-signed-incompatible-rejected');
  const afterIncompatible=await rpc(app.page,'resources.list');
  assert.deepEqual(afterIncompatible.ptys.map(item=>item.id),before.ptys.map(item=>item.id));
  assert.deepEqual(afterIncompatible.connections.map(item=>item.id),before.connections.map(item=>item.id));
  checks.push('UPD-03 compatibility subcase: correctly signed actual terminal candidate requiring core >=99 is rejected; prior active/pending and actual SSH PTY remain usable');
  assert.deepEqual(app.errors, []);
  await stopNormally(app); app = null;
  app = await launchSameData();
  const activated = await versions(app);
  assert.equal(activated.activeVersion, '0.2.1'); assert.equal(activated.pendingVersion, null);
  assert.equal((await invoke(app, 'settings.get')).fontSize, 19);
  assert.equal((await rpc(app.page, 'resources.list')).ptys.length, 0, 'Restart must not silently recreate remote shells');
  await rpc(app.page, 'auth.set', { serverId: server.id, password: 'isolated-test-password', remember: false });
  const newView = await openTerminal(app,server.id);
  const newTerminal = await terminalPage(app, newView.viewId);
  await newTerminal.getByRole('button', { name: '恢复默认字号' }).click();
  await until(async () => (await invoke(app, 'settings.get')).fontSize === 14);
  await echo(newTerminal, 'new-terminal-version-active');
  await newTerminal.screenshot({ path: join(output, 'terminal-0.2.1-reset.png') });
  checks.push('UPD-02: restart activates new UI and QuickJS worker, preserves settings, real reset button persists 14, and prior PTYs are ended');

  await invoke(app, 'settings.set', { fontSize: 17 });
  await rpc(app.page, 'plugins.prepareUpdate', { path: faultyPath });
  assert.equal((await versions(app)).pendingVersion, '0.2.2');
  await stopNormally(app); app = null;
  app = await launchSameData();
  const recovered = await versions(app);
  assert.equal(recovered.activeVersion, '0.2.1'); assert.equal(recovered.pendingVersion, null);
  assert.match(recovered.error, /UPDATE_FIXTURE_INITIALIZATION_FAILED/);
  assert.equal((await invoke(app, 'settings.get')).fontSize, 17, 'Failed candidate data-copy write must not overwrite active data');
  assert.equal((await rpc(app.page,'servers.get',{serverId:server.id})).name,'更新隔离验证','Candidate must not delete shared server data');
  const deniedRecords=[];
  for(const method of ['servers.delete','resources.stop']) {
    const {records}=await rpc(app.page,'calls.list',{method,pluginId:'sodeck.terminal',limit:100});
    const denied=records.find(record=>record.caller?.pluginVersion==='0.2.2'&&record.errorCode==='INVALID_STATE');
    assert.ok(denied,`Candidate ${method} must have an attributed INVALID_STATE audit record`);
    assert.equal(denied.caller.source,'plugin');assert.equal(denied.status,'failed');assert.ok(denied.ended);
    deniedRecords.push(denied);
  }
  assert.equal(deniedRecords[0].caller.instanceId,deniedRecords[1].caller.instanceId,'Phase denials must belong to the same candidate instance');
  await writeFile(join(output,'candidate-phase-denials.json'),JSON.stringify(deniedRecords,null,2));
  await invoke(app, 'settings.reset');
  assert.equal((await invoke(app, 'settings.get')).fontSize, 14);
  checks.push('UPD-04 initialization subcase: actual candidate cannot delete shared server or stop resources; both calls recorded as INVALID_STATE; copied-data write then failure falls back with font 17 and usable reset');
  await stopNormally(app); app = null;
  const activeDirectory=join(data,'plugins/packages/sodeck.terminal/0.2.1');
  removedActive={original:activeDirectory,backup:join(output,'temporarily-removed-active-0.2.1')};
  await rename(removedActive.original,removedActive.backup);
  app=await launchMissingPackage();
  await writeFile(join(output,'missing-package-reason.txt'),app.startupError);
  assert.equal((await rpc(app.page,'servers.get',{serverId:server.id})).name,'更新隔离验证');
  assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
  const unavailable=await expectFailure(()=>openTerminal(app,server.id),'PLUGIN_UNAVAILABLE');
  assert.ok(unavailable.message);
  await until(()=>app.page.locator('.surface-error').count().then(count=>count>0));
  for(const [button,title] of [['运行资源','运行资源'],['调用记录','核心调用记录']]) {
    await app.page.getByRole('button',{name:button,exact:true}).click();
    await app.page.getByRole('dialog',{name:title,exact:true}).waitFor();
    if(button==='运行资源')await app.page.getByText('当前没有运行的终端。',{exact:true}).waitFor();
    else await until(()=>app.page.locator('.call-table tbody tr').count().then(count=>count>0));
    await app.page.getByRole('button',{name:'关闭对话框',exact:true}).click();
  }
  // A user can see why a preserved terminal cannot open while its package is absent.
  const placeholder=app.page.locator('.surface-error').filter({hasText:'视图暂时不可用'}).first();
  await placeholder.getByRole('button',{name:'重新加载视图',exact:true}).click();
  await until(()=>placeholder.innerText().then(text=>text.includes('不存在')||text.includes('missing')||text.includes('No such')||text.includes('找不到')));
  await app.page.screenshot({path:join(output,'missing-package-shell.png')});
  assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
  assert.deepEqual(app.errors,[]);
  await stopNormally(app);app=null;
  await rename(removedActive.backup,removedActive.original);removedActive=null;
  app=await launchSameData();
  assert.equal((await versions(app)).activeVersion,'0.2.1');
  assert.equal((await invoke(app,'settings.get')).fontSize,14);
  assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
  assert.deepEqual(app.errors,[]);
  await stopNormally(app);app=null;
  checks.push('UPD-05 missing active package: real shell keeps server/resource/audit pages and unavailable-view placeholder usable, exposes failure, opens no SSH; restoring the exact package and restarting restores terminal worker/settings');
  app = await launchSameData();
  assert.equal((await versions(app)).activeVersion, '0.2.1');
  assert.equal((await invoke(app, 'settings.get')).fontSize, 14);
  assert.equal((await rpc(app.page, 'servers.list')).servers.length, 1);
  assert.equal((await rpc(app.page, 'resources.list')).ptys.length, 0);
  assert.deepEqual(app.errors, []);
  await stopNormally(app); app = null;
  hashes.coreAfter = await hash(corePath); hashes.factoryAfter = await hash(factoryPath);
  assert.equal(hashes.coreAfter, hashes.coreBefore); assert.equal(hashes.factoryAfter, hashes.factoryBefore);
  checks.push('Repeated restart preserves new settings and server profile without activation loop or hidden PTY');
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, scope: 'Windows native Tauri + actual signed terminal UI/QuickJS + loopback SSH protocol; not Linux/OpenSSH', hashes, checks, normalStops }, null, 2));
  await recordStatus(`PASSED native update integration: ${checks.length} checks. Core rebuild is now safe. Result: ${join(output, 'result.json')}`);
  console.log(JSON.stringify({ passed: true, output, hashes, checks }));
} catch (error) {
  if(app){try{const pages=await pluginPages(app);const diagnostics=[];for(const page of pages){diagnostics.push(await page.evaluate(()=>({context:window.__SODECK_CONTEXT__,text:document.body.innerText})));}await writeFile(join(output,'failed-pages.json'),JSON.stringify(diagnostics,null,2));}catch{}}
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: false, hashes, checks, error: { message: error.message, stack: error.stack, code: error.code }, uiErrors: app?.errors, fixtureErrors }, null, 2));
  await recordStatus(`FAILED native update integration: ${error.code ?? ''} ${error.message}. See result.json and process.log. Core rebuild is now safe after test cleanup.`);
  console.error(error); process.exitCode = 1;
} finally {
  await stop(app);
  if(removedActive)await rename(removedActive.backup,removedActive.original).catch(error=>console.error('Restore isolated test package failed:',error.message));
  fixture?.kill();
  await writeFile(join(output, 'fixture-stderr.log'), fixtureErrors);
  for (const key of ['client-key', 'client-key-encrypted', 'client-openssh', 'client-openssh-encrypted','client-ed25519','client-ed25519-encrypted']) await rm(join(output, 'fixture', key), { force: true });
}
