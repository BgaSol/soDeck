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
const browser=await chromium.launch({headless:true}),themes=await readTestThemeTokens(browser),page=await browser.newPage({viewport:{width:1040,height:850}});
let snapshots=0,configs=0,clipboard='',actions=[],fail=false,detailFail=false,hold,holdDetail,holdSettings,taskReads=[];
let settingsSnapshot={pageId:'connection',definitionToken:'test-settings',revision:'1',values:{port:19846}},settingsSaves=[],conflict=false;
const data={service:'ready',endpoint:'http://127.0.0.1:19846/mcp',target:{state:'connected',target:{name:'prod-consul-01',endpoint:'192.0.2.11:22',username:'ops'}},evidenceDirectory:'/home/ops/.local/share/sodeck/plugin-data/sodeck.mcp/evidence',tasks:[{taskId:'t1',kind:'exec',requestId:'collect-consul-logs',native:{state:'running'},done:false,evidence:{complete:false}},{taskId:'t2',kind:'download',requestId:'consul-evidence',native:{state:'completed'},done:true,evidence:{complete:true}},{taskId:'t3',kind:'exec',requestId:'inspect-network',native:{state:'failed',exitCode:7},done:true,evidence:{complete:false,error:{message:'输出超出额度，请导出文件后下载。'}}}]};
const errors=[];page.on('pageerror',e=>errors.push(e.message));
Object.assign(data.tasks[0],{input:{command:"journalctl -u consul --since '15 minutes ago' --no-pager"},createdAt:Date.now()-12000});
Object.assign(data.tasks[1],{input:{remotePath:'/tmp/consul-evidence.log'},createdAt:Date.now()-20000,finishedAt:Date.now()-15000,evidence:{complete:true,path:'/evidence/t2/download.bin'}});
Object.assign(data.tasks[2],{input:{command:'printf "<img src=x onerror=alert(1)>"\n'+'x'.repeat(16000)},createdAt:Date.now()-40000,finishedAt:Date.now()-35000});
data.target.targetId='target-a';for(const task of data.tasks){task.targetId='target-a';task.target=structuredClone(data.target.target);}
function summary(task){const {input,directory,...row}=structuredClone(task);const value=input?.command??input?.remotePath??task.requestId;row.inputPreview=value.slice(0,256);row.inputTruncated=value.length>256;if(row.evidence)delete row.evidence.path;return row;}
const records=new Map(data.tasks.map(task=>[task.taskId,task]));
await page.exposeFunction('mcpRequest',async(method,p)=>{
  if(method==='openView'){actions.push(p);throw new Error('MCP must not open a central page');}
  if(method==='settings'){
    if(p.op==='get')return structuredClone(settingsSnapshot);
    if(p.op==='save'){settingsSaves.push(p);if(holdSettings)await new Promise(r=>{holdSettings=r;});if(conflict){settingsSnapshot={...settingsSnapshot,revision:'2',values:{port:19849}};return {persisted:false,conflict:true,snapshot:structuredClone(settingsSnapshot)};}
      assert.equal(p.baseRevision,settingsSnapshot.revision);settingsSnapshot={...settingsSnapshot,revision:String(Number(settingsSnapshot.revision)+1),values:p.values};return {persisted:true,snapshot:structuredClone(settingsSnapshot)};}
  }
  if(method==='clipboard'){if(p.op==='beginWrite'){clipboard='';return {id:'clip'};}if(p.op==='writeChunk'){clipboard+=p.text;return {};}return {};}
  if(method==='data'){actions.push(p);return {};}
  actions.push(p.action);
  if(p.action==='config'){configs++;return {token:'illustrative-secret',config:'[mcp_servers.sodeck]\nurl = "http://127.0.0.1:19846/mcp"\nhttp_headers = { Authorization = "Bearer illustrative-secret" }'};}
  if(p.action==='cancel')return {cancelRequested:true};
  if(p.action==='task'){
    if(detailFail)throw new Error('详情超过界面传输大小，请使用本地证据或 MCP 读取');
    taskReads.push(p);const task=structuredClone(data.tasks.find(task=>task.taskId===p.taskId)||records.get(p.taskId));
    if(holdDetail&&p.taskId==='t3'){const resolve=holdDetail;holdDetail=null;await new Promise(r=>resolve(r));}
    if(task.kind==='exec')task.output={stdout:p.tail?'latest output':p.cursor==='100'?'second page':'first page <img src=x onerror=alert(1)>',stderr:p.tail?'':'stderr evidence',nextCursor:p.cursor==='100'?'200':'100',hasMore:!p.tail&&p.cursor!=='100'};
    return task;
  }
  if(p.action==='snapshot'){snapshots++;if(hold)await new Promise(r=>{hold=r;});if(fail)throw new Error('核心暂不可用，请稍后刷新。');}
  return {...structuredClone(data),tasks:data.tasks.map(summary)};
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
async function overview(){if(await frame.locator('#detail').isVisible())await frame.getByRole('button',{name:'返回记录'}).click();}
async function choose(name){await overview();await frame.getByRole('button',{name}).click();}
async function openSettings(){await overview();if(!await frame.locator('#settings').evaluate(el=>el.open))await frame.locator('#settings>summary').click();}
const message=(type,extra={})=>page.evaluate(m=>{if(m.type==='themeChanged')for(const [key,value]of Object.entries(m.tokens))document.documentElement.style.setProperty('--'+key,value);document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'test',...m},'*');},{type,...extra});
fs.mkdirSync('artifacts/mcp-ui',{recursive:true});
try{
  await page.goto('http://127.0.0.1:'+server.address().port);await frame.locator('#server').filter({hasText:'prod-consul-01'}).waitFor();
  assert.equal(await frame.locator('h1').count(),0,'the host sidebar owns the plugin title; content must not repeat it');
  assert.equal(await frame.locator('title').textContent(),'Agent MCP');
  await message('themeChanged',{tokens:themes.light});
  assert((await frame.locator('main').textContent()).includes(data.tasks[0].input.command),'running operation must expose its actual command');
  assert.equal(configs,0,'snapshot never discloses configuration');
  assert(await frame.locator('#settings').isVisible());assert(!await frame.locator('#settings').evaluate(el=>el.open),'connection settings start collapsed');assert(await frame.locator('#history').isVisible());assert(await frame.locator('#detail').isHidden(),'the sidebar starts with monitoring and records');
  await frame.locator('#monitor-output').filter({hasText:'latest output'}).waitFor();
  await page.locator('#terminal').fill('draft input kept in the primary workspace');await page.locator('#terminal').focus();const workspaceText=await page.locator('#workspace').innerText(),workspaceBox=await page.locator('#workspace').boundingBox();
  await new Promise(r=>setTimeout(r,1700));assert.equal(await page.locator(':focus').getAttribute('id'),'terminal','MCP polling leaves primary-workspace focus alone');
  await choose('查看命令详情 collect-consul-logs');
  await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();assert.equal(await frame.locator('#detail-output img').count(),0,'output stays text');
  await frame.getByRole('button',{name:'下一页',exact:true}).click();await frame.locator('#detail-output').filter({hasText:'second page'}).waitFor();
  await frame.getByRole('button',{name:'上一页',exact:true}).focus();const before=snapshots;
  await new Promise(r=>setTimeout(r,1700));assert(snapshots>before);assert.equal(await frame.locator(':focus').textContent(),'上一页');assert((await frame.locator('#detail-output').textContent()).includes('second page'),'polling preserves the selected output page');
  await frame.getByRole('button',{name:'上一页',exact:true}).click();await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();
  await frame.locator('#detail-cancel').click();await frame.locator('#notice').filter({hasText:'等待远端实际结果'}).waitFor();assert(actions.includes('cancel'));assert(await frame.locator('#detail-cancel').isVisible(),'cancellation request does not claim completion');
  await choose('查看命令详情 inspect-network');await frame.locator('#detail-command').filter({hasText:'printf'}).waitFor();assert.equal(await frame.locator('#detail-command').textContent(),data.tasks[2].input.command);assert((await frame.locator('#detail-state').textContent()).includes('退出码 7'));assert(await frame.locator('#detail-error').isVisible());
  const execName=await frame.getByRole('button',{name:'查看命令详情 inspect-network',includeHidden:true}).getAttribute('aria-label');assert(execName.includes('prod-consul-01')&&execName.includes('退出码 7')&&execName.includes('printf'));assert(execName.length<300&&execName.endsWith('…'),'long command names stay concise while full input remains in detail');
  detailFail=true;await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#detail-output').filter({hasText:'详情超过界面传输大小'}).waitFor();assert.equal(await frame.locator('#detail-command').textContent(),data.tasks[2].input.command,'summary refresh does not replace an already loaded full command');detailFail=false;
  data.recordCount=100;data.tasks[0].phase='finalizing';await choose('查看命令详情 collect-consul-logs');await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#detail-state').filter({hasText:'正在收尾'}).waitFor();await frame.locator('#task-count').filter({hasText:'/ 100'}).waitFor({state:'attached'});assert(await frame.locator('#detail-cancel').isHidden());delete data.recordCount;delete data.tasks[0].phase;
  const recordedTarget=data.tasks[2].target;data.tasks[2].target=null;await choose('查看命令详情 inspect-network');await frame.locator('#detail-server').filter({hasText:'服务器信息不可用'}).waitFor();data.tasks[2].target=recordedTarget;
  const newest={...structuredClone(data.tasks[0]),taskId:'t4',requestId:'new-active',input:{command:'ping -c 10 192.0.2.12'}};data.tasks.unshift(newest);
  await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#tasks').filter({hasText:'ping -c 10'}).waitFor({state:'attached'});assert.equal(await frame.locator('#detail-request').textContent(),'inspect-network','new active operations do not replace history details');
  await choose('查看下载详情 consul-evidence');await frame.locator('#detail-path').filter({hasText:'/evidence/t2/download.bin'}).waitFor({state:'attached'});assert(await frame.locator('#detail-output-section').isHidden());assert(await frame.locator('#detail-evidence').isVisible());assert.equal(await frame.locator('#detail-evidence').textContent(),'文件已保存');assert.equal(await frame.locator('#detail-path').textContent(),'/evidence/t2/download.bin');
  const downloadName=await frame.getByRole('button',{name:'查看下载详情 consul-evidence',includeHidden:true}).getAttribute('aria-label');assert(downloadName.includes('prod-consul-01')&&downloadName.includes('已结束')&&downloadName.includes('/tmp/consul-evidence.log'));
  await frame.locator('#evidence-details summary').click();await frame.locator('main').evaluate(el=>el.scrollTop=0);await page.screenshot({path:'artifacts/mcp-ui/download.png',fullPage:true});await frame.locator('#evidence-details summary').click();
  let releaseDetail;holdDetail=resolve=>{releaseDetail=resolve;};await choose('查看命令详情 inspect-network');while(!releaseDetail)await new Promise(r=>setTimeout(r,10));
  await choose('查看下载详情 consul-evidence');releaseDetail();await new Promise(r=>setTimeout(r,100));assert.equal(await frame.locator('#detail-request').textContent(),'consul-evidence','late detail responses cannot overwrite a new selection');
  await frame.getByRole('button',{name:'返回记录'}).click();
  await choose('查看命令详情 collect-consul-logs');await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();await frame.locator('#next').focus();
  newest.done=true;newest.finishedAt=Date.now();newest.native={state:'completed',exitCode:0};data.tasks=[data.tasks[1],data.tasks[0],...data.tasks.slice(2)];
  const reordered=snapshots;await new Promise(r=>setTimeout(r,1700));assert(snapshots>reordered);assert.equal(await frame.locator(':focus').getAttribute('id'),'next','reordering preserves focus inside details');
  await frame.getByRole('button',{name:'返回记录'}).click();
  const savedTasks=data.tasks,oldest=savedTasks.find(task=>task.taskId==='t3');
  const completed=Array.from({length:48},(_,i)=>({...structuredClone(data.tasks[1]),taskId:'history-'+i,requestId:'history-'+i}));
  data.tasks=[data.tasks[0],...completed,oldest];await frame.locator('#refresh').evaluate(button=>button.click());await frame.locator('#tasks>li').last().filter({hasText:'退出码 7'}).waitFor();
  const historyButton=frame.getByRole('button',{name:'查看命令详情 inspect-network'});await historyButton.scrollIntoViewIfNeeded();await historyButton.focus();const returnTop=await historyButton.evaluate(el=>el.getBoundingClientRect().top);await historyButton.click();await frame.locator('#next').click();await frame.locator('#detail-output').filter({hasText:'second page'}).waitFor();
  await frame.locator('#previous').focus();const selectedTop=await frame.locator('#detail').evaluate(el=>el.getBoundingClientRect().top);
  data.tasks=[newest,...data.tasks.slice(0,49)];await frame.locator('#refresh').evaluate(button=>button.click());
  await new Promise(r=>setTimeout(r,100));assert(await frame.locator('#detail').isVisible(),'selected history stays open beyond 50 recent records');assert.equal(await frame.locator('#detail-request').textContent(),'inspect-network');assert((await frame.locator('#detail-output').textContent()).includes('second page'));assert.equal(await frame.locator(':focus').getAttribute('id'),'previous');
  assert(Math.abs((await frame.locator('#detail').evaluate(el=>el.getBoundingClientRect().top))-selectedTop)<2,'new rows preserve the detail reading position');
  await frame.getByRole('button',{name:'返回记录'}).click();assert((await frame.locator(':focus').getAttribute('aria-label')).includes('inspect-network'),'return restores the selected record focus');assert(Math.abs((await historyButton.evaluate(el=>el.getBoundingClientRect().top))-returnTop)<2,'return restores the list reading position after new records');data.tasks=savedTasks;await frame.locator('#refresh').evaluate(button=>button.click());await frame.getByRole('button',{name:'查看下载详情 consul-evidence',includeHidden:true}).waitFor();
  await openSettings();await frame.getByRole('button',{name:'复制 Codex 配置'}).click();await frame.locator('#notice').filter({hasText:'配置已复制'}).waitFor();assert(clipboard.includes('illustrative-secret'));
  await frame.locator('#show-token').click();await frame.locator('#access-token').filter({hasText:'illustrative-secret'}).waitFor();
  await frame.locator('#config-details summary').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  await frame.locator('main').evaluate(el=>el.scrollTop=0);await page.screenshot({path:'artifacts/mcp-ui/config-expanded.png',fullPage:true});
  await frame.locator('#settings>summary').click();await page.waitForFunction(()=>document.querySelector('iframe').contentDocument.querySelector('#config').textContent==='');assert(!(await frame.locator('#access-token').textContent()).includes('illustrative-secret'),'collapsing connection settings clears secrets');await openSettings();await frame.locator('#show-token').click();await frame.locator('#access-token').filter({hasText:'illustrative-secret'}).waitFor();await choose('查看命令详情 collect-consul-logs');assert.equal(await frame.locator('#config').textContent(),'');assert(!(await frame.locator('#access-token').textContent()).includes('illustrative-secret'),'entering task details clears secrets');await overview();
  await message('visibilityChanged',{visible:false});await page.waitForFunction(()=>document.querySelector('iframe').contentDocument.querySelector('#config').textContent==='');assert(!(await frame.locator('#access-token').textContent()).includes('illustrative-secret'));const hiddenCount=snapshots;await new Promise(r=>setTimeout(r,1700));assert.equal(snapshots,hiddenCount);
  await message('visibilityChanged',{visible:true});await frame.locator('#server').filter({hasText:'prod-consul-01'}).waitFor();
  await frame.locator('#directory-details summary').click();await frame.getByRole('button',{name:'打开证据目录'}).click();await page.waitForFunction(()=>!document.querySelector('iframe').contentDocument.querySelector('#reveal').disabled);assert(actions.some(p=>p?.op==='pluginData'&&p.action==='reveal'));await frame.locator('#directory-details summary').click();
  await openSettings();await frame.locator('#port').fill('1023');await frame.locator('#save-port').click();assert.equal(settingsSaves.length,0,'invalid ports never reach persistence');
  holdSettings=true;await frame.locator('#port').fill('19847');await frame.locator('#save-port').click();while(typeof holdSettings!=='function')await new Promise(r=>setTimeout(r,10));assert(await frame.locator('#port').isDisabled(),'a save cannot overwrite edits made while its reply is pending');await message('settingsChanged',{settings:settingsSnapshot});assert(await frame.locator('#port').isDisabled());holdSettings();holdSettings=null;await frame.locator('#port-notice').filter({hasText:'重启'}).waitFor();assert.equal(settingsSnapshot.values.port,19847);assert((await frame.locator('#endpoint').textContent()).includes('19846'),'saved port does not pretend the running listener changed');
  conflict=true;await frame.locator('#port').fill('19848');await frame.locator('#save-port').click();await frame.locator('#port-notice').filter({hasText:'草稿已保留'}).waitFor();assert.equal(await frame.locator('#port').inputValue(),'19848');assert(await frame.locator('#save-port').isDisabled());await frame.locator('main').evaluate(el=>el.scrollTop=0);await page.screenshot({path:'artifacts/mcp-ui/port-conflict.png',fullPage:true});await frame.locator('#reload-port').click();assert.equal(await frame.locator('#port').inputValue(),'19849');conflict=false;
  await setSize(320,1000);
  await choose('查看命令详情 collect-consul-logs');await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();await frame.locator('main').evaluate(el=>el.scrollTop=0);
  let supersededRead;holdDetail=resolve=>{supersededRead=resolve;};await choose('查看命令详情 inspect-network');while(!supersededRead)await new Promise(r=>setTimeout(r,10));
  await frame.locator('main').evaluate(el=>el.scrollTop=0);
  await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#detail-command').filter({hasText:records.get('t3').input.command}).waitFor();
  const racedTop=await frame.locator('#detail').evaluate(el=>el.getBoundingClientRect().top);supersededRead();assert(racedTop>=-1&&racedTop<=200,`refresh keeps sidebar details visible (${racedTop}px)`);assert(await frame.locator('#overview').isHidden());assert.equal(await frame.locator('main').evaluate(el=>el.scrollTop),0,'selection enters the side-panel details before any read completes');
  fs.mkdirSync('artifacts/mcp-ui',{recursive:true});
  for(const [name,tokens]of Object.entries(themes))for(const width of [200,320,640,1024]){
    await setSize(width,1000);await message('themeChanged',{tokens});
    await page.waitForFunction(canvas=>document.querySelector('iframe').contentDocument.documentElement.style.getPropertyValue('--canvas')===canvas,tokens.canvas);
    assert(await frame.locator('body').evaluate(el=>getComputedStyle(el).backgroundColor!== 'rgba(0, 0, 0, 0)'));
    assert(await frame.locator('main').evaluate(el=>el.ownerDocument.documentElement.scrollWidth<=innerWidth+1));
    await overview();await frame.locator('main').evaluate(el=>el.scrollTop=0);
    assert(await frame.locator('#refresh').evaluate(el=>el.parentElement.contains(el.ownerDocument.querySelector('#server'))),'refresh belongs to the server row');
    if(width===320||width===1024)await page.screenshot({path:`artifacts/mcp-ui/${name}-${width}.png`,fullPage:true});
    await choose('查看命令详情 inspect-network');
    await frame.locator('#detail-command').filter({hasText:records.get('t3').input.command}).waitFor();
    assert(await frame.locator('main').evaluate(el=>el.ownerDocument.documentElement.scrollWidth<=innerWidth+1),'long command detail fits narrow sidebars');
    if(width<=700){const detailTop=await frame.locator('#detail').evaluate(el=>el.getBoundingClientRect().top);assert(detailTop>=-1&&detailTop<=200,`selection opens details inside the sidebar (${width}px: ${detailTop}px)`);assert(await frame.locator('#overview').isHidden());}
    await frame.locator('main').evaluate(el=>el.scrollTop=0);
    if(width===320||width===1024)await page.screenshot({path:`artifacts/mcp-ui/${name}-${width}-detail.png`,fullPage:true});
    await frame.getByRole('button',{name:'返回记录'}).click();
  }
  for(const width of [200,320,640]){
    await setSize(width,400);
    assert(await frame.locator('main').evaluate(el=>el.clientHeight===innerHeight),'short pages retain a scrollable viewport');
    await openSettings();await frame.locator('#config-details summary').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
    await frame.locator('footer').evaluate(el=>el.scrollIntoView({block:'end'}));assert(await frame.locator('footer').evaluate(el=>el.getBoundingClientRect().bottom<=innerHeight+1),'short pages can reach their footer');
    await frame.locator('#config-details summary').click();
    if(width===320){await frame.locator('main').evaluate(el=>el.scrollTop=0);await page.screenshot({path:'artifacts/mcp-ui/short-pane.png',fullPage:true});}
  }
  await setSize(320,1000);
  await overview();await frame.locator('main').evaluate(el=>el.scrollTop=0);await frame.locator('#monitor-output').filter({hasText:'latest output'}).waitFor();assert(await frame.locator('#history').isVisible());await page.screenshot({path:'artifacts/mcp-ui/sidebar.png',fullPage:true});
  await frame.locator('#monitor-details').click();assert(await frame.locator('#detail').isVisible());assert(await frame.locator('#overview').isHidden());await overview();
  const finalWorkspaceBox=await page.locator('#workspace').boundingBox();for(const key of ['x','y','width'])assert.equal(finalWorkspaceBox[key],workspaceBox[key]);assert.equal(await page.locator('#workspace').innerText(),workspaceText);assert.equal(await page.locator('#terminal').inputValue(),'draft input kept in the primary workspace');assert(!actions.some(p=>p?.viewType),'all MCP controls stay within the sidebar');
  fail=true;await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#notice').filter({hasText:'核心暂不可用'}).waitFor();fail=false;
  await choose('查看命令详情 inspect-network');await frame.locator('#detail').waitFor();
  await frame.locator('#next').click();await frame.locator('#detail-output').filter({hasText:'second page'}).waitFor();
  data.target.targetId='target-b';data.tasks=[{...structuredClone(savedTasks[0]),taskId:'new-target',requestId:'new-target',targetId:'target-b'}];await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#detail').waitFor({state:'hidden'});assert(await frame.locator('#overview').isVisible(),'a new connection returns to its own overview');await choose('查看命令详情 new-target');await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();assert.equal(await frame.locator('#page-number').textContent(),'第 1 页','new connections clear the old output cursor');
  data.target.targetId='target-c';data.tasks=[];await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#empty').waitFor();assert.equal(await frame.locator('#idle').getAttribute('hidden'),null);assert(await frame.locator('#detail').isHidden(),'new connections clear old selected records');
  await frame.locator('main').evaluate(el=>el.scrollTop=0);await page.screenshot({path:'artifacts/mcp-ui/empty.png',fullPage:true});
  hold=true;await frame.getByRole('button',{name:'刷新',exact:true}).click();while(typeof hold!=='function')await new Promise(r=>setTimeout(r,10));await message('visibilityChanged',{visible:false});await message('visibilityChanged',{visible:true});hold();hold=null;const resume=snapshots;await new Promise(r=>setTimeout(r,1700));assert(snapshots>resume,'in-flight visibility change resumes polling');
  await message('unmount');const unmounted=snapshots;await new Promise(r=>setTimeout(r,1700));assert.equal(snapshots,unmounted);assert.deepEqual(errors,[]);
  assert(taskReads.some(p=>p.tail));assert(taskReads.some(p=>p.cursor==='100'));assert(!actions.some(p=>p?.viewType),'MCP never requests a central page');
  console.log('MCP UI: live commands/output, history details/paging, late replies, focus, three themes, narrow layouts, config, cancellation and lifecycle passed');
}finally{await browser.close();await new Promise(r=>server.close(r));assert.equal(path.dirname(assets),os.tmpdir());fs.rmSync(assets,{recursive:true,force:true});}
