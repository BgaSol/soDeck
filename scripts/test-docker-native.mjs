// Actual Windows WebView2, authenticated core session and independent Docker archive.
// The caller owns the isolated SSH/Engine fixture; no production server is accepted implicitly.
import assert from 'node:assert/strict';
import fs from 'node:fs';import path from 'node:path';import net from 'node:net';import http from 'node:http';
import {spawn,spawnSync} from 'node:child_process';import {setTimeout as delay} from 'node:timers/promises';
import {createHash} from 'node:crypto';import {chromium} from 'playwright';
assert.equal(process.platform,'win32','Native log driver requires actual WebView2');
const executable=path.resolve(process.argv[2]),plugin=path.resolve(process.env.SODECK_TEST_DOCKER_PLUGIN);
const ssh=JSON.parse(fs.readFileSync(process.env.SODECK_TEST_SSH_CONFIG,'utf8'));
assert(fs.existsSync(executable));assert(/^[a-f0-9]{64}$/.test(ssh.containerId));
const duration=Number(process.env.SODECK_LOG_DURATION_MS||60000),manual=Number(process.env.SODECK_LOG_MANUAL_MS||0);
const reopenCycles=Number(process.env.SODECK_LOG_REOPEN_CYCLES||20);
assert(Number.isSafeInteger(duration)&&duration>=1000&&duration<=5400000);assert(Number.isSafeInteger(manual)&&manual>=0&&manual<=1800000);
assert(Number.isSafeInteger(reopenCycles)&&reopenCycles>=20&&reopenCycles<=100);
const reportRoot=path.resolve('artifacts/log-validation/native-'+new Date().toISOString().replaceAll(/[:.]/g,'-'));
const directory=path.join(reportRoot,'app-data');fs.mkdirSync(directory,{recursive:true});
const manifest=JSON.parse(fs.readFileSync(path.join(plugin,'plugin.json'),'utf8'));
fs.writeFileSync(path.join(directory,'plugins.json'),JSON.stringify([{directory:plugin,manifest,enabled:true}]));
const report={platform:process.platform,arch:process.arch,executable,executableSha256:createHash('sha256').update(fs.readFileSync(executable)).digest('hex'),pluginVersion:manifest.version,plugin,started:new Date().toISOString(),fixture:{engine:'24.0.9',historyLines:1000001,targetLinesPerSecond:1000,typicalBodyBytes:128,engineDiskLimitBytes:30*1024*1024},checks:[],samples:[],errors:[],manual:'not executed'};
let child,browser,page,frame,logView,browserView,context,runtime='';
const until=async(fn,message,timeout=30000)=>{const end=Date.now()+timeout;while(!await fn()){assert(Date.now()<end,message+'\n'+runtime);await delay(100);}};
const save=()=>fs.writeFileSync(path.join(reportRoot,'result.json'),JSON.stringify(report,null,2)+'\n');
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
const views=async()=>{const s=await invoke('component_ui',{operation:'snapshot',args:{}});return s.workspaces[context.workspaceId].views;};
const metrics=()=>frame.locator('body').evaluate(()=>globalThis.__logProbe);
const processes=()=>{const r=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-Command',`$all=Get-CimInstance Win32_Process; $ids=[System.Collections.Generic.HashSet[int]]::new(); [void]$ids.Add(${child.pid}); do { $added=$false; foreach($p in $all) { if($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added=$true } } } while($added); @(Get-Process -Id @($ids) -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,WorkingSet64,PrivateMemorySize64) | ConvertTo-Json -Compress`],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout||'[]');};
try {
  const socket=net.createServer();await new Promise(r=>socket.listen(0,'127.0.0.1',r));const port=socket.address().port;await new Promise(r=>socket.close(r));
  child=spawn(executable,['--interactive-test','--test-data-dir',directory],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`,WEBVIEW2_USER_DATA_FOLDER:path.join(directory,'webview2')}});
  child.stdout.on('data',b=>runtime=(runtime+b).slice(-16384));child.stderr.on('data',b=>runtime=(runtime+b).slice(-16384));child.on('error',e=>report.errors.push(String(e)));
  await until(()=>new Promise(resolve=>{assert.equal(child.exitCode,null,runtime);const r=http.get({hostname:'127.0.0.1',port,path:'/json/version',timeout:500},s=>{s.resume();resolve(s.statusCode===200);});r.on('timeout',()=>r.destroy());r.on('error',()=>resolve(false));}),'WebView2 CDP endpoint unavailable');
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  await until(()=>{page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));return !!page;},'No actual Tauri page');
  page.on('pageerror',e=>report.errors.push(String(e)));
  // Observation only: retain bounded timing/response receipts from the real module Worker.
  await browser.contexts()[0].addInitScript(()=>{
    const p=globalThis.__logProbe={workerCreated:0,workerTerminated:0,firstBodyAt:null,firstPaintAt:null,lastView:null,longTasks:[],times:{query:[],navigate:[],view:[]},exports:[],queryPaintText:null,actions:[]};
    document.addEventListener('click',e=>{const button=e.target.closest('button,input,select');if(button){p.actions.push({at:Date.now(),type:'click',id:button.id,focus:button.dataset.focus,trusted:e.isTrusted});if(p.actions.length>100)p.actions.shift();}},true);
    const Original=Worker;
    globalThis.Worker=class extends Original {
      constructor(...a){super(...a);p.workerCreated++;const requests=new Map();this.addEventListener('message',e=>{const q=requests.get(e.data.requestId);if(!q)return;requests.delete(e.data.requestId);if(p.times[q.type]?.length<100)p.times[q.type].push(performance.now()-q.at);if(q.type==='query')p.pendingQuery=q.query;if(q.type==='view')p.lastView=e.data.data;if(q.type==='exportChunk')p.exports.push([...e.data.data.bytes]);});this.postMessage=(value,...rest)=>{requests.set(value.requestId,{type:value.type,at:performance.now(),query:value.conditions?.query});if(['latest','reset','restore','anchor'].includes(value.type)){p.actions.push({at:Date.now(),type:value.type,generation:value.generation,stack:new Error().stack});if(p.actions.length>100)p.actions.shift();}if(value.type==='append'&&p.firstBodyAt===null)p.firstBodyAt=performance.now();return Original.prototype.postMessage.call(this,value,...rest);};}
      terminate(){p.workerTerminated++;return super.terminate();}
    };
    try{new PerformanceObserver(list=>{for(const e of list.getEntries())if(p.longTasks.length<2000)p.longTasks.push({start:e.startTime,duration:e.duration});}).observe({type:'longtask',buffered:true});}catch{}
    new MutationObserver(entries=>{if(p.firstBodyAt!==null&&p.firstPaintAt===null&&document.querySelector('.log-text')?.textContent)p.firstPaintAt=performance.now();if(p.pendingQuery!==undefined&&entries.some(e=>e.target.id==='log-lines')&&document.querySelector('#log-query')?.value===p.pendingQuery){p.queryPaintText=p.pendingQuery;delete p.pendingQuery;}}).observe(document,{childList:true,subtree:true});
  });
  await until(async()=>(await invoke('snapshot')).plugins.some(p=>p.manifest.id==='sodeck.docker'&&p.status==='running'),'Docker plugin did not start');
  const catalog=await invoke('server_catalog'),changed=await invoke('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'Docker日志隔离验收',host:'127.0.0.1',port:ssh.port,username:ssh.username,authentication:{type:'privateKey',privateKeyPath:ssh.privateKeyPath,secret:{action:'clear'}},route:[]}}});
  await invoke('connection',{command:{op:'start',serverId:changed.selectedId}});
  await until(async()=>{const [v]=await invoke('connection',{command:{op:'snapshot'}});assert.notEqual(v.state,'failed',JSON.stringify(v.error));if(v.question)await invoke('connection',{command:{op:'answer',attemptId:v.attemptId,questionId:v.question.id,answer:{kind:'hostKey',accept:true}}});return v.state==='connected';},'SSH connection failed');
  context=(await invoke('workspace_snapshot')).workspaces[0].context;
  await page.locator(`[data-id="${context.workspaceId}"] [data-workspace-action="select"]`).click();
  const opened=await invoke('component_ui',{operation:'action',args:{context,action:{op:'open',key:'sodeck.docker:browser',requestId:'native-docker'}}});browserView=opened.viewId;
  const browserFrame=page.frameLocator(`.component-surface[data-view-id="${browserView}"] iframe`);
  await browserFrame.locator('#query').fill(ssh.containerId);await browserFrame.locator(`[data-tree-key="container:${ssh.containerId}"]`).click();await browserFrame.getByRole('button',{name:'日志',exact:true}).click();
  await until(async()=>{logView=(await views()).find(v=>v.definitionKey==='sodeck.docker:logs')?.id;return !!logView;},'Logs view did not open');
  frame=page.frameLocator(`.component-surface[data-view-id="${logView}"] iframe`);
  await frame.locator('.log-text').first().waitFor({timeout:45000});await until(()=>frame.locator('#log-status').textContent().then(s=>s==='持续接收'),'Live capture did not start');
  report.checks.push('actual WebView2 module Worker/CSP/core/plugin/SSH/Engine body receipt');
  console.log('Native logs ready:',reportRoot);fs.writeFileSync(path.join(reportRoot,'ready.json'),JSON.stringify({pid:child.pid,port,reportRoot,directory,logView,browserView,context}));
  if(manual){report.manual='awaiting native device interaction';save();const end=Date.now()+manual;while(Date.now()<end&&!fs.existsSync(path.join(reportRoot,'manual-complete.json')))await delay(500);assert(fs.existsSync(path.join(reportRoot,'manual-complete.json')),'Native device interaction was not signed off before the deadline');report.manual=JSON.parse(fs.readFileSync(path.join(reportRoot,'manual-complete.json'),'utf8').replace(/^\uFEFF/,''));assert.notEqual(report.manual.result,'failed','Native device acceptance failed');}
  const exportPath=path.join(reportRoot,'中文 export logs.txt');
  if(fs.existsSync(exportPath)) {const probe=await metrics();const expected=Buffer.concat(probe.exports.map(b=>Buffer.from(b)));assert(expected.length>0,'No actual frozen export receipt');const actual=fs.readFileSync(exportPath);assert.deepEqual(actual,expected);report.checks.push('native save dialog produced exact frozen export bytes');report.export={bytes:actual.length,sha256:createHash('sha256').update(actual).digest('hex')};await frame.locator('body').evaluate(()=>globalThis.__logProbe.exports=[]);}
  // Start following, then keep a reading anchor through at least ten actual core tail rotations.
  while(await frame.locator('.log-tools details[open] summary').count())await frame.locator('.log-tools details[open] summary').first().click();
  if(await frame.locator('#log-latest').isVisible())await frame.locator('#log-latest').click();await delay(1500);
  await frame.getByText('显示',{exact:true}).click();await frame.locator('#log-hold').click();await frame.getByText('显示',{exact:true}).click();await delay(500);
  const anchor=await frame.locator('#log-lines').evaluate(el=>{const viewport=el.parentElement,viewportTop=viewport.getBoundingClientRect().top,r=[...el.querySelectorAll('.log-line')].find(x=>x.getBoundingClientRect().bottom>viewportTop);return {id:r.dataset.id,top:r.getBoundingClientRect().top,relativeTop:r.getBoundingClientRect().top-viewportTop,scroll:viewport.scrollTop,height:viewport.clientHeight,linesTop:el.offsetTop,footerHeight:document.querySelector('.log-footer').getBoundingClientRect().height};});
  report.anchor=anchor;report.initialView=await metrics();report.initialView.exports=[];await page.screenshot({path:path.join(reportRoot,'protected-before.png')});
  const baseline=await frame.locator('body').evaluate(()=>sodeck.request('event',{action:'logs.status'}));
  const initialProduced=Number(baseline.resources.find(r=>r.end==null).status.producedBytes);
  const start=Date.now();let round=0,lastProduced=initialProduced,lastEarliest=0,evictions=0;
  while(Date.now()-start<duration){
    await delay(Math.max(1,Math.min(10000,duration-(Date.now()-start))));
    const capture=await frame.locator('body').evaluate(()=>sodeck.request('event',{action:'logs.status'}));const live=capture.resources.find(r=>r.end==null);assert(live,JSON.stringify(capture));assert.equal(live.status.state,'running',JSON.stringify(live));
    const produced=Number(live.status.producedBytes),earliest=Number(live.status.earliestAvailableOffset);assert(produced>=lastProduced);lastProduced=produced;if(earliest>lastEarliest){evictions++;lastEarliest=earliest;}
    const position=await frame.locator('body').evaluate((_,id)=>{const el=[...document.querySelectorAll('#log-lines .log-line')].find(el=>el.dataset.id===id),viewport=document.getElementById('log-viewport');if(!el)throw new Error('Protected record is no longer rendered');return {top:el.getBoundingClientRect().top,relativeTop:el.getBoundingClientRect().top-viewport.getBoundingClientRect().top,scroll:viewport.scrollTop,height:viewport.clientHeight,linesTop:el.parentElement.offsetTop,footerHeight:document.querySelector('.log-footer').getBoundingClientRect().height};},anchor.id);report.latestPosition=position;assert(Math.abs(position.relativeTop-anchor.relativeTop)<=2,'Reading anchor drifted within the viewport without navigation');
    const probe=await metrics();report.actions=probe.actions;report.samples.push({elapsedMs:Date.now()-start,producedBytes:produced,earliestOffset:earliest,retainedBytes:produced-earliest,rows:probe.lastView?.windowTotal,newCount:probe.lastView?.newCount,position,processes:processes()});
    if(++round%3===0){await page.locator('#catalog-tab').click();await delay(30000);await page.locator('#return-workbench').click();await until(()=>frame.locator('#log-status').textContent().then(s=>s==='持续接收'),'Hidden capture did not resume');}
    save();console.log('Log soak',Math.round((Date.now()-start)/1000),'seconds; retained bytes',produced-earliest,'new records',probe.lastView?.newCount);
  }
  assert(lastProduced>initialProduced,'No data arrived while protected');report.checks.push('protected anchor stable during reception and hiding');report.load={elapsedMs:Date.now()-start,producedBytes:lastProduced-initialProduced,evictionObservations:evictions,rotations:(lastProduced-initialProduced)/(32*1024*1024)};
  // Thirty real input-to-painted-result samples, with source size and worker timings kept separate.
  await frame.locator('#log-latest').click();await delay(500);
  const measureSearch=async prefix=>{const samples=[];for(let i=0;i<30;i++){const q=prefix+String(i).padStart(7,'0'),at=performance.now();await frame.locator('#log-query').fill(q);await until(()=>frame.locator('body').evaluate((_,q)=>globalThis.__logProbe.queryPaintText===q,q),'Search did not paint');samples.push(performance.now()-at);}return samples;};
  const queryTimes=await measureSearch('live ');
  await frame.locator('#log-query').fill('');await frame.locator('#log-latest').click();await delay(500);assert(await frame.locator('#log-latest').isHidden());
  const probe=await metrics();const percentile=a=>{a=[...a].sort((x,y)=>x-y);return {samples:a.length,p50:a[Math.floor(a.length*.5)],p95:a[Math.floor(a.length*.95)],max:a.at(-1)};};
  report.performance={firstBodyToPaintMs:probe.firstPaintAt-probe.firstBodyAt,inputToResultMs:percentile(queryTimes),workerQueryMs:percentile(probe.times.query),sourceRows:probe.lastView?.windowTotal,longTasks:probe.longTasks,workerMemory:'not measured',processMemory:'per-process native WorkingSet64 and PrivateMemorySize64'};
  if(ssh.benchmarkId){
    await browserFrame.locator('#query').fill(ssh.benchmarkId);
    await browserFrame.locator(`[data-tree-key="container:${ssh.benchmarkId}"]`).click();await browserFrame.getByRole('button',{name:'日志',exact:true}).click();
    let benchmarkView;await until(async()=>{benchmarkView=(await views()).find(v=>v.definitionKey==='sodeck.docker:logs'&&v.id!==logView)?.id;return !!benchmarkView;},'Benchmark log view did not open');
    frame=page.frameLocator(`.component-surface[data-view-id="${benchmarkView}"] iframe`);await frame.locator('.log-text').first().waitFor();
    const before=await frame.locator('body').evaluate(()=>sodeck.request('event',{action:'logs.status'}));
    await frame.locator('#log-range').selectOption('86400');await until(()=>frame.locator('body').evaluate(async(_,generation)=>{const r=await sodeck.request('event',{action:'logs.status'});return r.generation>generation&&r.resources.length>0&&r.resources.every(r=>r.end!=null&&r.status.finished);},before.generation),'Range preview did not finish');await until(()=>frame.locator('#log-reload').isEnabled(),'Range switch did not finish');await frame.getByText('更多',{exact:true}).click();await frame.locator('#log-complete').click();await frame.getByText('更多',{exact:true}).click();
    await until(()=>frame.locator('body').evaluate(()=>globalThis.__logProbe.lastView?.windowTotal===200000),'Native 200000-row history was not built',120000);
    await until(()=>frame.locator('body').evaluate(async()=>{const r=await sodeck.request('event',{action:'logs.status'});return r.resources.every(r=>r.status.finished);}), 'Complete range did not finish',150000);
    const samples=await measureSearch('INFO '),result=await metrics();report.performance.history200k={inputToResultMs:percentile(samples),workerQueryMs:percentile(result.times.query),rows:result.lastView.windowTotal,longTasks:result.longTasks,processes:processes()};
    report.checks.push('actual native 200000-row bounded history search');
  }
  assert.deepEqual(report.errors,[]);report.checks.push('query/filter/latest native page path');await page.screenshot({path:path.join(reportRoot,'native-logs.png')});
  // Closing a real Surface must release its Worker and command capture before reopening.
  for(const view of (await views()).filter(v=>v.definitionKey==='sodeck.docker:logs'&&v.id!==logView)) {
    await page.locator(`[data-component-action="close"][data-key="${view.id}"]`).click();
    await until(async()=>!(await views()).some(v=>v.id===view.id),'History view did not close');
  }
  const outputRoot=path.join(directory,'command-output'),outputDirectories=()=>fs.existsSync(outputRoot)?fs.readdirSync(outputRoot):[];
  const retainedDirectories=new Set(outputDirectories()),workers=page.workers().length;
  report.reopen=[];
  await browserFrame.locator('body').evaluate(()=>{
    globalThis.__openProbe={clicks:0,errors:[]};
    document.addEventListener('click',e=>{if(e.target.closest('[data-focus="openLogs"]'))__openProbe.clicks++;},true);
    addEventListener('message',e=>{if(['sodeck-ui-v1','sodeck-ui-v2'].includes(e.data?.channel)&&e.data.type==='response'&&e.data.error)__openProbe.errors.push(e.data.error);});
  });
  for(let cycle=0;cycle<reopenCycles;cycle++) {
    await browserFrame.getByRole('button',{name:'日志',exact:true}).click();
    const opened=await browserFrame.locator('body').evaluate(()=>__openProbe);assert.equal(opened.clicks,cycle+1,'Native pointer click was lost during browser refresh: '+JSON.stringify(opened));
    let id;await until(async()=>{id=(await views()).find(v=>v.definitionKey==='sodeck.docker:logs'&&v.id!==logView)?.id;const probe=await browserFrame.locator('body').evaluate(()=>__openProbe);assert.deepEqual(probe.errors,[],'Repeated open returned an error');return !!id;},'Repeated log view did not open');
    frame=page.frameLocator(`.component-surface[data-view-id="${id}"] iframe`);await frame.locator('.log-text').first().waitFor({timeout:45000});
    assert.equal((await metrics()).workerCreated,1,'Each log Surface creates one Worker');
    await frame.getByText('显示',{exact:true}).click();await frame.locator('#log-hold').click();await frame.getByText('显示',{exact:true}).click();
    await page.locator(`[data-component-action="close"][data-key="${id}"]`).click();
    await until(async()=>!(await views()).some(v=>v.id===id)&&await page.locator(`.component-surface[data-view-id="${id}"]`).count()===0,'Closed log view/Surface survived');
    await until(()=>outputDirectories().every(name=>retainedDirectories.has(name))&&page.workers().length<=workers,'Closed log Worker or capture directory survived');
    report.reopen.push({cycle:cycle+1,workers:page.workers().length,captureDirectories:outputDirectories().length});
    report.openProbe=await browserFrame.locator('body').evaluate(()=>__openProbe);save();
  }
  report.checks.push(`${reopenCycles} real log open/hold/close cycles release Surfaces, Workers and capture directories`);
  const pids=(await invoke('snapshot')).plugins.map(p=>p.pid);await invoke('plugin:window|close',{label:'main'});await until(()=>child.exitCode!==null,'Core did not exit');assert.equal(child.exitCode,0);
  await until(()=>pids.every(pid=>{try{process.kill(pid,0);return false;}catch{return true;}}),'Plugin leaked after exit');report.checks.push('native core/plugin exit');
  report.finished=new Date().toISOString();report.result='passed';save();console.log('Native log validation passed:',reportRoot);
}catch(e){report.result='failed';report.error=String(e);report.runtime=runtime;if(frame){report.failureView=await metrics().catch(()=>null);report.failureCapture=await frame.locator('body').evaluate(()=>sodeck.request('event',{action:'logs.status'})).catch(()=>null);report.failureSnapshot=await frame.locator('body').evaluate(()=>sodeck.request('event',{action:'snapshot'})).catch(()=>null);report.failureRows=await frame.locator('#log-lines').evaluate(el=>({rows:[...el.querySelectorAll('.log-line')].map(r=>({id:r.dataset.id,text:r.textContent.slice(0,100),top:r.getBoundingClientRect().top})),scroll:el.parentElement.scrollTop})).catch(()=>null);}await page?.screenshot({path:path.join(reportRoot,'failure.png')}).catch(()=>{});save();console.error('Native log validation failed:',reportRoot);throw e;}
finally{if(child?.exitCode===null){child.kill();await new Promise(r=>child.once('exit',r));}await browser?.close().catch(()=>{});}
