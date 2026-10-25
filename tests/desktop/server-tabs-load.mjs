import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,until,stop,pluginPages,delay} from './harness.mjs';

const output=join(root,'.qa/t-004/load',String(Date.now()));await mkdir(output,{recursive:true});
const fixture=spawn(process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe',['-u',join(root,'tests/core/ssh_fixture.py'),join(output,'fixture')],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,PYTHONPATH:join(root,'.local/python')}});
let ready='',fixtureErrors='',app;fixture.stdout.on('data',d=>ready+=d);fixture.stderr.on('data',d=>fixtureErrors+=d);
const checks=[],measurements={queries:[],stops:[],memory:[]};
checks.push=(...items)=>{for(const item of items)console.log(item);return Array.prototype.push.apply(checks,items);};
const surface=id=>until(async()=>{for(const page of await pluginPages(app)){try{if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===id)return page;}catch{}}});
const percentile=values=>[...values].sort((a,b)=>a-b)[Math.ceil(values.length*.95)-1];
async function processStats(){
  const command=`$all=Get-CimInstance Win32_Process; $ids=[Collections.Generic.HashSet[int]]::new(); [void]$ids.Add(${app.process.pid}); do { $changed=$false; foreach($p in $all){if($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)){$changed=$true}} } while($changed); @($all | Where-Object {$ids.Contains([int]$_.ProcessId)} | ForEach-Object {$p=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue; @{id=$_.ProcessId;parent=$_.ParentProcessId;name=$_.Name;worker=($_.CommandLine -like '*--sodeck-internal-plugin-host*');privateBytes=$p.PrivateMemorySize64;workingSet=$p.WorkingSet64;peakWorkingSet=$p.PeakWorkingSet64}}) | ConvertTo-Json -Compress`;
  const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-Command',command],{windowsHide:true});return JSON.parse(stdout);
}
try{
  await until(()=>ready.includes('\n'));const {port}=JSON.parse(ready.split('\n')[0]);
  app=await launch({data:join(output,'app'),shellPort:9473,pluginPort:9474});
  const servers=[];
  for(const name of ['负载 A','负载 B','负载 C']){
    const s=await rpc(app.page,'servers.create',{name,host:'127.0.0.1',port,username:'test-user',authType:'password'});
    let challenge;try{await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id});}catch(e){challenge=e;}
    assert.ok(['HOST_KEY_REQUIRED','AUTH_REQUIRED'].includes(challenge?.code));if(challenge.code==='HOST_KEY_REQUIRED')await rpc(app.page,'hosts.trust',{serverId:s.id,...challenge.details});
    await rpc(app.page,'auth.set',{serverId:s.id,password:'isolated-test-password'});await rpc(app.page,'workspaces.open',{serverId:s.id});servers.push(s);
  }
  const [a,b,c]=servers,views=[];
  for(const s of [a,a,b,b])views.push(await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:s.id}));
  for(const v of views){await rpc(app.page,'views.activate',{viewId:v.viewId});const p=await surface(v.viewId);await until(()=>p.locator('.xterm-rows').innerText().then(t=>t.includes('fixture-ready')));}
  const resources=await rpc(app.page,'resources.list');const ids=resources.ptys.map(p=>p.id).sort();
  const initialProcesses=await processStats();const worker=initialProcesses.filter(p=>p.worker).map(p=>p.id);assert.equal(worker.length,1);
  await rpc(app.page,'views.move',{viewId:views[1].viewId,placement:'detached'});await rpc(app.page,'views.move',{viewId:views[3].viewId,placement:'detached'});
  await rpc(app.page,'workspaces.move',{workspaceId:b.id,operationId:crypto.randomUUID()});await rpc(app.page,'workspaces.move',{workspaceId:c.id,operationId:crypto.randomUUID()});
  await rpc(app.page,'workspaces.activate',{workspaceId:a.id});
  assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,3);
  assert.equal(app.browser.contexts().flatMap(ctx=>ctx.pages()).filter(p=>p.url().includes('tauri.localhost')).length,5);
  checks.push('ST-15: main, two workspace windows and two detached view windows share exactly one worker and four PTYs');
  // Move A while one of its views is detached, then return that view.
  const aWindow=await rpc(app.page,'workspaces.move',{workspaceId:a.id,operationId:crypto.randomUUID()});
  await rpc(app.page,'views.move',{viewId:views[1].viewId,placement:'center'});
  assert.equal(await (await surface(views[1].viewId)).evaluate(()=>window.__SODECK_CONTEXT__.windowId),aWindow.targetWindowId);
  await rpc(app.page,'workspaces.move',{workspaceId:a.id,targetWindowId:'main',operationId:crypto.randomUUID()});
  for(let n=0;n<10;n++){
    await rpc(app.page,'workspaces.move',{workspaceId:a.id,operationId:crypto.randomUUID()});
    await rpc(app.page,'workspaces.move',{workspaceId:a.id,targetWindowId:'main',operationId:crypto.randomUUID()});
    assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort(),ids);
  }
  checks.push('ST-05/07: ten whole-workspace round trips keep all PTY identities; detached view returns to its current owner');
  await rpc(app.page,'views.move',{viewId:views[1].viewId,placement:'detached'});
  await rpc(app.page,'views.activate',{viewId:views[0].viewId});await rpc(app.page,'views.activate',{viewId:views[2].viewId});
  const pages=[];
  for(const v of views){
    const page=await surface(v.viewId);await page.waitForSelector('.xterm');pages.push(page);
    await page.evaluate(()=>{
      window.__load={bytes:0,started:0,markers:[],pending:{},tail:''};
      const receive=window.__sodeckReceive;
      window.__sodeckReceive=response=>{
        const load=window.__load;
        for(const frame of response.result?.frames??[]){
          const bytes=atob(frame.bytes);load.bytes+=bytes.length;load.tail=(load.tail+bytes).slice(-32768);
          for(const [marker,start] of Object.entries(load.pending))if(load.tail.includes(marker)){load.markers.push(performance.now()-start);delete load.pending[marker];}
        }
        return receive(response);
      };
    });
    await page.locator('.xterm-helper-textarea').focus();await page.keyboard.insertText('__SODECK_TEST_STREAM__');await page.keyboard.press('Enter');
  }
  const started=Date.now();let sample=0;
  while(Date.now()-started<60000){
    const t=performance.now();await rpc(app.page,'resources.list');measurements.queries.push(performance.now()-t);
    for(const page of pages){await page.locator('.xterm-helper-textarea').focus();const marker=`RTT${sample++}X`;await page.evaluate(marker=>{window.__load.pending[marker]=performance.now();},marker);await page.keyboard.insertText(marker);}
    if(sample%40===4)measurements.memory.push(await processStats());
    await delay(300);
  }
  measurements.elapsedMs=Date.now()-started;measurements.views=await Promise.all(pages.map(page=>page.evaluate(()=>window.__load)));
  measurements.environment={webview:app.browser.version(),node:process.version,os:process.platform,cpu:(await import('node:os')).cpus()[0].model,ram:(await import('node:os')).totalmem()};
  for(const view of measurements.views){assert.ok(view.bytes>15*1024*1024,`observed ${view.bytes} bytes`);assert.ok(view.markers.length>20);assert.ok(percentile(view.markers)<=500,`marker P95 ${percentile(view.markers)}`);}
  assert.ok(percentile(measurements.queries)<=500);
  for(const view of views){const t=performance.now();await rpc(app.page,'views.close',{viewId:view.viewId});measurements.stops.push(performance.now()-t);}
  assert.ok(percentile(measurements.stops)<=500,`stop P95 ${percentile(measurements.stops)}`);
  checks.push('ST-15: four native terminal WebViews receive 256 KiB/s per PTY for 60 seconds; per-view RTT, query and stop P95 <= 500 ms');
  for(const s of servers)await rpc(app.page,'workspaces.close',{workspaceId:s.id});
  for(let n=0;n<20;n++){
    await rpc(app.page,'workspaces.open',{serverId:a.id,target:'newWindow'});
    const v=await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:a.id});await surface(v.viewId);
    await rpc(app.page,'workspaces.close',{workspaceId:a.id});
  }
  await delay(600);const after=await rpc(app.page,'resources.list');
  for(const key of ['ptys','leases','connections'])assert.equal(after[key].length,0);
  assert.deepEqual((await processStats()).filter(p=>p.worker).map(p=>p.id),worker);
  assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0);
  measurements.cleanup=await until(async()=>{
    const d=(await rpc(app.page,'diagnostics.get')).workbench;
    return Object.entries(d).every(([key,value])=>key==='native'?Object.values(value).every(n=>n===0):value===0)?d:false;
  });
  assert.equal(app.browser.contexts().flatMap(ctx=>ctx.pages()).filter(p=>p.url().includes('tauri.localhost')).length,1);
  checks.push('ST-15: twenty create/close cycles return to one main window, no views or SSH resources, and the same worker');
  await stop(app);assert.equal(app.process.exitCode,0);app=null;
  await writeFile(join(output,'results.json'),JSON.stringify({checks,measurements},null,2));console.log(JSON.stringify({output,checks}));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({checks,measurements,error:String(error),stack:error.stack},null,2));console.error(error);process.exitCode=1;}
finally{await stop(app);fixture.kill();await writeFile(join(output,'fixture.log'),fixtureErrors);}
