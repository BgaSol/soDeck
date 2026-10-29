import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import {launch,rpc,stop} from './harness.mjs';
const directory=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T025'),output=resolve('.qa/t025/package-smoke');await mkdir(output,{recursive:true});
const build=JSON.parse(await readFile(join(directory,'build.json'),'utf8'));
for(const [name,expected] of Object.entries(build.hashes))assert.equal(createHash('sha256').update(await readFile(join(directory,name))).digest('hex'),expected,name);
let app;let evidence;
try{
 app=await launch({ordinary:true,bundledResources:true,data:join(directory,'data'),executable:join(directory,'sodeck-desktop.exe'),cwd:directory,shellPort:12591,pluginPort:12592});
 await app.page.getByRole('heading',{name:'新建 SSH 连接',exact:true}).waitFor();
 const info=await rpc(app.page,'app.info'),plugins=await rpc(app.page,'plugins.list'),caps=await rpc(app.page,'capabilities.list');
 assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,0);assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
 for(const p of build.pluginPackages){const actual=plugins.find(v=>v.pluginId===p.pluginId);assert.equal(actual?.activeVersion,p.version);assert.ok(!actual.error);}
 await app.page.getByRole('button',{name:'新建代理',exact:true}).click();await app.page.getByRole('heading',{name:'新建代理',exact:true}).waitFor();assert.equal(await app.page.getByLabel('代理端口',{exact:true}).inputValue(),'1080');
 await app.page.getByRole('button',{name:'新建 SSH 连接',exact:true}).click();await app.page.getByRole('heading',{name:'新建 SSH 连接',exact:true}).waitFor();
 await app.page.screenshot({path:join(output,'connection-home.png')});evidence={passed:true,directory,info,plugins,caps,coreSha256:build.hashes['sodeck-desktop.exe'],ordinary:true,override:'No SODECK_* variables; only WebView2 debugging port for read-only inspection',errors:app.errors};assert.deepEqual(app.errors,[]);
}finally{await stop(app);await writeFile(join(output,'results.json'),JSON.stringify({...evidence,shutdown:app?.shutdown},null,2));}
