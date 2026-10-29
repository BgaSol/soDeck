// WIN-SHELL-01: explicitly authorized Windows target plus isolated Linux fixture.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {launch,rpc,pluginRpc,pluginPages,until,stop,delay} from './harness.mjs';

const source=resolve(process.argv[2]),windowsId=process.argv[3],python=process.env.SODECK_TEST_PYTHON;
assert.ok(windowsId&&python,'Supply source package, authorized Windows server ID and SODECK_TEST_PYTHON');
const output=resolve('.qa/t027/windows-shell-native-'+Date.now());await mkdir(output,{recursive:true});
const {stdout}=await promisify(execFile)(python,['tests/desktop/core-snapshot.py',join(source,'data/core'),join(output,'data/core')],{windowsHide:true});
await writeFile(join(output,'config-copy.json'),stdout);
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy','monitor'].map(n=>n==='terminal'?resolve('.qa/t027/packages/terminal-windows-fix.sodeck-plugin'):join(source,'resources',n+'.sodeck-plugin')).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':12992,'sodeck.files':12993,'sodeck.network-proxy':12994,'sodeck.monitor':12995});
const checks=[];let app,failed;
const note=text=>{checks.push(text);console.log(text);};
const state=page=>pluginRpc(page,'view.initialize');
async function setting(enabled){
  const pageId='plugin:sodeck.terminal:general';const {snapshot:s}=await rpc(app.page,'settings.page.get',{pageId});
  await rpc(app.page,'settings.commit',{pageId,providerEpoch:s.providerEpoch,schemaVersion:s.schemaVersion,expectedRevision:s.settingsRevision,operationId:crypto.randomUUID(),changes:[{key:'shellIntegration',value:enabled}]});
}
async function terminal(serverId){return until(async()=>{for(const page of await pluginPages(app))if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.serverId)===serverId)return page;},30000);}
async function entered(page,command){await page.locator('.xterm-helper-textarea').pressSequentially(command);await page.locator('.xterm-helper-textarea').press('Enter');}
const live=s=>['running','active','connected'].includes(s.session?.status);
async function running(page){return until(async()=>{const s=await state(page);return live(s)?s:false;},30000);}
async function cwd(page,path){
  const scope=await page.evaluate(()=>({workspaceId:window.__SODECK_CONTEXT__.workspaceId,workspaceGeneration:window.__SODECK_CONTEXT__.workspaceGeneration}));
  let last;
  try{await until(async()=>{const {value}=await pluginRpc(page,'state.get',{contract:'workspace.directory.v1',target:scope});last=value;return value.status==='ready'&&Buffer.from(value.physicalPath?.bytesBase64??'','base64').toString()===path;},30000);}
  catch(error){await writeFile(join(output,'directory-failure.json'),JSON.stringify({path,last},null,2));throw error;}
}
try{
  app=await launch({executable:join(source,'sodeck-desktop.exe'),data:join(output,'data'),shellPort:12991,pluginPort:12992});
  assert.equal((await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.terminal').activeVersion,'0.13.1');
  const windows=(await rpc(app.page,'servers.routes.list')).servers.find(s=>s.id===windowsId);
  assert.equal(windows.host,'192.168.1.104');assert.equal(windows.accessPath.steps.length,0);
  await setting(true);await rpc(app.page,'workspaces.open',{serverId:windowsId});
  const win=await terminal(windowsId);const initial=await running(win);
  assert.equal(initial.shellIntegration,true);assert.equal(initial.session.integrationAvailable,false);
  const checkbox=win.getByRole('checkbox',{name:'Shell 目录集成',exact:true});
  await until(async()=>await checkbox.isDisabled());assert.equal(await checkbox.isChecked(),false);
  await until(async()=>/>\s*$/.test(await win.locator('.xterm-rows').innerText()));
  await entered(win,'echo SODECK_WINDOWS_SHELL_OK');
  await until(async()=>/(?:^|\n)SODECK_WINDOWS_SHELL_OK\s*(?:\n|$)/.test(await win.locator('.xterm-rows').innerText()));
  await delay(1500);assert.ok(live(await state(win)));
  assert.doesNotMatch(await win.locator('.xterm-rows').innerText(),/此时不应有|unexpected at this time|远程 shell 已结束/i);
  await win.screenshot({path:join(output,'windows-normal-shell.png')});
  note('Actual Windows .104: normal Shell stays running, echo works, session integration disabled while global preference remains true');
  const viewId=await win.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
  await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.reconnect',viewId,operationId:crypto.randomUUID()});
  await until(async()=>{const s=await state(win);return live(s)&&s.session.ptyId!==initial.session.ptyId;});
  await until(async()=>{const text=await win.locator('.xterm-rows').innerText();return !text.includes('SODECK_WINDOWS_SHELL_OK')&&/>\s*$/.test(text);});
  await entered(win,'echo SODECK_WINDOWS_RECONNECT_OK');
  await until(async()=>/(?:^|\n)SODECK_WINDOWS_RECONNECT_OK\s*(?:\n|$)/.test(await win.locator('.xterm-rows').innerText()));
  assert.equal((await state(win)).session.integrationAvailable,false);
  assert.doesNotMatch(await win.locator('body').innerText(),/终端已经关闭|远端 shell 已结束/);
  note('Actual Windows reconnect: new normal PTY, interactive echo remains available');

  // A/B deliberately forbid session channels. Use the dedicated guest's SSH
  // management endpoint without weakening those mixed-route test fixtures.
  const entry={host:'127.0.0.1',port:22373,username:'root',key:resolve('.qa/t008/linux/test-key')};
  assert.equal(entry.host,'127.0.0.1');
  const linux=await rpc(app.page,'servers.routes.create',{name:'WIN-SHELL-01 Linux fixture',host:entry.host,port:entry.port,username:entry.username,authType:'key',accessPath:{steps:[]}});
  const prepared=await rpc(app.page,'credentials.configuration.prepare',{target:{kind:'ssh',id:linux.id},expectedOwnerRevision:linux.revision,expectedDependencies:[]});
  await rpc(app.page,'credentials.configuration.set',{operationId:crypto.randomUUID(),contextToken:prepared.contextToken,privateKeyPath:entry.key,remember:true});
  await rpc(app.page,'workspaces.open',{serverId:linux.id});
  await until(async()=>{const dialog=app.page.getByRole('dialog',{name:'确认身份与登录',exact:true});if(await dialog.count())await dialog.getByRole('button',{name:'信任并连接',exact:true}).click();return (await rpc(app.page,'resources.list')).ptys.filter(p=>p.state==='active').length===2;},45000);
  const lin=await terminal(linux.id),integrated=await running(lin);
  assert.equal(integrated.session.integrationAvailable,true);await cwd(lin,'/root');
  await entered(lin,'cd /tmp');await cwd(lin,'/tmp');
  await lin.screenshot({path:join(output,'linux-integrated-shell.png')});
  note('Real Linux/OpenSSH Bash: integration enabled, initial directory and cd /tmp reported through core state');
  await setting(false);assert.equal((await state(lin)).session.ptyId,integrated.session.ptyId);
  await entered(lin,'cd /');await cwd(lin,'/');
  await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.reconnect',viewId:await lin.evaluate(()=>window.__SODECK_CONTEXT__.viewId),operationId:crypto.randomUUID()});
  await until(async()=>{const s=await state(lin);return live(s)&&s.session.ptyId!==integrated.session.ptyId;});
  await until(async()=>{const text=await lin.locator('.xterm-rows').innerText();return !text.includes('cd /tmp')&&/[$#]\s*$/.test(text);});
  await entered(lin,"printf 'SODECK_ORDINARY=%s\\n' \"${PROMPT_COMMAND-}\"");
  await until(async()=>/SODECK_ORDINARY=\s*\n/.test(await lin.locator('.xterm-rows').innerText()));
  assert.equal((await state(lin)).shellIntegration,false);
  note('Global preference false: existing Linux PTY keeps reporting until explicit reconnect, new ordinary Shell has no integration prompt hook');
  assert.equal(app.errors.length,0);
}catch(error){failed=error;console.error(error);if(app)await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),resources:await rpc(app.page,'resources.list').catch(()=>null),sessions:await Promise.all((await pluginPages(app).catch(()=>[])).map(async p=>({session:await state(p).catch(()=>null),text:await p.locator('body').innerText().catch(()=>null)})))},null,2));}
finally{
  await stop(app);const result={passed:!failed,output,checks,shutdown:app?.shutdown};
  await writeFile(join(output,'results.json'),JSON.stringify(result,null,2));await writeFile('.qa/t027/windows-shell-native-latest.json',JSON.stringify(result,null,2));
}
if(failed)process.exitCode=1;
