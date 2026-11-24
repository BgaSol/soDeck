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
const duration=Number(process.env.SODECK_LOG_DURATION_MS||60000);
const reopenCycles=Number(process.env.SODECK_LOG_REOPEN_CYCLES||20);
assert(Number.isSafeInteger(duration)&&duration>=1000&&duration<=5400000);
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
  await browser.contexts()[0].addInitScript(()=>{globalThis.__logProbe={reads:[],actions:[],longTasks:[],firstBodyAt:null,firstPaintAt:null};try{new PerformanceObserver(list=>{for(const e of list.getEntries())if(__logProbe.longTasks.length<1000)__logProbe.longTasks.push(e.duration);}).observe({type:'longtask',buffered:true});}catch{}addEventListener('sodeck:mount',()=>{const original=sodeck.request.bind(sodeck);sodeck.request=async(method,query)=>{const at=performance.now();if(method==='event')__logProbe.actions.push(query);const result=await original(method,query);if(method==='read'){if(__logProbe.reads.length<1000)__logProbe.reads.push({op:query.op,ms:performance.now()-at});if(query.op==='window'&&result.rows?.length&&__logProbe.firstBodyAt===null)__logProbe.firstBodyAt=performance.now();}return result;};});new MutationObserver(()=>{if(__logProbe.firstBodyAt!==null&&__logProbe.firstPaintAt===null&&document.querySelector('.log-text'))__logProbe.firstPaintAt=performance.now();}).observe(document,{childList:true,subtree:true});});
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
  await frame.locator('.log-text').first().waitFor({timeout:45000});
  const logRead=query=>frame.locator('body').evaluate((_,query)=>sodeck.request('read',query),query);
  const logEvent=data=>frame.locator('body').evaluate((_,data)=>sodeck.request('event',data),data);
  await until(async()=>(await logRead({op:'status'})).receiving,'Native follow did not start');report.checks.push('actual WebView2/CSP/read bridge/plugin/SSH/Engine body receipt');
  const initial=await logRead({op:'status'});report.initialView=initial;
  await logEvent({action:'logs.hold',revision:initial.revision});await delay(300);
  const anchor=await frame.locator('#log-lines').evaluate(root=>{const top=root.parentElement.getBoundingClientRect().top,n=[...root.querySelectorAll('.log-line')].find(n=>n.getBoundingClientRect().bottom>top);return{id:n.dataset.id,pixel:top-n.getBoundingClientRect().top};});report.anchor=anchor;
  const end=Date.now()+duration;while(Date.now()<end){await delay(Math.min(5000,end-Date.now()));const state=await logRead({op:'status'});const actual=await frame.locator('#log-lines').evaluate(root=>{const top=root.parentElement.getBoundingClientRect().top,n=[...root.querySelectorAll('.log-line')].find(n=>n.getBoundingClientRect().bottom>top);return{id:n.dataset.id,pixel:top-n.getBoundingClientRect().top};});assert.equal(actual.id,anchor.id);assert(Math.abs(actual.pixel-anchor.pixel)<2);report.samples.push({at:new Date().toISOString(),newCount:state.newCount,budgetBytes:state.budgetBytes,processes:processes()});save();}
  report.checks.push('native held reading survives ongoing input with bounded session data');
  await frame.locator('#log-query').fill('INFO');await frame.locator('#log-matches').filter({hasText:/[1-9].*条匹配/}).waitFor();await frame.locator('#log-query').fill('不存在的关键词');await frame.locator('#log-matches').filter({hasText:'0 条匹配'}).waitFor();await frame.locator('#log-query').fill('');await frame.locator('#log-latest').click();await until(async()=>(await logRead({op:'status'})).following,'Native return to latest failed');
  report.performance=await metrics();report.checks.push('native literal query, zero-result scope and actual latest follow');await page.screenshot({path:path.join(reportRoot,'native-logs.png')});
  report.reopen=[];const baseline=(await logRead({op:'status'})).budgetBytes;
  await page.locator(`[data-component-action="close"][data-key="${logView}"]`).click();await until(async()=>!(await views()).some(v=>v.id===logView),'Initial log view did not close');
  for(let cycle=0;cycle<reopenCycles;cycle++){
    await browserFrame.getByRole('button',{name:'日志',exact:true}).click();let next;await until(async()=>{next=(await views()).find(v=>v.definitionKey==='sodeck.docker:logs')?.id;return!!next;},'Repeated log view did not open');
    logView=next;frame=page.frameLocator(`.component-surface[data-view-id="${next}"] iframe`);await frame.locator('.log-text').first().waitFor({timeout:45000});const current=await logRead({op:'status'});assert(current.budgetBytes<=baseline+8*1024*1024,'Old session records survived closing');await logEvent({action:'logs.hold',revision:current.revision});
    await page.locator(`[data-component-action="close"][data-key="${next}"]`).click();await until(async()=>!(await views()).some(v=>v.id===next),'Closed View survived');report.reopen.push({cycle:cycle+1,budgetBytes:current.budgetBytes});save();
  }
  report.checks.push(`${reopenCycles} native open/hold/close cycles release View session data`);
  const pids=(await invoke('snapshot')).plugins.map(p=>p.pid);await invoke('plugin:window|close',{label:'main'});await until(()=>child.exitCode!==null,'Core did not exit');assert.equal(child.exitCode,0);
  await until(()=>pids.every(pid=>{try{process.kill(pid,0);return false;}catch{return true;}}),'Plugin leaked after exit');report.checks.push('native core/plugin exit');
  report.finished=new Date().toISOString();report.result='passed';save();console.log('Native log validation passed:',reportRoot);
}catch(e){report.result='failed';report.error=String(e);report.runtime=runtime;if(frame){report.failureView=await metrics().catch(()=>null);report.failureCapture=await frame.locator('body').evaluate(()=>sodeck.request('read',{op:'status'})).catch(()=>null);report.failureSnapshot=await frame.locator('body').evaluate(()=>sodeck.request('event',{action:'snapshot'})).catch(()=>null);report.failureRows=await frame.locator('#log-lines').evaluate(el=>({rows:[...el.querySelectorAll('.log-line')].map(r=>({id:r.dataset.id,text:r.textContent.slice(0,100),top:r.getBoundingClientRect().top})),scroll:el.parentElement.scrollTop})).catch(()=>null);}await page?.screenshot({path:path.join(reportRoot,'failure.png')}).catch(()=>{});save();console.error('Native log validation failed:',reportRoot);throw e;}
finally{if(child?.exitCode===null){child.kill();await new Promise(r=>child.once('exit',r));}await browser?.close().catch(()=>{});}
