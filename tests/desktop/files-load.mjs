import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import os from 'node:os';
import {performance} from 'node:perf_hooks';
import {rpc,until,delay,pluginRpc} from './harness.mjs';
import {filesHarness,invoke,snapshot,locate} from './files-harness.mjs';
const h=await filesHarness('load',9671),samples={firstPage:[],idleEcho:[],loadedEcho:[],localSelection:[],cycles:[]};
const p95=values=>[...values].sort((a,b)=>a-b)[Math.ceil(values.length*.95)-1];
let inputSeq=0;
async function echo(page,pty,marker){
  const start=performance.now();
  await pluginRpc(page,'pty.write',{ptyId:pty.id,generation:pty.generation,inputSeq:++inputSeq,bytes:Buffer.from(`printf 'T008-%03d\\n' ${marker}\r`).toString('base64')});
  await page.waitForFunction(text=>document.querySelector('.xterm-rows')?.textContent?.includes(text),`T008-${String(marker).padStart(3,'0')}`,{polling:'raf',timeout:5000});
  return performance.now()-start;
}
try{
  const app=await h.start(),a=await h.create('负载 A'),b=await h.create('负载 B','reader');
  await rpc(app.page,'workspaces.open',{serverId:b.id});await h.view('sodeck.files',b.id);await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===1,25000);
  await rpc(app.page,'workspaces.open',{serverId:a.id});const file=await h.view('sodeck.files',a.id),terminal=await h.view('sodeck.terminal',a.id);
  await until(async()=>(await snapshot(file)).state==='ready',25000);
  const pty=(await rpc(app.page,'resources.list')).ptys.find(p=>p.serverId===a.id);
  await pluginRpc(terminal,'pty.write',{ptyId:pty.id,generation:pty.generation,inputSeq:++inputSeq,bytes:Buffer.from('stty -echo\r').toString('base64')});await delay(200);
  for(let i=0;i<100;i++)samples.idleEcho.push(await echo(terminal,pty,i));
  const selected=await locate(file,'/srv/sodeck-t008/many');const key=selected.selected;await invoke(file,'files.toggle',{id:key});await until(async()=>{const s=await snapshot(file);return !s.rows.find(r=>r.id===key)?.loading&&s.rows.some(r=>r.name.startsWith('item-'));});
  const readings=(async()=>{for(let i=0;i<20;i++){const start=performance.now();await invoke(file,'files.refresh',{id:key});await until(async()=>{const s=await snapshot(file);return !s.rows.find(r=>r.id===key)?.loading&&s.rows.some(r=>r.name.startsWith('item-'));},10000);samples.firstPage.push(performance.now()-start);}})();
  for(let i=0;i<100;i++)samples.loadedEcho.push(await echo(terminal,pty,100+i));await readings;
  for(let i=0;i<30;i++){const start=performance.now();await file.getByRole('tree').focus();await file.keyboard.press(i%2?'ArrowUp':'ArrowDown');await file.waitForFunction(()=>Boolean(document.querySelector('[role=treeitem][aria-selected=true]')));samples.localSelection.push(performance.now()-start);}
  for(let i=0;i<20;i++){
    const start=performance.now();await rpc(app.page,'workspaces.close',{workspaceId:a.id});await rpc(app.page,'workspaces.open',{serverId:a.id});const p=await h.view('sodeck.files',a.id);await until(async()=>(await snapshot(p)).state==='ready',25000);await rpc(app.page,'workspaces.close',{workspaceId:a.id});
    await until(async()=>{const r=await rpc(app.page,'resources.list');return r.connections.length===1&&r.leases.length<=2&&!r.ptys.some(p=>p.serverId===a.id);},5000);samples.cycles.push(performance.now()-start);
  }
  await rpc(app.page,'workspaces.close',{workspaceId:b.id});assert.equal((await rpc(app.page,'resources.list')).connections.length,0);
  const metrics={firstPage:p95(samples.firstPage),idleEcho:p95(samples.idleEcho),loadedEcho:p95(samples.loadedEcho),localSelection:p95(samples.localSelection)};
  await writeFile(join(h.output,'metrics.json'),JSON.stringify({environment:{cpu:os.cpus()[0].model,logicalCpus:os.cpus().length,memory:os.totalmem(),platform:os.platform(),guest:'Debian 13 / QEMU 11.1.0 TCG, 2 vCPU, 2 GiB'},metrics,samples},null,2));console.log(metrics);
  assert.ok(metrics.firstPage<=2000,'first-page p95');assert.ok(metrics.loadedEcho<=150,'loaded PTY p95');assert.ok(metrics.loadedEcho-metrics.idleEcho<=100,'PTY latency increment');assert.ok(metrics.localSelection<=100,'local response p95');
}finally{await writeFile(join(h.output,'samples.json'),JSON.stringify(samples,null,2));await h.stop();}
