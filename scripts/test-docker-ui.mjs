import assert from 'node:assert/strict';
import fs from 'node:fs';import path from 'node:path';import {createServer} from 'node:http';
import {chromium} from 'playwright';
import {packageRoot,context} from './docker-process.mjs';
import {readTestThemeTokens} from './test-theme-tokens.mjs';
const root=packageRoot(),id='a'.repeat(64),second='b'.repeat(64),requests=[];
const rows=[{id,name:'prod-api-中文',image:'api:2026.11',status:'running',health:'unhealthy',project:'shop',service:'api'},
  {id:second,name:'worker',image:'worker:1',status:'exited',health:null,project:'shop',service:'worker'}];
let snapshot={kind:'browser',context,containerId:id,endpoint:'unix:///var/run/docker.sock',engine:{id:'engine',name:'prod-engine-01',version:'24.0.9',arch:'amd64'},revision:'1',nextIntent:'1',state:'ready',status:'running',observedAt:Date.now(),busy:false,paused:false,tail:200,navigation:{}};
const env=[{key:'TOKEN',value:'a=b=c',secret:true},{key:'EMPTY',value:'',secret:true}];
const logs=[{stream:'stdout',text:'2026-11-10T03:42:01.123Z 服务启动成功'},{stream:'stderr',text:'2026-11-10T03:42:02Z connection refused <script>boom()</script>'},{stream:'stdout',text:'repeat'},{stream:'stdout',text:'repeat'}];
const server=createServer((req,res)=>{
  if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><html><body style="margin:0"><iframe title="Docker 测试" style="border:0;width:100vw;height:100vh" src="/ui/index.html"></iframe></body></html>');return;}
  const file=path.resolve(root,'.'+decodeURIComponent(req.url.split('?')[0]));
  if(!file.startsWith(root+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}
  res.setHeader('Content-Type',{'.html':'text/html','.js':'text/javascript','.css':'text/css'}[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const browser=await chromium.launch({headless:true}),page=await browser.newPage({viewport:{width:320,height:780},reducedMotion:'reduce'});
const contexts=browser.contexts().length,themes=await readTestThemeTokens(browser);
assert.equal(browser.contexts().length,contexts,'Theme snapshots release their temporary page and context');
for(const [name,tokens]of Object.entries(themes)){
  assert(Object.values(tokens).every(value=>value!==''),name+' has all host theme tokens');
  assert.equal(tokens['color-scheme'],name==='dark'?'dark':'light');
  for(const key of ['sd-radius-control','sd-tab-height','sd-status-height','sd-hit-size'])assert.equal(tokens[key],themes.light[key],key+' stays stable across themes');
}
assert.notEqual(themes.light.canvas,themes.dark.canvas);
assert.notEqual(themes.light.accent,themes.dopamine.accent);
const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.exposeFunction('mock',async d=>{
  requests.push(d);
  if(d.action==='snapshot')return snapshot;
  if(d.action==='list'){
    const found=rows.filter(r=>(d.status==='all'||d.status===r.status||(d.status==='abnormal'&&r.health==='unhealthy'))&&(!d.query||JSON.stringify(r).includes(d.query)));
    const offset=d.offset||0,limit=d.limit||50;
    return {rows:found.slice(offset,offset+limit),total:found.length,nextOffset:offset+limit<found.length?offset+limit:null};
  }
  if(d.action==='section')return {rows:d.section==='environment'?env:[{key:'容器工作目录',value:'/srv/app'},{key:'重启策略',value:'{"Name":"unless-stopped"}'}],total:2,nextOffset:null};
  if(d.action==='readLogs'){
    const found=logs.filter(r=>!d.query||r.text.toLowerCase().includes(d.query.toLowerCase())),offset=d.offset||0,limit=d.limit||100;
    return {rows:found.slice(offset,offset+limit),total:found.length,nextOffset:offset+limit<found.length?offset+limit:null};
  }
  if(d.action==='navigation'){snapshot.navigation=d.state;return snapshot;}
  if(d.action==='resumeInput')return snapshot;
  if(d.action==='openContainer'||d.action==='openLogs')return {viewId:'opened'};
  if(d.action==='openTerminal'){snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),terminal:{status:'pending'}};return snapshot;}
  if(d.action==='pauseRefresh'){snapshot.paused=d.paused;return snapshot;}
  if(d.action==='operate'){snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),nextIntent:'2',status:d.operation==='stop'?'exited':'running',operation:{action:d.operation,state:'succeeded',message:'已刷新当前观测状态'}};return snapshot;}
  if(d.action==='stats'){snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),stats:{CPUPerc:'150.5%',MemUsage:'512MiB / 2GiB',NetIO:'10MB / 2MB',BlockIO:'1MB / 0B'}};return snapshot;}
  if(d.action==='refresh'){snapshot.revision=String(Number(snapshot.revision)+1);return snapshot;}
  throw new Error('Unexpected UI action '+d.action);
});
await page.addInitScript(({snapshot,tokens})=>{
  if(window!==top)return;
  window.mount=snapshot;window.tokens=tokens;window.flushed=[];
  window.post=(type,extra={})=>document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'surface',type,...extra},'*');
  addEventListener('message',async e=>{
    if(e.source!==document.querySelector('iframe')?.contentWindow||e.data?.channel!=='sodeck-ui-v2')return;
    const m=e.data;
    if(m.type==='ready')window.post('mount',{data:window.mount,tokens:window.tokens,capabilities:{clipboard:1},readOnly:!!window.candidate});
    if(m.type==='prepared'&&!window.candidate)window.post('surfaceReady');
    if(m.type==='flushed')window.flushed.push(m);
    if(m.type==='request'){
      try{const data=m.method==='event'?await window.mock(m.params):{};window.post('response',{id:m.id,data});}
      catch(error){window.post('response',{id:m.id,error:error.message});}
    }
  });
},{snapshot,tokens:themes.light});
const url='http://127.0.0.1:'+server.address().port;
try{
  await page.goto(url);let frame=page.frames()[1];
  await frame.locator('.container-row').first().waitFor();
  await frame.locator('#query').fill('中文');assert.equal(await frame.locator('.container-row').count(),1);
  await frame.locator('.container-row').first().focus();await page.keyboard.press('Enter');
  await page.waitForFunction(()=>true);await new Promise(r=>setTimeout(r,100));assert(requests.some(r=>r.action==='openContainer'&&r.id===id));
  await frame.locator('#query').fill('absent');await frame.locator('#empty').waitFor({state:'visible'});
  await frame.locator('#query').fill('');await frame.locator('#status-filter').selectOption('all');await frame.locator('.container-row').nth(1).waitFor();
  await frame.locator('.container-row').nth(1).click();await frame.locator('#query').fill('prod');
  await frame.locator('.container-row[tabindex="0"][data-id="'+id+'"]').waitFor();assert.equal(await frame.locator('.container-row[tabindex="0"]').getAttribute('data-id'),id);
  rows.push(...Array.from({length:58},(_,i)=>({id:(i+1).toString(16).padStart(64,'0'),name:'extra-'+i,image:'extra:1',status:'running'})));
  await frame.locator('#query').fill('');
  await frame.locator('#next:not([disabled])').waitFor();await frame.locator('#next').click();await frame.locator('#page-label').filter({hasText:'51–60'}).waitFor();
  assert.equal(await frame.locator('.container-row[tabindex="0"]').count(),1,'page without the selected row must keep a keyboard entry');
  await frame.locator('#previous').click();await frame.locator('#page-label').filter({hasText:'1–50'}).waitFor();
  await frame.locator('#scroll').evaluate(e=>{e.scrollTop=180;});
  await page.evaluate(()=>window.post('flush',{id:'navigation',freeze:false}));await page.waitForFunction(()=>window.flushed.some(f=>f.id==='navigation'));
  const nav=snapshot.navigation;assert.equal(nav.scrollTop,180);assert.deepEqual(nav.listHistory,[]);
  await page.evaluate(s=>window.post('mount',{data:s,tokens:window.tokens,readOnly:false}),snapshot);
  await page.waitForFunction(()=>true);await new Promise(r=>setTimeout(r,100));assert.equal(await frame.locator('#scroll').evaluate(e=>e.scrollTop),180,'remount restores scroll after data rendering');
  rows.splice(2);await frame.locator('#query').fill('prod');await frame.locator('.container-row').first().waitFor();
  fs.mkdirSync('artifacts/docker-ui',{recursive:true});
  for(const mode of ['browser','container','logs']){
    snapshot={...snapshot,kind:mode,name:'prod-api-中文',revision:String(Number(snapshot.revision)+1)};
    await page.evaluate(s=>{window.mount=s;window.post('mount',{data:s,tokens:window.tokens,readOnly:false,capabilities:{clipboard:1}});},snapshot);
    await frame.locator('#'+mode).waitFor({state:'visible'});
    if(mode==='container'){
      await frame.locator('#terminal-shell').fill('/bin/bash');
      await frame.locator('#open-terminal').click();
      assert(await frame.locator('#open-terminal').isDisabled());
      assert(await frame.locator('#terminal-shell').isDisabled());
      await frame.locator('#open-terminal').dispatchEvent('click');
      await frame.locator('#terminal-result').filter({hasText:'终端请求待确认'}).waitFor();
      assert.equal(requests.filter(r=>r.action==='openTerminal').length,1);
      assert.equal(requests.find(r=>r.action==='openTerminal').shell,'/bin/bash');
      snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),terminal:{status:'completed',viewId:'terminal-page'}};
      await frame.locator('#terminal-result').filter({hasText:'终端页面已创建'}).waitFor();
      await frame.getByRole('button',{name:'环境变量',exact:true}).click();await frame.locator('#fields').filter({hasText:'TOKEN'}).waitFor();assert(!(await frame.locator('#fields').innerText()).includes('a=b=c'));
      await frame.locator('#show-env').check();assert((await frame.locator('#fields').innerText()).includes('a=b=c'));
      await frame.getByRole('button',{name:'停止',exact:true}).click();await frame.locator('#confirm').waitFor({state:'visible'});await page.keyboard.press('Escape');
      assert.equal(requests.filter(r=>r.action==='operate').length,0);
      await frame.getByRole('button',{name:'停止',exact:true}).click();await frame.locator('#confirm-submit').click();
      await frame.locator('#operation').filter({hasText:'命令成功'}).waitFor();assert.equal(requests.filter(r=>r.action==='operate').length,1);
      await frame.locator('#read-stats').click();await frame.locator('#stats').filter({hasText:'150.5%'}).waitFor();
    }
    if(mode==='logs'){
      await frame.locator('#log-content').filter({hasText:'服务启动'}).waitFor();
      logs.push(...Array.from({length:120},(_,i)=>({stream:'stdout',text:'fixture '+i})),{stream:'stdout',text:'off-page UNIQUE'});
      await frame.locator('#log-query').fill('unique');await frame.locator('#log-content').filter({hasText:'off-page UNIQUE'}).waitFor();
      assert.equal(await frame.locator('#logs-page').innerText(),'1–1 / 1');
      await frame.locator('#log-query').fill('no-match');await frame.locator('#logs-page').filter({hasText:'无匹配日志'}).waitFor();
      logs.splice(4);await frame.locator('#log-query').fill('refused');await frame.locator('#log-content').filter({hasText:'refused'}).waitFor();
      assert(!(await frame.locator('#log-content').innerText()).includes('服务启动'));await frame.locator('#log-query').fill('');await frame.locator('#log-content').filter({hasText:'服务启动'}).waitFor();
    }
    for(const t of ['light','dark','dopamine'])for(const width of [200,320,400,900]){
      await page.setViewportSize({width,height:780});await page.evaluate(tokens=>window.post('themeChanged',{tokens}),themes[t]);
      assert(await frame.locator('body').evaluate(e=>e.scrollWidth<=e.clientWidth),mode+' '+t+' overflow '+width);
      await page.screenshot({path:'artifacts/docker-ui/'+mode+'-'+t+'-'+width+'.png'});
    }
  }
  const before=requests.length;await page.evaluate(()=>{window.candidate=true;window.post('mount',{data:window.mount,tokens:window.tokens,readOnly:true});});await new Promise(r=>setTimeout(r,1200));assert.equal(requests.length,before,'Candidate must not poll or send UI events');
  await page.evaluate(()=>{window.candidate=false;window.post('surfaceReady');window.post('flush',{id:'move',freeze:true});});await page.waitForFunction(()=>window.flushed.some(f=>f.id==='move'));
  const frozen=requests.length;await new Promise(r=>setTimeout(r,1200));assert.equal(requests.length,frozen,'Frozen surface must not keep polling');
  await page.evaluate(()=>window.post('transferCancelled'));await new Promise(r=>setTimeout(r,1200));assert(requests.length>frozen);
  assert.deepEqual(errors,[]);console.log('Docker UI: filters, keyboard, confirmation, environment visibility, logs, stats, themes, narrow layouts and transfer passed.');
}finally{await browser.close();await new Promise(r=>server.close(r));}
