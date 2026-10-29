import assert from 'node:assert/strict';
import {mkdir,mkdtemp,readFile,writeFile,copyFile,cp,readdir} from 'node:fs/promises';
import {resolve,join,dirname,delimiter} from 'node:path';
import {createHash} from 'node:crypto';
import {launch,rpc,stop} from './harness.mjs';

const output=resolve('.qa/t016/rebuild');await mkdir(output,{recursive:true});
const run=await mkdtemp(join(output,'run-')),source=resolve('.local/dev/SoDeck-v2-T019');
const manifest=JSON.parse(await readFile(join(source,'build.json'),'utf8'));let app;const checks=[];
const note=text=>{checks.push(text);console.log(text);};
async function copyPackage(name){const dir=join(run,name);for(const file of [...Object.keys(manifest.hashes),'build.json']){await mkdir(dirname(join(dir,file)),{recursive:true});const bytes=await readFile(join(source,file));if(manifest.hashes[file])assert.equal(createHash('sha256').update(bytes).digest('hex'),manifest.hashes[file]);await writeFile(join(dir,file),bytes);}return dir;}
try{
 const old=await copyPackage('old'),original=join(old,'data');
 app=await launch({ordinary:true,data:original,executable:join(old,'sodeck-desktop.exe'),shellPort:9621});
 assert.equal((await rpc(app.page,'app.info')).version,'2.0.0-dev.15');
 const server=await rpc(app.page,'servers.create',{name:'T016 schema rollback fixture',host:'127.0.0.1',port:1,username:'fixture',authType:'key'});
 const oldCalls=await rpc(app.page,'calls.list',{limit:100});
 await stop(app);app=undefined;
 const data=join(run,'upgrade-data');await cp(original,data,{recursive:true});
 const priorBackups=new Set(await readdir(join(data,'backups/core-schema-reset')));
 process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);
 await writeFile(join(output,'state.json'),JSON.stringify({run,data,stage:'awaiting-native-upgrade-notice'},null,2));
 app=await launch({data,shellPort:9623,pluginPort:9624,startupTimeout:180000});
 assert.equal((await rpc(app.page,'app.info')).version,'2.0.0-dev.16');
 assert.ok((await rpc(app.page,'servers.list')).servers.some(s=>s.id===server.id));
 const current=await rpc(app.page,'events.v2.query',{filter:{},limit:100});
 assert.ok(current.items.every(e=>e.schemaVersion===2));await stop(app);app=undefined;
 const backups=join(data,'backups/core-schema-reset');const dirs=(await readdir(backups)).filter(name=>!priorBackups.has(name));assert.equal(dirs.length,1);const backup=join(backups,dirs[0]);
 const journal=JSON.parse(await readFile(join(backup,'backup.json'),'utf8'));
 assert.equal(journal.members.length,4);assert.ok(journal.members.every(m=>m.path.startsWith('observability/')&&m.previousVersion===1&&m.targetVersion===2));
 for(const member of journal.members)for(const file of member.backupFiles){const bytes=await readFile(join(backup,dirname(member.path),file.name));assert.equal(createHash('sha256').update(bytes).digest('hex'),file.sha256);}
 note('Native observation schema 1 to 2 reset preserved the server configuration and backed up all four old partitions with matching hashes');
 const rollback=await copyPackage('rollback');await cp(original,join(rollback,'data'),{recursive:true});
 for(const member of journal.members)await copyFile(join(backup,member.path),join(rollback,'data',member.path));
 app=await launch({ordinary:true,data:join(rollback,'data'),executable:join(rollback,'sodeck-desktop.exe'),shellPort:9625});
 assert.equal((await rpc(app.page,'app.info')).version,'2.0.0-dev.15');
 assert.ok((await rpc(app.page,'servers.list')).servers.some(s=>s.id===server.id));
 const restored=await rpc(app.page,'calls.list',{limit:100});
 const rows=value=>Array.isArray(value)?value:value.records;
 assert.ok(Array.isArray(rows(oldCalls))&&rows(oldCalls).length>0);
 for(const call of rows(oldCalls)){assert.equal(typeof call.id,'string');assert.ok(rows(restored).some(c=>c.id===call.id));}
 await stop(app);assert.equal(app.shutdown.forced,false);app=undefined;
 note('The original dev.15 executable reads the backup-restored copy and the exact old call IDs; the upgraded dataset is kept separate');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,run,data,backup,checks,oldCalls:rows(oldCalls).length,storeEpoch:current.storeEpoch},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({run,checks,error:String(error),code:error.code,stack:error.stack},null,2));throw error;}
finally{await stop(app);}
