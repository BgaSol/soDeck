import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile,writeFile,mkdir,copyFile,readdir,access} from 'node:fs/promises';
import {resolve,join,dirname} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {until,delay} from './harness.mjs';
const source=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T015-startup-check'),output=resolve('.local/test-results/t015/startup-'+Date.now());await mkdir(output,{recursive:true});
const manifest=JSON.parse(await readFile(join(source,'build.json'),'utf8')),results=[];
const ps=async(action,pack,pid,extra={})=>{const args=['-NoProfile','-File','tests/desktop/startup-gui.ps1','-Action',action,'-Package',pack,...(pid?['-AppProcessId',String(pid)]:[])];for(const [k,v]of Object.entries(extra))args.push('-'+k,String(v));const {stdout}=await promisify(execFile)('powershell.exe',args,{windowsHide:true,timeout:45000});return stdout.trim()?JSON.parse(stdout):null;};
const inspect=(pack,pid)=>ps('inspect',pack,pid),button=(pack,pid,name)=>ps('button',pack,pid,{Name:name});
const alive=pid=>{try{process.kill(pid,0);return true;}catch{return false;}};
const noConsole=s=>{assert.equal(s.processes.some(p=>/^(conhost|cmd|powershell|pwsh|OpenConsole)\.exe$/i.test(p.Name)),false);assert.equal(s.windows.some(w=>w.kind==='ConsoleWindowClass'),false);};
async function copy(name){const pack=resolve(`.local/dev/T015 启动 ${name}-${Date.now()}`);for(const p of [...Object.keys(manifest.hashes),'build.json']){await mkdir(dirname(join(pack,p)),{recursive:true});await copyFile(join(source,p),join(pack,p));}return pack;}
async function ready(pack,pid){return until(async()=>{const s=await inspect(pack,pid);return s.elements.some(e=>e.name==='添加第一台服务器')?s:false;},20000);}
async function exit(pack,pid){await button(pack,pid,'应用菜单');await button(pack,pid,'退出 SoDeck');await until(()=>!alive(pid));}
async function files(root){const all=[];for(const e of await readdir(root,{withFileTypes:true})){const p=join(root,e.name);if(e.isDirectory())all.push(...await files(p));else all.push(p);}return all;}
try{
 for(const version of [3,5]){
  const pack=await copy('版本'+version),core=join(pack,'data/core');await mkdir(join(core,'secrets'),{recursive:true});
  let db=new DatabaseSync(join(core,'core.sqlite'));db.exec(`CREATE TABLE marker(value TEXT);INSERT INTO marker VALUES('old-${version}');PRAGMA user_version=${version}`);db.close();
  await writeFile(join(core,'secrets/fixture.bin'),'encrypted-fixture-only');const launched=await ps('launch',pack,null,{ObserveMs:300});const pid=launched.pid;
  try{
   assert.deepEqual(launched.consoleWindows,[]);const notice=await inspect(pack,pid);noConsole(notice);assert.equal(notice.processes.length,1);assert.ok(notice.elements.some(e=>e.name.includes('数据库版本与当前程序不一致')));
   await writeFile(join(output,`version-${version}-notice.json`),JSON.stringify(notice,null,2));await ps('capture',pack,pid,{OutputPath:join(output,`version-${version}-notice.png`)});
   await button(pack,pid,'确定');const running=await ready(pack,pid);noConsole(running);assert.equal(running.processes.filter(p=>p.CommandLine?.includes('--sodeck-internal-plugin-host')).length,3);
   await assert.rejects(access(join(pack,'SHOULD-NOT-EXIST')));await exit(pack,pid);
   db=new DatabaseSync(join(core,'core.sqlite'));assert.equal(db.prepare('PRAGMA user_version').get().user_version,4);assert.equal(db.prepare('SELECT count(*) AS n FROM servers').get().n,0);assert.equal(db.prepare("SELECT count(*) AS n FROM sqlite_master WHERE name='marker'").get().n,0);db.close();
   const backupFiles=await files(join(pack,'data/backups')),backup=backupFiles.find(p=>p.endsWith('core.sqlite')&&!p.includes('original'));
   assert.ok(backup);db=new DatabaseSync(backup,{readOnly:true});assert.equal(db.prepare('SELECT value FROM marker').get().value,'old-'+version);db.close();assert.ok(backupFiles.some(p=>p.endsWith('fixture.bin')));
   // Same-version reopen retains a marker; no second reset or notice.
   db=new DatabaseSync(join(core,'core.sqlite'));db.exec("CREATE TABLE same_version_marker(value TEXT);INSERT INTO same_version_marker VALUES('keep')");db.close();
   const reopened=await ps('launch',pack);try{await ready(pack,reopened.pid);await exit(pack,reopened.pid);}finally{if(alive(reopened.pid))await ps('stop',pack,reopened.pid);}
   db=new DatabaseSync(join(core,'core.sqlite'));assert.equal(db.prepare('SELECT value FROM same_version_marker').get().value,'keep');db.close();
   results.push({version,pack,reset:true,backupVerified:true,sameVersionPreserved:true});console.log('Version '+version+': native notice before workers; backup and fresh v4 verified; same-version data retained');
  }finally{if(alive(pid))await ps('stop',pack,pid);}
 }
 const pack=await copy('备份失败'),core=join(pack,'data/core');await mkdir(core,{recursive:true});let db=new DatabaseSync(join(core,'core.sqlite'));db.exec('CREATE TABLE marker(value TEXT);PRAGMA user_version=3');db.close();await writeFile(join(pack,'data/backups'),'blocked');
 const launched=await ps('launch',pack);try{await button(pack,launched.pid,'确定');const failure=await until(async()=>{const s=await inspect(pack,launched.pid);return s.elements.some(e=>e.name.includes('无法启动 SoDeck'))?s:false;});noConsole(failure);assert.equal(failure.processes.length,1);await writeFile(join(output,'backup-failure.json'),JSON.stringify(failure,null,2));await button(pack,launched.pid,'确定');await until(()=>!alive(launched.pid));db=new DatabaseSync(join(core,'core.sqlite'));assert.equal(db.prepare('PRAGMA user_version').get().user_version,3);db.close();results.push({backupFailurePreserved:true,pack});console.log('Backup failure: visible native error, original v3 retained, no worker');}finally{if(alive(launched.pid))await ps('stop',pack,launched.pid);}
}finally{await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));await writeFile('.local/test-results/t015/startup-latest.json',JSON.stringify({output},null,2));}
