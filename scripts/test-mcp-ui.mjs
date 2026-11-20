import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {createServer} from 'node:http';
import {chromium} from 'playwright';
import {packageMenuAssets} from './plugin-menu-assets.mjs';
import {packageStyles} from '../packages/ui/package.mjs';
import {readTestThemeTokens} from './test-theme-tokens.mjs';
const root=path.resolve('plugins/mcp'), assets=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-mcp-ui-'));
await packageMenuAssets(assets);packageStyles(assets);
fs.copyFileSync('plugins/shared/clipboard.js',path.join(assets,'ui/clipboard-bridge.js'));
const server=createServer((req,res)=>{
  if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><meta charset="UTF-8"><style>body{margin:0;display:flex;height:100vh;font:13px system-ui;background:#f5f5f5}#workspace{flex:1;min-width:0;padding:20px;box-sizing:border-box}#terminal{display:block;width:100%;height:65vh;box-sizing:border-box;background:#202124;color:#eee;padding:16px;font:13px monospace}aside{width:320px;flex:none;display:flex;flex-direction:column;border-left:1px solid #ddd;box-sizing:border-box}#sidebar-title{padding:7px 12px;background:var(--panel,#fff);color:var(--text,#222);border-bottom:1px solid var(--border,#ddd);font-weight:600}iframe{border:0;width:100%;flex:1;min-height:0}</style><main id="workspace"><h2>prod-consul-01 · 终端</h2><p>主工作区保持原有页面和输入</p><textarea id="terminal" aria-label="终端输入"></textarea></main><aside><div id="sidebar-title">Agent MCP</div><iframe title="Agent MCP" src="/ui/index.html"></iframe></aside>');return;}
  const url=req.url.split('?')[0],base=['/ui/text-menu.js','/ui/shared.css','/ui/clipboard-bridge.js'].includes(url)?assets:root,file=path.join(base,url);
  if(!file.startsWith(base+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}
  res.setHeader('Content-Type',{'.js':'text/javascript','.html':'text/html','.css':'text/css'}[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const browser=await chromium.launch({headless:true}),themes=await readTestThemeTokens(browser),page=await browser.newPage({viewport:{width:1040,height:850},recordVideo:{dir:'artifacts/mcp-ui/motion',size:{width:1040,height:850}}});
let longOutput=false,bodyFail=false,holdConfig,holdClipboard,clipboardFail=false,clipboardWrites=0;
const longLines=Array.from({length:24000},(_,i)=>'log-'+i+' nginx request status=200').join('\n').split('\n');
let snapshots=0,configs=0,clipboard='',actions=[],fail=false,targetFail=false,invalidIdentity=false,detailFail=false,hold,holdDetail,holdSettings,taskReads=[];
let settingsSnapshot={pageId:'connection',definitionToken:'test-settings',revision:'1',values:{port:19846}},settingsSaves=[],conflict=false;
const data={runId:'run-a',targetId:'target-a',service:'ready',endpoint:'http://127.0.0.1:19846/mcp',target:{availability:'ready',targetSource:'current',capabilities:['exec','files'],error:null,descriptor:{name:'prod-consul-01',protocol:'ssh',host:'192.0.2.11',port:22,endpoint:'192.0.2.11:22',username:'ops'}},evidenceDirectory:'/home/ops/.local/share/sodeck/plugin-data/sodeck.mcp/evidence',tasks:[{taskId:'t1',kind:'exec',requestId:'collect-consul-logs',native:{state:'running'},done:false,evidence:{complete:false}},{taskId:'t2',kind:'download',requestId:'consul-evidence',native:{state:'completed'},done:true,evidence:{complete:true}},{taskId:'t3',kind:'exec',requestId:'inspect-network',native:{state:'failed',exitCode:7},done:true,evidence:{complete:false,error:{message:'输出超出额度，请导出文件后下载。'}}}]};
const errors=[];page.on('pageerror',e=>errors.push(e.message));
Object.assign(data.tasks[0],{input:{command:"journalctl -u consul --since '15 minutes ago' --no-pager"},createdAt:Date.now()-12000});
Object.assign(data.tasks[1],{input:{remotePath:'/tmp/consul-evidence.log'},createdAt:Date.now()-20000,finishedAt:Date.now()-15000,native:{state:'completed',result:{size:'25190'}},evidence:{complete:true,path:'/evidence/t2/download.bin'}});
Object.assign(data.tasks[2],{input:{command:'printf "<img src=x onerror=alert(1)>"\n'+'x'.repeat(16000)},createdAt:Date.now()-40000,finishedAt:Date.now()-35000});
for(const task of data.tasks){task.phase=task.done?'done':'collecting';task.evidence.state=task.done?'saved':'collecting';task.targetId=data.targetId;task.runId=data.runId;task.revision='1';task.outputInfo={savedBytes:'200'};task.target=structuredClone(data.target.descriptor);}
function summary(task){const {input,directory,...row}=structuredClone(task);const value=input?.command??input?.remotePath??task.requestId;row.inputPreview=value.slice(0,256);row.inputTruncated=value.length>256;if(row.evidence)delete row.evidence.path;return row;}
const records=new Map(data.tasks.map(task=>[task.taskId,task]));
await page.exposeFunction('mcpRequest',async(method,p)=>{
  if(method==='openView'){actions.push(p);throw new Error('MCP must not open a central page');}
  if(method==='settings'){
    if(p.op==='get')return structuredClone(settingsSnapshot);
    if(p.op==='save'){settingsSaves.push(p);if(holdSettings)await new Promise(r=>{holdSettings=r;});if(conflict){settingsSnapshot={...settingsSnapshot,revision:'2',values:{port:19849}};return {persisted:false,conflict:true,snapshot:structuredClone(settingsSnapshot)};}
      assert.equal(p.baseRevision,settingsSnapshot.revision);settingsSnapshot={...settingsSnapshot,revision:String(Number(settingsSnapshot.revision)+1),values:p.values};return {persisted:true,snapshot:structuredClone(settingsSnapshot)};}
  }
  if(method==='clipboard'){if(p.op==='beginWrite'){clipboardWrites++;if(clipboardFail)throw new Error('clipboard unavailable');if(holdClipboard)await new Promise(r=>{holdClipboard=r;});clipboard='';return {id:'clip'};}if(p.op==='writeChunk'){clipboard+=p.text;return {};}return {};}
  if(method==='data'){actions.push(p);return {};}
  actions.push(p.action);
  if(p.action==='config'){configs++;if(holdConfig)await new Promise(r=>{holdConfig=r;});return {token:'illustrative-secret',config:'[mcp_servers.sodeck]\nurl = "http://127.0.0.1:19846/mcp"\nhttp_headers = { Authorization = "Bearer illustrative-secret" }'};}
  if(p.action==='cancel')return {cancelRequested:true};
  if(p.action==='task'){
    if(detailFail || (bodyFail && p.outputMode!=='none'))throw new Error('详情超过界面传输大小，请使用本地证据或 MCP 读取');
    taskReads.push(p);const task=structuredClone(data.tasks.find(task=>task.taskId===p.taskId)||records.get(p.taskId));
    if(holdDetail&&p.taskId==='t3'&&p.outputMode!=='none'){const resolve=holdDetail;holdDetail=null;await new Promise(r=>resolve(r));}
    if(task.kind==='download')task.file={remotePath:task.input.remotePath,localPath:task.done&&task.evidence.complete?task.evidence.path:null};
    if(task.kind==='exec'&&p.outputMode!=='none')task.output={savedBytes:task.outputInfo.savedBytes,stdout:p.tail?(longOutput?longLines.slice(-120).join('\n'):'latest output'):p.cursor==='100'?'second page':'first page <img src=x onerror=alert(1)>',stderr:p.tail?'':'stderr evidence',nextCursor:p.cursor==='100'?'200':'100',hasMore:!p.tail&&p.cursor!=='100'};
    return task;
  }
  if(p.action==='snapshot'){snapshots++;if(hold)await new Promise(r=>{hold=r;});if(fail)throw new Error('核心暂不可用，请稍后刷新。');}
  const projected={...structuredClone(data),target:targetFail?{descriptor:null,targetSource:'unknown',availability:'unavailable',capabilities:null,error:{code:-32000,message:'目标摘要暂时读取失败'}}:structuredClone(data.target),tasks:data.tasks.map(summary)};
  if(invalidIdentity)delete projected.targetId;
  return projected;
});
await page.addInitScript(()=>{
  if(window!==top)return;
  addEventListener('message',async e=>{const m=e.data;if(m?.channel!=='sodeck-ui-v2')return;
    if(m.type==='ready')e.source.postMessage({channel:m.channel,type:'mount',surfaceId:'test',viewType:'status',visible:true,capabilities:{clipboard:1,settings:1},tokens:{}},'*');
    if(m.type==='request')try{const data=await window.mcpRequest(m.method,m.params);e.source.postMessage({channel:m.channel,type:'response',surfaceId:'test',id:m.id,data},'*');}catch(error){e.source.postMessage({channel:m.channel,type:'response',surfaceId:'test',id:m.id,error:error.message},'*');}
  });
});
const frame=page.frameLocator('iframe');
async function setSize(width,height){await page.setViewportSize({width:width+720,height});await page.locator('aside').evaluate((el,w)=>el.style.width=w+'px',width);}
async function overview(){if(await frame.locator('#detail').isVisible())await frame.locator('#detail-close').click();}
async function choose(name){await overview();await frame.getByRole('button',{name}).click();}
async function openSettings(){await overview();if(!await frame.locator('#settings').evaluate(el=>el.open))await frame.locator('#settings>summary').click();}
const message=(type,extra={})=>page.evaluate(m=>{if(m.type==='themeChanged')for(const [key,value]of Object.entries(m.tokens))document.documentElement.style.setProperty('--'+key,value);document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'test',...m},'*');},{type,...extra});
async function capture(options){await frame.locator('main').evaluate(async el=>{await Promise.all(el.getAnimations({subtree:true}).map(a=>a.finished.catch(()=>{})));});await page.screenshot(options);}
fs.mkdirSync('artifacts/mcp-ui',{recursive:true});
try {
  await page.goto('http://127.0.0.1:'+server.address().port);
  await frame.locator('#server').filter({hasText:'prod-consul-01'}).waitFor();
  await message('themeChanged',{tokens:themes.light});
  assert.equal(configs,0,'ordinary polling never reads secrets');
  assert.equal(await frame.locator('#tasks>.record').count(),3);
  assert.equal(await frame.locator('#tasks .outgoing').count(),3);
  assert.equal(await frame.locator('#tasks .incoming').count(),3);
  assert.equal(await frame.locator('#monitor').count(),0,'the duplicate live monitor is removed');
  assert.equal(await frame.locator('h1').count(),0);
  assert(await frame.locator('#detail').isHidden());
  const originalInput=data.tasks[0].input.command;
  data.tasks[0].input.command='line\n'.repeat(60);data.tasks[0].revision=String(BigInt(data.tasks[0].revision)+1n);await frame.locator('#refresh').click();
  await frame.locator('[data-task-id="t1"] .command').filter({hasText:'line'}).waitFor();
  assert((await frame.locator('[data-task-id="t1"] .command').boundingBox()).height<=59,'request bubbles clamp multiline input to three lines');
  await capture({path:'artifacts/mcp-ui/multiline-command.png',fullPage:true});
  data.tasks[0].native={state:'opening',execSent:true,launchQueued:true,launchAccepted:false};data.tasks[0].revision=String(BigInt(data.tasks[0].revision)+1n);await frame.locator('#refresh').click();
  await frame.locator('[data-task-id="t1"] .steps').filter({hasText:'等待远端确认'}).waitFor();
  await capture({path:'artifacts/mcp-ui/pending-admission.png',fullPage:true});data.tasks[0].native={state:'running'};
  data.tasks[0].input.command=originalInput;data.tasks[0].revision=String(BigInt(data.tasks[0].revision)+1n);await frame.locator('#refresh').click();
  await page.locator('#terminal').fill('primary workspace draft');await page.locator('#terminal').focus();
  await page.waitForTimeout(1700);
  assert.equal(await page.locator(':focus').getAttribute('id'),'terminal');
  assert(taskReads.filter(p=>p.tail).length<=4,'the initial viewport has a bounded preview batch');
  await choose('查看命令详情 collect-consul-logs');
  await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();
  assert.equal(await frame.locator('#detail-output img').count(),0);
  await frame.getByRole('button',{name:'下一页',exact:true}).click();
  await frame.locator('#detail-output').filter({hasText:'second page'}).waitFor();
  await frame.locator('#detail-output').evaluate(el=>el.scrollTop=12);
  await frame.locator('#previous').focus();
  const frozen=await frame.locator('#detail-output').textContent(),scroll=await frame.locator('#detail-output').evaluate(el=>el.scrollTop),reads=taskReads.length;
  data.tasks[0].revision=String(BigInt(data.tasks[0].revision)+1n);data.tasks[0].outputInfo.savedBytes='300';
  await frame.locator('#refresh').click();await frame.locator('#output-new').waitFor();
  assert.equal(await frame.locator('#detail-output').textContent(),frozen);
  assert.equal(await frame.locator('#detail-output').evaluate(el=>el.scrollTop),scroll);
  assert(taskReads.slice(reads).every(p=>p.outputMode==='none'),'polling reads facts rather than the frozen body');
  await frame.locator('#latest').click();await frame.locator('#detail-output').filter({hasText:'latest output'}).waitFor();
  assert.equal(await frame.locator('#page-number').textContent(),'最近一页');
  assert(await frame.locator('#next').isDisabled());
  await frame.getByRole('button',{name:'从头查看'}).click();await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();
  bodyFail=true;await frame.locator('#next').click();await frame.locator('#output-error').waitFor();
  assert((await frame.locator('#detail-output').textContent()).includes('first page'));
  assert.equal(await frame.locator('#page-number').textContent(),'第 1 页','failed pages do not advance the cursor');
  bodyFail=false;await frame.locator('#output-retry').click();await frame.locator('#detail-output').filter({hasText:'second page'}).waitFor();
  targetFail=true;await frame.locator('#refresh').click();await frame.locator('#service').filter({hasText:'目标摘要暂时读取失败'}).waitFor();
  assert(await frame.locator('#detail').isVisible());assert((await frame.locator('#detail-output').textContent()).includes('second page'));
  targetFail=false;await frame.locator('#refresh').click();await frame.locator('#service').waitFor({state:'hidden'});
  invalidIdentity=true;await frame.locator('#refresh').click();await frame.locator('#notice').filter({hasText:'快照无效'}).waitFor();
  assert(await frame.locator('#detail').isVisible());invalidIdentity=false;
  await frame.locator('#refresh').click();await frame.locator('#freshness').waitFor({state:'hidden'});
  await frame.locator('#detail-cancel').click();await frame.locator('#notice').filter({hasText:'等待远端实际结果'}).waitFor();
  assert(actions.includes('cancel'));
  data.tasks[0].cancelRequested=true;data.tasks[0].revision=String(BigInt(data.tasks[0].revision)+1n);await frame.locator('#refresh').click();await frame.locator('#detail-cancel').waitFor({state:'hidden'});
  data.tasks[0].cancelRequested=false;data.tasks[0].phase='finalizing';data.tasks[0].revision=String(BigInt(data.tasks[0].revision)+1n);await frame.locator('#refresh').click();await frame.locator('#detail-state').filter({hasText:'收尾'}).waitFor();
  delete data.tasks[0].phase;data.tasks[0].revision=String(BigInt(data.tasks[0].revision)+1n);
  await choose('查看命令详情 inspect-network');await frame.locator('#detail-command').filter({hasText:'printf'}).waitFor();
  assert.equal(await frame.locator('#detail-command').textContent(),records.get('t3').input.command);
  assert((await frame.locator('#detail-state').textContent()).includes('退出码 7'));assert(await frame.locator('#detail-evidence').isVisible());
  await capture({path:'artifacts/mcp-ui/failed-command.png',fullPage:true});
  const signalTask=data.tasks.find(t=>t.taskId==='t3'),savedNative=signalTask.native;signalTask.native={state:'completed',exitSignal:'TERM'};signalTask.revision='2';
  await frame.locator('#refresh').click();await frame.locator('#detail-state').filter({hasText:'退出信号 TERM'}).waitFor();
  await capture({path:'artifacts/mcp-ui/signal-exit.png',fullPage:true});signalTask.native=savedNative;signalTask.revision='3';
  const savedEvidence=signalTask.evidence;
  signalTask.native={state:'completed',exitCode:0};signalTask.evidence={state:'failed',complete:false,error:{message:'磁盘已满'}};signalTask.cleanupError={message:'通道未释放'};signalTask.revision='9007199254740993';
  await frame.locator('#refresh').click();await frame.locator('#detail-state').filter({hasText:'已结束 · 退出码 0'}).waitFor();
  assert.equal(await frame.locator('#detail-state').getAttribute('data-tone'),'normal');assert(await frame.locator('#detail-error').isHidden());
  assert((await frame.locator('#detail-evidence').textContent()).includes('磁盘已满'));assert((await frame.locator('#detail-cleanup').textContent()).includes('通道未释放'));
  await frame.locator('#evidence-details>summary').click();await frame.locator('#detail-output').focus();
  await frame.locator('#detail-output').evaluate(el=>{const range=document.createRange();range.selectNodeContents(el);getSelection().removeAllRanges();getSelection().addRange(range);});
  const selection=await frame.locator('#detail-output').evaluate(()=>getSelection().toString());
  signalTask.revision='9007199254740992';signalTask.native={state:'running'};signalTask.evidence={complete:true};
  await frame.locator('#refresh').click();await page.waitForTimeout(100);
  assert((await frame.locator('#detail-state').textContent()).includes('退出码 0'),'older revisions above Number precision cannot replace accepted facts');
  assert(await frame.locator('#evidence-details').evaluate(el=>el.open));assert.equal(await frame.locator('#detail-command').textContent(),records.get('t3').input.command,'summary polling retains the complete input');
  assert.equal(await frame.locator('#detail-output').evaluate(()=>getSelection().toString()),selection,'fact updates preserve the frozen log selection');
  await overview();const issueRow=frame.locator('[data-task-id="t3"]');
  assert.equal(await issueRow.getAttribute('data-tone'),'normal');assert((await issueRow.textContent()).includes('磁盘已满'));assert((await issueRow.textContent()).includes('通道未释放'));
  assert.equal(await issueRow.locator('time').count(),1);assert.equal((await issueRow.textContent()).split('证据保存失败').length-1,1);
  await issueRow.scrollIntoViewIfNeeded();
  await capture({path:'artifacts/mcp-ui/independent-save-cleanup.png',fullPage:true});
  signalTask.native=savedNative;signalTask.evidence=savedEvidence;delete signalTask.cleanupError;signalTask.revision='9007199254740994';
  await frame.locator('#refresh').click();await choose('查看命令详情 inspect-network');
  const selected=data.tasks.find(task=>task.taskId==='t3');
  data.tasks=data.tasks.filter(task=>task.taskId!=='t3');
  await frame.locator('#refresh').click();await frame.locator('#detail-request').filter({hasText:'inspect-network'}).waitFor({state:'attached'});
  assert(await frame.locator('#detail').isVisible(),'details remain readable outside the recent selection');
  await frame.locator('#detail-close').click();assert.equal(await frame.locator(':focus').getAttribute('id'),'refresh');
  assert.equal(await frame.locator('#tasks>.record').count(),2,'pinned details do not become an extra recent row');
  data.tasks.push(selected);await frame.locator('#refresh').click();
  await choose('查看下载详情 consul-evidence');await frame.locator('#file-local').filter({hasText:'/evidence/t2/download.bin'}).waitFor();
  assert(await frame.locator('#detail-output-section').isHidden());assert.equal(await frame.locator('#file-name').textContent(),'consul-evidence.log');
  assert.equal(await frame.locator('#file-path-label').textContent(),'保存位置');
  assert((await frame.locator('#file-summary').textContent()).includes('24.6 KiB'),'file receipt formats the existing native size');
  await frame.locator('#evidence-details>summary').click();assert.equal(await frame.locator('#detail-size').textContent(),'25190 字节');await frame.locator('#evidence-details>summary').click();
  const beforeDownloadResume=taskReads.length;await message('visibilityChanged',{visible:false});await message('visibilityChanged',{visible:true});
  await frame.locator('#refresh').click();await page.waitForTimeout(100);
  assert(taskReads.slice(beforeDownloadResume).every(p=>p.outputMode==='none'),'download resume only reads facts');
  data.tasks.find(t=>t.taskId==='t2').evidence.complete=false;data.tasks.find(t=>t.taskId==='t2').revision='2';
  await frame.locator('#refresh').click();await frame.locator('#file-path-label').filter({hasText:'证据或残留位置'}).waitFor();
  await capture({path:'artifacts/mcp-ui/failed-download.png',fullPage:true});
  data.tasks.find(t=>t.taskId==='t2').evidence.complete=true;data.tasks.find(t=>t.taskId==='t2').revision='3';
  await overview();
  let late;holdDetail=resolve=>{late=resolve;};await choose('查看命令详情 inspect-network');
  await page.waitForFunction(()=>true);while(!late)await page.waitForTimeout(10);
  await choose('查看下载详情 consul-evidence');late();await page.waitForTimeout(100);
  assert.equal(await frame.locator('#detail-request').textContent(),'consul-evidence');
  await overview();await frame.locator('#config-toggle').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  await frame.locator('#config-popover').evaluate(async el=>{await Promise.all(el.getAnimations().map(a=>a.finished.catch(()=>{})));});
  const copyWidth=(await frame.locator('#copy').boundingBox()).width;
  holdClipboard=true;const writesBefore=clipboardWrites;await frame.locator('#copy').click();await frame.locator('#copy').filter({hasText:'复制中…'}).waitFor();
  assert.equal((await frame.locator('#copy').boundingBox()).width,copyWidth);assert.equal(await frame.locator(':focus').getAttribute('id'),'copy');
  while(typeof holdClipboard!=='function')await page.waitForTimeout(10);
  await frame.locator('#copy').evaluate(el=>el.click());assert.equal(clipboardWrites,writesBefore+1,'busy copy cannot start another write');
  const releaseClipboard=holdClipboard;holdClipboard=null;releaseClipboard();
  await frame.locator('#copy').filter({hasText:'已复制'}).waitFor();assert.equal((await frame.locator('#copy').boundingBox()).width,copyWidth);
  clipboardFail=true;await frame.locator('#copy').click();await frame.locator('#copy').filter({hasText:'重试复制'}).waitFor();
  assert.equal((await frame.locator('#copy').boundingBox()).width,copyWidth);assert(await frame.locator('#config-notice').isVisible());clipboardFail=false;
  await frame.locator('#copy').click();await frame.locator('#copy').filter({hasText:'已复制'}).waitFor();assert(clipboard.includes('illustrative-secret'));
  // Interrupt the exit in the same frame: no old animation may hide a reopened popup.
  await frame.locator('#config-toggle').evaluate(el=>{el.click();if(document.getElementById('config').textContent!=='')throw new Error('secret retained while closing');el.click();});
  await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();await page.waitForTimeout(200);assert(await frame.locator('#config-popover').isVisible());
  assert.equal(await frame.locator('#config-popover').evaluate(el=>el.style.height),'');
  holdClipboard=true;await frame.locator('#copy').click();while(typeof holdClipboard!=='function')await page.waitForTimeout(10);
  await frame.locator('#config-toggle').evaluate(el=>{el.click();el.click();});await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  const staleClipboard=holdClipboard;holdClipboard=null;staleClipboard();await page.waitForTimeout(100);assert.equal(await frame.locator('#copy').textContent(),'复制','old copy result cannot mark a new open as copied');
  await frame.locator('#config-toggle').evaluate(el=>{el.click();el.click();});await message('themeChanged',{tokens:themes.dark});
  assert.equal(await frame.locator('#config-popover').evaluate(el=>el.getAnimations().length),0);assert.equal(await frame.locator('#config-popover').evaluate(el=>el.style.height),'');
  await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  await frame.locator('#config-toggle').evaluate(el=>el.click());await page.emulateMedia({reducedMotion:'reduce'});
  await frame.locator('#config-popover').waitFor({state:'hidden'});assert.equal(await frame.locator('#config-popover').evaluate(el=>el.getAnimations().length),0);
  await page.emulateMedia({reducedMotion:'no-preference'});await frame.locator('#config-toggle').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  await frame.locator('#settings>summary').click();await frame.locator('#show-token').click();await frame.locator('#access-token').filter({hasText:'illustrative-secret'}).waitFor();
  await frame.locator('#port').fill('1023');await frame.locator('#save-port').click();assert.equal(settingsSaves.length,0);
  await frame.locator('#port').fill('19847');await frame.locator('#save-port').click();await frame.locator('#port-notice').filter({hasText:'重启'}).waitFor();
  assert.equal(settingsSnapshot.values.port,19847);assert((await frame.locator('#endpoint').textContent()).includes('19846'));
  conflict=true;await frame.locator('#port').fill('19848');await frame.locator('#save-port').click();await frame.locator('#port-notice').filter({hasText:'草稿已保留'}).waitFor();
  assert.equal(await frame.locator('#port').inputValue(),'19848');assert(await frame.locator('#save-port').isDisabled());await frame.locator('#reload-port').click();assert.equal(await frame.locator('#port').inputValue(),'19849');conflict=false;
  await frame.locator('#port').fill('19850');
  await frame.locator('#config').press('Escape');
  assert.equal(await frame.locator('#config-popover').getAttribute('aria-hidden'),'true');assert(await frame.locator('#config-popover').evaluate(el=>el.inert));
  assert.equal(await frame.locator('#config').textContent(),'','closing clears the secret in the same interaction, before visual exit');
  await frame.locator('#config-popover').waitFor({state:'hidden'});
  assert.equal(await frame.locator('#config').textContent(),'');assert(!(await frame.locator('#access-token').textContent()).includes('illustrative-secret'));assert.equal(await frame.locator(':focus').getAttribute('id'),'config-toggle');
  await frame.locator('#config-toggle').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();await frame.locator('#settings>summary').click();assert.equal(await frame.locator('#port').inputValue(),'19850','closing the popover preserves the unsaved port draft');
  await frame.locator('#reload-port').evaluate(el=>el.click());await frame.locator('#config-toggle').click();
  // Hiding a pending configuration cannot refill it after the view returns.
  holdConfig=true;await frame.locator('#config-toggle').click();while(typeof holdConfig!=='function')await page.waitForTimeout(10);
  await message('visibilityChanged',{visible:false});const releaseConfig=holdConfig;holdConfig=null;releaseConfig();
  await page.waitForTimeout(100);assert.equal(await frame.locator('#config').textContent(),'');
  const hidden=snapshots;await page.waitForTimeout(1700);assert.equal(snapshots,hidden);
  await message('visibilityChanged',{visible:true});await frame.locator('#refresh').click();
  await choose('查看命令详情 collect-consul-logs');
  longOutput=true;await frame.locator('#latest').click();await frame.locator('#detail-output').filter({hasText:'log-23999'}).waitFor();
  assert((await frame.locator('#detail-output').textContent()).length<16384,'only one page of the 24,000-line fixture enters the DOM');
  assert((await frame.locator('#detail-output').boundingBox()).height<=261);
  longOutput=false;
  const originalTasks=data.tasks;
  data.recordCount=80;data.tasks=Array.from({length:50},(_,i)=>({...structuredClone(originalTasks[0]),taskId:'many-'+i,requestId:'many-'+i,createdAt:Date.now()-i*1000,revision:'1'}));
  await overview();await frame.locator('#refresh').click();await frame.locator('#task-count').filter({hasText:'50 / 80'}).waitFor();
  await frame.locator('#content').evaluate(el=>el.scrollTop=300);await frame.locator('#tasks>.record').nth(4).scrollIntoViewIfNeeded();
  const beforeNew=await frame.locator('#content').evaluate(el=>el.scrollTop);
  data.tasks.push({...structuredClone(originalTasks[0]),taskId:'new-row',requestId:'new-row',createdAt:Date.now()+1000});
  data.tasks.shift();await frame.locator('#refresh').click();await frame.locator('#new-tasks').waitFor();
  assert(Math.abs((await frame.locator('#content').evaluate(el=>el.scrollTop))-beforeNew)<5,'new messages preserve the reading anchor');
  await capture({path:'artifacts/mcp-ui/new-operations.png',fullPage:true});
  const row=frame.locator('#tasks>.record').nth(4),rowId=await row.getAttribute('data-task-id');
  const rowNode=await row.elementHandle();data.tasks.find(t=>t.taskId===rowId).done=true;
  await frame.locator('#refresh').click();assert.equal(await rowNode.evaluate(el=>el.isConnected),true,'state changes preserve the task DOM node');
  await frame.locator('#new-tasks').click();assert(await frame.locator('#new-tasks').isHidden());
  data.tasks=originalTasks;delete data.recordCount;await frame.locator('#refresh').click();
  // Capture the supported layouts in one batch after exercising interactions.
  for(const [name,tokens] of Object.entries(themes))for(const width of [220,280,390]){
    await setSize(width,740);await message('themeChanged',{tokens});
    await page.waitForFunction(canvas=>document.querySelector('iframe').contentDocument.documentElement.style.getPropertyValue('--canvas')===canvas,tokens.canvas);
    await overview();await frame.locator('#content').evaluate(el=>el.scrollTop=el.scrollHeight);
    assert(await frame.locator('body').evaluate(el=>el.scrollWidth<=innerWidth+1));
    await capture({path:'artifacts/mcp-ui/'+name+'-'+width+'.png',fullPage:true});
    await choose('查看命令详情 collect-consul-logs');await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();
    assert(await frame.locator('body').evaluate(el=>el.scrollWidth<=innerWidth+1));
    if(width===280)await capture({path:'artifacts/mcp-ui/'+name+'-command.png',fullPage:true});
    await choose('查看下载详情 consul-evidence');await frame.locator('#file-local').filter({hasText:'download.bin'}).waitFor();
    if(width===280)await capture({path:'artifacts/mcp-ui/'+name+'-file.png',fullPage:true});
  }
  await overview();await setSize(220,380);await page.emulateMedia({reducedMotion:'reduce'});
  await frame.locator('#config-toggle').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  const box=await frame.locator('#config-popover').boundingBox(),iframeBox=await page.locator('iframe').boundingBox();assert(box.x>=iframeBox.x && box.x+box.width<=iframeBox.x+iframeBox.width+1);assert(box.y+box.height<=iframeBox.y+iframeBox.height+1);
  await capture({path:'artifacts/mcp-ui/config-short.png',fullPage:true});
  await frame.locator('#config-toggle').click();
  fail=true;await frame.locator('#refresh').click();await frame.locator('#notice').filter({hasText:'核心暂不可用'}).waitFor();fail=false;
  await choose('查看命令详情 inspect-network');await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();
  let oldReply;holdDetail=resolve=>{oldReply=resolve;};await frame.locator('#next').click();while(!oldReply)await page.waitForTimeout(10);
  data.targetId='target-b';data.tasks=[{...structuredClone(originalTasks[0]),taskId:'new-target',requestId:'new-target',targetId:'target-b'}];
  await frame.locator('#refresh').click();await frame.locator('#detail').waitFor({state:'hidden'});
  await choose('查看命令详情 new-target');await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();oldReply();await page.waitForTimeout(100);
  assert.equal(await frame.locator('#detail-request').textContent(),'new-target');assert.equal(await frame.locator('#page-number').textContent(),'第 1 页');
  data.runId='run-b';await frame.locator('#refresh').click();await frame.locator('#detail').waitFor({state:'hidden'});
  data.tasks=[];await frame.locator('#refresh').click();await frame.locator('#empty').waitFor();
  await capture({path:'artifacts/mcp-ui/empty.png',fullPage:true});
  const upload={...structuredClone(originalTasks[1]),taskId:'upload',kind:'upload',revision:'1',targetId:data.targetId,runId:data.runId,requestId:'upload-file',input:{localPath:'C:/work/脚本.bin',remotePath:'/tmp/脚本.bin',overwrite:true},native:{state:'committing',received:'100',total:'100',transfer:{commitState:'pending'}},file:{localPath:'C:/work/脚本.bin',remotePath:'/tmp/脚本.bin',overwrite:true,commitState:'pending'},done:false};
  data.tasks=[upload];await frame.locator('#refresh').click();await choose('查看上传详情 upload-file');await frame.locator('#file-summary').filter({hasText:'正在提交'}).waitFor();assert(await frame.locator('#detail-output-section').isHidden());assert.equal(await frame.locator('#file-local').textContent(),upload.input.localPath);
  const uploadReads=taskReads.length;await page.waitForTimeout(1700);assert(taskReads.slice(uploadReads).every(p=>p.outputMode==='none'),'uploads never read command output');
  upload.done=true;upload.native.state='completed';upload.native.transfer.commitState='committed';upload.file.commitState='committed';upload.evidence={state:'failed',complete:false,error:{message:'record disk failed'}};upload.revision='2';await frame.locator('#refresh').click();await frame.locator('#file-summary').filter({hasText:'已上传'}).waitFor();assert((await frame.locator('#file-summary').textContent()).includes('证据保存失败'));await capture({path:'artifacts/mcp-ui/upload-committed-record-failed.png',fullPage:true});
  upload.native.state='unknown';upload.native.transfer.commitState='unknown';upload.file.commitState='unknown';upload.revision='3';await frame.locator('#refresh').click();await frame.locator('#file-summary').filter({hasText:'结果待核对'}).waitFor();
  await message('unmount');const unmounted=snapshots;await page.waitForTimeout(1700);assert.equal(snapshots,unmounted);
  assert.equal(await page.locator('#terminal').inputValue(),'primary workspace draft');
  assert(!actions.some(p=>p?.viewType));assert.deepEqual(errors,[]);
  console.log('MCP task conversation: identity, bounded previews, frozen pages, retry, late replies, configuration, port conflicts, three themes, narrow layouts and lifecycle passed');
} finally {
  await browser.close();await new Promise(r=>server.close(r));assert.equal(path.dirname(assets),os.tmpdir());fs.rmSync(assets,{recursive:true,force:true});
}
