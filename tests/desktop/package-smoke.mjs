import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,stop,until,pluginPages} from './harness.mjs';

const packageRoot=process.env.SODECK_TEST_PACKAGE_DIR??join(root,'.local/dev/SoDeck-v2-T006');
const output=join(root,'.qa/t006-package',String(Date.now()));
await mkdir(output,{recursive:true});let app;
try{
  const build=JSON.parse(await readFile(join(packageRoot,'build.json'),'utf8'));
  for(const [file,expected] of Object.entries(build.hashes))assert.equal(createHash('sha256').update(await readFile(join(packageRoot,file))).digest('hex'),expected);
  app=await launch({data:join(output,'app'),shellPort:9503,pluginPort:9504,executable:join(packageRoot,'sodeck-desktop.exe'),packagePath:join(packageRoot,'resources/terminal.sodeck-plugin'),keysPath:join(packageRoot,'resources/development-public-key.json')});
  assert.equal((await rpc(app.page,'app.info')).version,'2.0.0-dev.3');
  assert.equal((await rpc(app.page,'plugins.list'))[0].activeVersion,'0.3.0');
  const server=await rpc(app.page,'servers.create',{name:'目录包离线验收',host:'127.0.0.1',port:22,username:'fixture',authType:'password'});
  await rpc(app.page,'workspaces.open',{serverId:server.id});
  assert.equal((await rpc(app.page,'workbench.snapshot')).schemaVersion,3);
  assert.equal(await app.page.locator('.sessions-panel').count(),0);
  const moved=await rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()});assert.equal(moved.committed,true);
  await rpc(app.page,'windows.merge',{sourceWindowId:moved.targetWindowId,operationId:crypto.randomUUID()});
  assert.deepEqual((await rpc(app.page,'workbench.snapshot')).workspaceIds,[server.id]);
  const resources=await rpc(app.page,'resources.list');for(const key of ['ptys','leases','connections'])assert.equal(resources[key].length,0);
  await stop(app);assert.equal(app.process.exitCode,0);app=null;
  await writeFile(join(output,'result.json'),JSON.stringify({passed:true,build,checks:['copied files match build hashes','relocated core and plugin start','offline workspace has no session sidebar and detaches/merges without SSH','normal exit code 0']},null,2));
  console.log(JSON.stringify({passed:true,output}));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),stack:error.stack},null,2));console.error(error);process.exitCode=1;}
finally{await stop(app);}
