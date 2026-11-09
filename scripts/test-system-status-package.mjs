// Verify the executable and UI from the actual archive, independently of core packaging.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import readline from 'node:readline';
import {spawn,spawnSync} from 'node:child_process';
import {cargoPackageVersion} from './cargo-package.mjs';
const target=spawnSync('rustc',['-vV'],{encoding:'utf8'}).stdout.match(/^host: (.+)$/m)?.[1];
const version=cargoPackageVersion('plugins/system-status/Cargo.toml');
const stem=`sodeck-system-status_${version}_${target}`;
const archive=path.resolve(`artifacts/plugins/${stem}${process.platform==='win32'?'.zip':'.tar.gz'}`);
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-status-package-'));
let child;
try{
  const extract=spawnSync('tar',['-xf',archive,'-C',directory],{encoding:'utf8',windowsHide:true});assert.equal(extract.status,0,extract.stderr);
  const root=path.join(directory,stem),manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json'),'utf8'));
  assert.equal(manifest.target,target);assert.equal(manifest.ui.views[0].backgroundOnWorkspace,true);
  for(const file of ['index.html','view.js','charts.js','style.css','shared.css','text-menu.js','clipboard-bridge.js'])assert(fs.statSync(path.join(root,'ui',file)).size>0);
  child=spawn(path.join(root,manifest.entry),[],{cwd:root,stdio:['pipe','pipe','pipe'],windowsHide:true});
  let sequence=0;const pending=new Map();
  const send=frame=>child.stdin.write(JSON.stringify({jsonrpc:'2.0',...frame})+'\n');
  readline.createInterface({input:child.stdout}).on('line',line=>{const f=JSON.parse(line);if(f.method){let result={};if(f.method==='settings.get')result={values:{dynamicIntervalSeconds:'5'}};else if(f.method==='session.state')result={context:{workspaceId:'w',serverId:'s',connectionId:'c',generation:1},state:'disconnected'};send({id:f.id,result});}else{const p=pending.get(f.id);if(!p)return;pending.delete(f.id);clearTimeout(p.timer);f.error?p.reject(new Error(f.error.message)):p.resolve(f.result);}});
  const request=(method,params)=>new Promise((resolve,reject)=>{const id=String(++sequence),timer=setTimeout(()=>reject(new Error('Package RPC timeout: '+method)),6000);pending.set(id,{resolve,reject,timer});send({id,method,params});});
  await request('initialize',{apiVersion:'1',capabilities:['settings.v1','session.exec.v1','ui.background.v1'],dataDirectory:directory});
  const context={workspaceId:'w',serverId:'s',connectionId:'c',generation:1};
  const mounted=await request('ui.createView',{viewId:'v',viewType:'overview',context});assert.equal(mounted.metadata.dirty,false);
  await request('ui.getView',{viewId:'v',viewType:'overview',context});
  await request('ui.event',{viewId:'v',data:{action:'pause'}});
  assert.equal((await request('ui.event',{viewId:'v',data:{action:'snapshot'}})).data.paused,true);
  await request('ui.disposeView',{viewId:'v'});await request('shutdown',{});
  const exit=await new Promise(resolve=>{if(child.exitCode!==null)resolve(child.exitCode);else child.once('exit',resolve);});assert.equal(exit,0);
  console.log(`System status archive: native ${target}, initialize/mount/pause/dispose/shutdown and packaged UI passed.`);
}finally{if(child?.exitCode===null&&child.signalCode===null){const exited=new Promise(resolve=>child.once('exit',resolve));child.kill();await exited;}assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-status-package-'));fs.rmSync(directory,{recursive:true,force:true});}
