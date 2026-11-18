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
  if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><body style="margin:0"><iframe title="MCP 状态" style="border:0;width:100vw;height:100vh" src="/ui/index.html"></iframe>');return;}
  const url=req.url.split('?')[0],base=['/ui/text-menu.js','/ui/shared.css','/ui/clipboard-bridge.js'].includes(url)?assets:root,file=path.join(base,url);
  if(!file.startsWith(base+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}
  res.setHeader('Content-Type',{'.js':'text/javascript','.html':'text/html','.css':'text/css'}[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const browser=await chromium.launch({headless:true}),themes=await readTestThemeTokens(browser),page=await browser.newPage({viewport:{width:320,height:850}});
let snapshots=0,configs=0,clipboard='',actions=[],fail=false,hold,holdDetail,taskReads=[];
const data={service:'ready',endpoint:'http://127.0.0.1:19846/mcp',target:{state:'connected',target:{name:'prod-consul-01',endpoint:'192.0.2.11:22',username:'ops'}},evidenceDirectory:'/home/ops/.local/share/sodeck/plugin-data/sodeck.mcp/evidence',tasks:[{taskId:'t1',kind:'exec',requestId:'collect-consul-logs',native:{state:'running'},done:false,evidence:{complete:false}},{taskId:'t2',kind:'download',requestId:'consul-evidence',native:{state:'completed'},done:true,evidence:{complete:true}},{taskId:'t3',kind:'exec',requestId:'inspect-network',native:{state:'failed',exitCode:7},done:true,evidence:{complete:false,error:{message:'输出超出额度，请导出文件后下载。'}}}]};
const errors=[];page.on('pageerror',e=>errors.push(e.message));
Object.assign(data.tasks[0],{input:{command:"journalctl -u consul --since '15 minutes ago' --no-pager"},createdAt:Date.now()-12000});
Object.assign(data.tasks[1],{input:{remotePath:'/tmp/consul-evidence.log'},createdAt:Date.now()-20000,finishedAt:Date.now()-15000,evidence:{complete:true,path:'/evidence/t2/download.bin'}});
Object.assign(data.tasks[2],{input:{command:'printf "<img src=x onerror=alert(1)>"\n'+'x'.repeat(16000)},createdAt:Date.now()-40000,finishedAt:Date.now()-35000});
data.target.targetId='target-a';for(const task of data.tasks)task.targetId='target-a';
const records=new Map(data.tasks.map(task=>[task.taskId,task]));
await page.exposeFunction('mcpRequest',async(method,p)=>{
  if(method==='clipboard'){if(p.op==='beginWrite'){clipboard='';return {id:'clip'};}if(p.op==='writeChunk'){clipboard+=p.text;return {};}return {};}
  if(method==='data'){actions.push(p);return {};}
  actions.push(p.action);
  if(p.action==='config'){configs++;return {config:'[mcp_servers.sodeck]\nurl = "http://127.0.0.1:19846/mcp"\nhttp_headers = { Authorization = "Bearer illustrative-secret" }'};}
  if(p.action==='task'){
    taskReads.push(p);const task=structuredClone(data.tasks.find(task=>task.taskId===p.taskId)||records.get(p.taskId));
    if(holdDetail&&p.taskId==='t3'){const resolve=holdDetail;holdDetail=null;await new Promise(r=>resolve(r));}
    if(task.kind==='exec')task.output={stdout:p.tail?'latest output':p.cursor==='100'?'second page':'first page <img src=x onerror=alert(1)>',stderr:p.tail?'':'stderr evidence',nextCursor:p.cursor==='100'?'200':'100',hasMore:!p.tail&&p.cursor!=='100'};
    return task;
  }
  if(p.action==='snapshot'){snapshots++;if(hold)await new Promise(r=>{hold=r;});if(fail)throw new Error('核心暂不可用，请稍后刷新。');}
  return structuredClone(data);
});
await page.addInitScript(()=>{
  if(window!==top)return;
  addEventListener('message',async e=>{const m=e.data;if(m?.channel!=='sodeck-ui-v2')return;
    if(m.type==='ready')e.source.postMessage({channel:m.channel,type:'mount',surfaceId:'test',visible:true,capabilities:{clipboard:1},tokens:{}},'*');
    if(m.type==='request')try{const data=await window.mcpRequest(m.method,m.params);e.source.postMessage({channel:m.channel,type:'response',surfaceId:'test',id:m.id,data},'*');}catch(error){e.source.postMessage({channel:m.channel,type:'response',surfaceId:'test',id:m.id,error:error.message},'*');}
  });
});
const frame=page.frameLocator('iframe');
const message=(type,extra={})=>page.evaluate(m=>document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'test',...m},'*'),{type,...extra});
try{
  await page.goto('http://127.0.0.1:'+server.address().port);await frame.locator('#server').filter({hasText:'prod-consul-01'}).waitFor();
  assert((await frame.locator('main').textContent()).includes(data.tasks[0].input.command),'running operation must expose its actual command');
  assert.equal(configs,0,'snapshot never discloses configuration');
  await frame.locator('#monitor-output').filter({hasText:'latest output'}).waitFor();
  await frame.getByRole('button',{name:'查看命令详情 collect-consul-logs'}).click();
  await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();assert.equal(await frame.locator('#detail-output img').count(),0,'output stays text');
  await frame.getByRole('button',{name:'下一页',exact:true}).click();await frame.locator('#detail-output').filter({hasText:'second page'}).waitFor();
  await frame.getByRole('button',{name:'上一页',exact:true}).focus();const before=snapshots;
  await new Promise(r=>setTimeout(r,1700));assert(snapshots>before);assert.equal(await frame.locator(':focus').textContent(),'上一页');assert((await frame.locator('#detail-output').textContent()).includes('second page'),'polling preserves the selected output page');
  await frame.getByRole('button',{name:'上一页',exact:true}).click();await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();
  await frame.locator('#monitor-cancel').click();await frame.locator('#notice').filter({hasText:'等待远端实际结果'}).waitFor();assert(actions.includes('cancel'));assert(await frame.locator('#monitor-body').isVisible(),'cancellation request does not claim completion');
  await frame.getByRole('button',{name:'查看命令详情 inspect-network'}).click();await frame.locator('#detail-command').filter({hasText:'printf'}).waitFor();assert.equal(await frame.locator('#detail-command').textContent(),data.tasks[2].input.command);assert((await frame.locator('#detail-state').textContent()).includes('退出码 7'));assert(await frame.locator('#detail-error').isVisible());
  const newest={...structuredClone(data.tasks[0]),taskId:'t4',requestId:'new-active',input:{command:'ping -c 10 192.0.2.12'}};data.tasks.unshift(newest);
  await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#monitor-command').filter({hasText:'ping -c 10'}).waitFor();assert.equal(await frame.locator('#detail-request').textContent(),'inspect-network','new active operations do not replace history details');
  await frame.getByRole('button',{name:'查看下载详情 consul-evidence'}).click();assert(await frame.locator('#detail-output-section').isHidden());assert.equal(await frame.locator('#detail-path').textContent(),'/evidence/t2/download.bin');
  let releaseDetail;holdDetail=resolve=>{releaseDetail=resolve;};await frame.getByRole('button',{name:'查看命令详情 inspect-network'}).click();while(!releaseDetail)await new Promise(r=>setTimeout(r,10));
  await frame.getByRole('button',{name:'查看下载详情 consul-evidence'}).click();releaseDetail();await new Promise(r=>setTimeout(r,100));assert.equal(await frame.locator('#detail-request').textContent(),'consul-evidence','late detail responses cannot overwrite a new selection');
  await frame.getByRole('button',{name:'收起详情'}).click();
  await frame.getByRole('button',{name:'查看命令详情 collect-consul-logs'}).click();await frame.locator('#detail-output').filter({hasText:'first page'}).waitFor();await frame.locator('#next').focus();
  newest.done=true;newest.finishedAt=Date.now();newest.native={state:'completed',exitCode:0};data.tasks=[data.tasks[1],data.tasks[0],...data.tasks.slice(2)];
  const reordered=snapshots;await new Promise(r=>setTimeout(r,1700));assert(snapshots>reordered);assert.equal(await frame.locator(':focus').getAttribute('id'),'next','reordering preserves focus inside details');
  await frame.getByRole('button',{name:'收起详情'}).click();
  const savedTasks=data.tasks,oldest=savedTasks.find(task=>task.taskId==='t3');
  const completed=Array.from({length:48},(_,i)=>({...structuredClone(data.tasks[1]),taskId:'history-'+i,requestId:'history-'+i}));
  data.tasks=[data.tasks[0],...completed,oldest];await frame.locator('#refresh').evaluate(button=>button.click());await frame.locator('#tasks>li').last().filter({hasText:'退出码 7'}).waitFor();
  await frame.getByRole('button',{name:'查看命令详情 inspect-network'}).click();await frame.locator('#next').click();await frame.locator('#detail-output').filter({hasText:'second page'}).waitFor();
  await frame.locator('#previous').focus();const selectedTop=await frame.locator('#detail').evaluate(el=>el.getBoundingClientRect().top);
  data.tasks=[newest,...data.tasks.slice(0,49)];await frame.locator('#refresh').evaluate(button=>button.click());
  await new Promise(r=>setTimeout(r,100));assert(await frame.locator('#detail').isVisible(),'selected history stays open beyond 50 recent records');assert.equal(await frame.locator('#detail-request').textContent(),'inspect-network');assert((await frame.locator('#detail-output').textContent()).includes('second page'));assert.equal(await frame.locator(':focus').getAttribute('id'),'previous');
  assert(Math.abs((await frame.locator('#detail').evaluate(el=>el.getBoundingClientRect().top))-selectedTop)<2,'new rows preserve the detail reading position');
  await frame.getByRole('button',{name:'收起详情'}).click();data.tasks=savedTasks;await frame.locator('#refresh').evaluate(button=>button.click());await frame.getByRole('button',{name:'查看下载详情 consul-evidence'}).waitFor();
  await frame.getByRole('button',{name:'接入设置',exact:true}).click();
  await frame.getByRole('button',{name:'复制 Codex 配置'}).click();await frame.locator('#notice').filter({hasText:'配置已复制'}).waitFor();assert(clipboard.includes('illustrative-secret'));
  await frame.locator('#config-details summary').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  await message('visibilityChanged',{visible:false});await page.waitForFunction(()=>document.querySelector('iframe').contentDocument.querySelector('#config').textContent==='');const hiddenCount=snapshots;await new Promise(r=>setTimeout(r,1700));assert.equal(snapshots,hiddenCount);
  await message('visibilityChanged',{visible:true});await frame.locator('#server').filter({hasText:'prod-consul-01'}).waitFor();
  await frame.getByRole('button',{name:'接入设置',exact:true}).click();await frame.getByRole('button',{name:'打开证据目录'}).click();await page.waitForFunction(()=>!document.querySelector('iframe').contentDocument.querySelector('#reveal').disabled);assert(actions.some(p=>p?.op==='pluginData'&&p.action==='reveal'));
  await frame.getByRole('button',{name:'接入设置',exact:true}).click();
  fs.mkdirSync('artifacts/mcp-ui',{recursive:true});
  for(const [name,tokens]of Object.entries(themes))for(const width of [200,320,640]){
    await page.setViewportSize({width,height:1000});await message('themeChanged',{tokens});
    await page.waitForFunction(canvas=>document.querySelector('iframe').contentDocument.documentElement.style.getPropertyValue('--canvas')===canvas,tokens.canvas);
    assert(await frame.locator('body').evaluate(el=>getComputedStyle(el).backgroundColor!== 'rgba(0, 0, 0, 0)'));
    assert(await frame.locator('main').evaluate(el=>el.ownerDocument.documentElement.scrollWidth<=innerWidth+1));
    if(width===320)await page.screenshot({path:`artifacts/mcp-ui/${name}.png`,fullPage:true});
    await frame.getByRole('button',{name:'查看命令详情 inspect-network'}).click();
    assert(await frame.locator('main').evaluate(el=>el.ownerDocument.documentElement.scrollWidth<=innerWidth+1),'long command detail fits narrow sidebars');
    if(width===320)await page.screenshot({path:`artifacts/mcp-ui/${name}-detail.png`,fullPage:true});
    await frame.getByRole('button',{name:'收起详情'}).click();
  }
  for(const width of [200,320,640]){
    await page.setViewportSize({width,height:400});
    assert(await frame.locator('footer').evaluate(el=>el.getBoundingClientRect().bottom<=innerHeight+1),'short panes retain the footer and operation list');
    assert(await frame.locator('#history').evaluate(el=>el.clientHeight>=90),'short panes keep the list usable');
    await frame.getByRole('button',{name:'接入设置',exact:true}).click();
    assert(await frame.locator('footer').evaluate(el=>el.getBoundingClientRect().bottom<=innerHeight+1),'opening settings does not clip the list');
    await frame.getByRole('button',{name:'接入设置',exact:true}).click();
    if(width===320)await page.screenshot({path:'artifacts/mcp-ui/short-pane.png',fullPage:true});
  }
  await page.setViewportSize({width:320,height:1000});
  fail=true;await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#notice').filter({hasText:'核心暂不可用'}).waitFor();fail=false;
  await frame.getByRole('button',{name:'查看命令详情 inspect-network'}).click();await frame.locator('#detail').waitFor();
  data.target.targetId='target-b';data.tasks=[];await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#empty').waitFor();assert(await frame.locator('#idle').isVisible());assert(await frame.locator('#detail').isHidden(),'new connections clear old selected records');
  hold=true;await frame.getByRole('button',{name:'刷新',exact:true}).click();while(typeof hold!=='function')await new Promise(r=>setTimeout(r,10));await message('visibilityChanged',{visible:false});await message('visibilityChanged',{visible:true});hold();hold=null;const resume=snapshots;await new Promise(r=>setTimeout(r,1700));assert(snapshots>resume,'in-flight visibility change resumes polling');
  await message('unmount');const unmounted=snapshots;await new Promise(r=>setTimeout(r,1700));assert.equal(snapshots,unmounted);assert.deepEqual(errors,[]);
  assert(taskReads.some(p=>p.tail));assert(taskReads.some(p=>p.cursor==='100'));
  console.log('MCP UI: live commands/output, history details/paging, late replies, focus, three themes, narrow layouts, config, cancellation and lifecycle passed');
}finally{await browser.close();await new Promise(r=>server.close(r));assert.equal(path.dirname(assets),os.tmpdir());fs.rmSync(assets,{recursive:true,force:true});}
