// Opens a COPY of a closed T-020 v6 test database; the old test data is never edited.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,cp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {createHash} from 'node:crypto';
import {launch,rpc,stop} from './harness.mjs';
const source=resolve('.qa/t020/native-1793169316236/data/core'),output=resolve('.qa/t025/compatibility-'+Date.now()),data=join(output,'data');
await mkdir(data,{recursive:true});await cp(source,join(data,'core'),{recursive:true});
const python='C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const database=join(data,'core/core.sqlite');
const inspect=async()=>JSON.parse((await promisify(execFile)(python,['-c',"import sqlite3,json,sys; c=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); print(json.dumps({'version':c.execute('pragma user_version').fetchone()[0],'servers':c.execute('select id,data from servers order by id').fetchall(),'proxies':c.execute('select id,data from socks_proxies order by id').fetchall()}))",database],{windowsHide:true})).stdout);
const sourceHash=createHash('sha256').update(await readFile(join(source,'core.sqlite'))).digest('hex'),before=await inspect();assert.equal(before.version,6);assert.ok(before.servers.length);
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`.qa/t025/packages/${n}.sodeck-plugin`)).join(delimiter);
let app;const checks=[];
try{
 app=await launch({data,shellPort:12581,pluginPort:12582});await app.page.getByText(`共 ${before.servers.length} 台`,{exact:true}).waitFor();
 const old=await rpc(app.page,'servers.list');assert.equal(old.servers.length,before.servers.length);assert.equal((await rpc(app.page,'proxies.socks5.list')).proxies.length,before.proxies.length);
 assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,0);assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
 const added=await rpc(app.page,'proxies.catalog.save',{operationId:crypto.randomUUID(),configuration:{name:'T025 compatibility',host:'new.invalid',port:1080,authType:'none',username:''}});
 assert.equal((await rpc(app.page,'proxies.socks5.get',{proxyId:added.proxy.id})).name,'T025 compatibility');
 await stop(app);app=undefined;
 const after=await inspect();assert.equal(after.version,6);assert.deepEqual(after.servers,before.servers);for(const row of before.proxies)assert.deepEqual(after.proxies.find(v=>v[0]===row[0]),row);
 assert.equal(createHash('sha256').update(await readFile(join(source,'core.sqlite'))).digest('hex'),sourceHash);
 assert.ok(!(await readdir(data)).some(n=>n.includes('pre-v6')));
 checks.push('Existing T020 v6 servers/proxies retain IDs, names, identities and revisions after startup and a new API write; old API reads the new proxy; no workspace/PTY or database rebuild');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,source,sourceHash,before,after},null,2));
}finally{await stop(app);await writeFile('.qa/t025/compatibility-latest.json',JSON.stringify({output,checks},null,2));}
