import assert from 'node:assert/strict';
import {mkdir,readFile,readdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,stop} from './harness.mjs';

const output=join(root,'.qa/t-004/upgrade',String(Date.now()));await mkdir(output,{recursive:true});const data=join(output,'app');let app;const checks=[];
const legacyRoot=join(root,'.local/dev/SoDeck-v2');
try{
  app=await launch({data,shellPort:9463,pluginPort:9464,executable:join(legacyRoot,'sodeck-desktop.exe'),packagePath:join(legacyRoot,'resources/terminal.sodeck-plugin'),keysPath:join(legacyRoot,'resources/development-public-key.json')});
  assert.equal((await rpc(app.page,'plugins.list'))[0].activeVersion,'0.1.0');
  const server=await rpc(app.page,'servers.create',{name:'旧版升级副本',host:'127.0.0.1',port:22,username:'fixture',authType:'password'});
  await rpc(app.page,'workbench.activateWorkspace',{serverId:server.id});
  let incompatible;try{await rpc(app.page,'plugins.prepareUpdate',{path:join(root,'resources/terminal.sodeck-plugin')});}catch(e){incompatible=e;}
  assert.equal(incompatible?.code,'PACKAGE_INVALID');assert.match(incompatible.message,/incompatible core or API|unknown field `commands`/i);
  await writeFile(join(output,'old-host-rejection.txt'),incompatible.message);
  assert.equal((await rpc(app.page,'plugins.list'))[0].activeVersion,'0.1.0');
  checks.push('ST-17: actual preserved API-1 core rejects the new API-2 signed terminal and keeps its active old package');
  await stop(app);app=null;
  const original=await readFile(join(data,'workbench.json'));const old=JSON.parse(original);assert.equal(old.schemaVersion,undefined);
  await writeFile(join(output,'legacy-layout.json'),original);
  app=await launch({data,shellPort:9463,pluginPort:9464});
  assert.equal((await rpc(app.page,'plugins.list'))[0].activeVersion,'0.2.1');
  assert.equal((await rpc(app.page,'servers.get',{serverId:server.id})).name,'旧版升级副本');
  const snapshot=await rpc(app.page,'workbench.snapshot');assert.equal(snapshot.schemaVersion,2);assert.deepEqual(snapshot.workspaceIds,[server.id]);
  assert.equal(snapshot.views.filter(v=>v.viewType==='terminal.sessions').length,1);
  const backups=(await readdir(data)).filter(name=>name.startsWith('workbench.pre-v2-'));assert.equal(backups.length,1);assert.deepEqual(await readFile(join(data,backups[0])),original);
  assert.ok((await readdir(join(data,'plugins/packages/sodeck.terminal'))).includes('0.1.0'));
  const resources=await rpc(app.page,'resources.list');assert.equal(resources.ptys.length,0);assert.equal(resources.connections.length,0);
  checks.push('ST-12/17: new core activates its signed factory terminal through the package transaction, backs up exact legacy layout bytes, preserves server and tool view, and opens no SSH');
  await writeFile(join(output,'results.json'),JSON.stringify({checks,output},null,2));console.log(JSON.stringify({checks,output},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(error),stack:error.stack},null,2));console.error(error);process.exitCode=1;}finally{await stop(app);}
