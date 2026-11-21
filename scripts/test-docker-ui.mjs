import {verifyDockerPortsUI} from './test-docker-ports-ui.mjs';
await verifyDockerPortsUI();
import assert from 'node:assert/strict';
import fs from 'node:fs';import path from 'node:path';import {createServer} from 'node:http';
import {chromium} from 'playwright';
import {packageRoot,context} from './docker-process.mjs';
import {verifyDockerTreeUI} from './test-docker-tree-ui.mjs';
await verifyDockerTreeUI();
import {readTestThemeTokens} from './test-theme-tokens.mjs';
const root=packageRoot(),id='a'.repeat(64),second='b'.repeat(64),requests=[];
const rows=[{id,name:'prod-api-中文',image:'api:2026.11',status:'running',health:'unhealthy',project:'shop',service:'api'},
  {id:second,name:'worker',image:'worker:1',status:'exited',health:null,project:'shop',service:'worker'}];
let snapshot={kind:'logs',context,containerId:id,endpoint:'unix:///var/run/docker.sock',engine:{id:'engine',name:'prod-engine-01',version:'24.0.9',arch:'amd64'},revision:'1',nextIntent:'1',state:'ready',status:'running',observedAt:Date.now(),busy:false,paused:false,tail:200,navigation:{}};
const env=[{key:'TOKEN',value:'a=b=c',secret:true},{key:'EMPTY',value:'',secret:true}];
const logs=[{stream:'stdout',text:new Date(Date.now()-90000).toISOString()+' INFO 服务启动成功'},{stream:'stderr',text:new Date(Date.now()-89000).toISOString()+' ERROR connection refused <script>boom()</script>'},{stream:'stdout',text:'repeat'},{stream:'stdout',text:'repeat'}];
let logCapture={generation:1,loading:false,navigation:{},resources:[{id:'history',start:0,end:2000000000,tail:true,status:{state:'completed',finished:true}},{id:'live',start:0,end:null,tail:false,status:{state:'running',finished:false}}]};
const subscriptions=new Map();
const bytesFor=rows=>Buffer.from(rows.map((row,i)=>JSON.stringify({v:1,sequence:String(i+1),source:row.stream,receivedAt:Date.now(),dataBase64:Buffer.from(row.text+'\n').toString('base64')})+'\n').join(''));
const capturedBytes=new Map([['history',bytesFor(logs)],['live',Buffer.alloc(0)]]);
async function sendFrame(id,kind,bytes=Buffer.alloc(0)) {const s=subscriptions.get(id);if(!s)return;s.packet++;s.offset+=bytes.length;await page.evaluate(frame=>window.post('data',{data:frame}),{kind,packet:s.packet,offset:s.offset,data:[...bytes],metadata:{streamId:id}});}
async function appendLive(rows) {const bytes=bytesFor(rows);capturedBytes.set('live',Buffer.concat([capturedBytes.get('live'),bytes]));await sendFrame('live','data',bytes);}
const server=createServer((req,res)=>{
  if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><html><body style="margin:0"><iframe title="Docker 测试" style="border:0;width:100vw;height:100vh" src="/ui/logs.html"></iframe></body></html>');return;}
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
  if(d.action==='logs.status')return logCapture;
  if(d.action==='logs.start')return {accepted:true};
  if(d.action==='logs.navigation'){logCapture.navigation=d.state;snapshot.logCapture=logCapture;return {};}
  if(d.action==='logs.stop'){logCapture.resources[1].status={state:'cancelled',finished:true};return {};}
  if(d.action==='logs.loadEarlier')return {accepted:true};
  if(d.action==='navigation'){snapshot.navigation=d.state;return snapshot;}
  if(d.action==='resumeInput')return snapshot;
  if(d.action==='openContainer'||d.action==='openLogs')return {viewId:'opened'};
  if(d.action==='openTerminal'){snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),terminal:{status:'pending',containerId:d.id}};return snapshot;}
  if(d.action==='pauseRefresh'){snapshot.paused=d.paused;return snapshot;}
  if(d.action==='operate'){const status=d.operation==='stop'?'exited':'running';const row=rows.find(r=>r.id===d.id);if(row)row.status=status;snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),nextIntent:String(Number(snapshot.nextIntent)+1),status,operation:{action:d.operation,containerId:d.id,name:row?.name,state:'succeeded',message:'已刷新当前观测状态'}};return snapshot;}
  if(d.action==='stats'){snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),stats:{CPUPerc:'150.5%',MemUsage:'512MiB / 2GiB',NetIO:'10MB / 2MB',BlockIO:'1MB / 0B'}};return snapshot;}
  if(d.action==='refresh'){snapshot.revision=String(Number(snapshot.revision)+1);return snapshot;}
  throw new Error('Unexpected UI action '+d.action);
});
await page.exposeFunction('mockData',async d=>{
  if(d.op==='sessionStream'){
    if(d.action==='attach'){const offset=Number(d.offset||0);subscriptions.set(d.id,{packet:0,offset});await sendFrame(d.id,'reset');const bytes=capturedBytes.get(d.id)||Buffer.alloc(0);for(let at=offset;at<bytes.length;at+=16384)await sendFrame(d.id,'data',bytes.subarray(at,at+16384));await sendFrame(d.id,'caughtUp');if(logCapture.resources.find(r=>r.id===d.id)?.status.finished)await sendFrame(d.id,'end');return {};}
    if(d.action==='detach'){subscriptions.delete(d.id);return {};}
    if(d.action==='pin')return {pinId:'pin-'+d.id};
    if(d.action==='unpin'||d.action==='ack')return {};
  }
  if(d.op==='contentExport')return d.action==='begin'?{id:'export',state:'receiving'}:{state:d.action==='finish'?'completed':d.action==='cancel'?'cancelled':'receiving'};
  throw new Error('Unexpected data request '+JSON.stringify(d));
});
await page.addInitScript(({snapshot,tokens})=>{
  if(window!==top)return;
  window.mount=snapshot;window.tokens=tokens;window.flushed=[];window.responses=[];window.prepared=0;
  window.post=(type,extra={})=>document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'surface',type,...extra},'*');
  addEventListener('message',async e=>{
    if(e.source!==document.querySelector('iframe')?.contentWindow||e.data?.channel!=='sodeck-ui-v2')return;
    const m=e.data;
    if(m.type==='ready')window.post('mount',{data:window.mount,tokens:window.tokens,capabilities:{clipboard:1},readOnly:!!window.candidate});
    if(m.type==='prepared'){window.prepared++;if(!window.candidate)window.post('surfaceReady');}
    if(m.type==='flushed')window.flushed.push(m);
    if(m.type==='request'){
      try{const data=m.method==='event'?await window.mock(m.params):m.method==='data'?await window.mockData(m.params):{};if(m.method==='event')window.responses.push(m.params);window.post('response',{id:m.id,data});}
      catch(error){window.post('response',{id:m.id,error:error.message});}
    }
  });
},{snapshot,tokens:themes.light});
const url='http://127.0.0.1:'+server.address().port;
try{
  await page.goto(url);await page.waitForFunction(()=>window.prepared>0);let frame=page.frames()[1];
  fs.mkdirSync('artifacts/docker-ui',{recursive:true});
  for(const mode of ['logs']){
    snapshot={...snapshot,kind:mode,name:'prod-api-中文',status:'running',terminal:null,operation:null,revision:String(Number(snapshot.revision)+1)};
    await page.evaluate(s=>{window.mount=s;window.post('mount',{data:s,tokens:window.tokens,readOnly:false,capabilities:{clipboard:1}});},snapshot);
    await frame.locator('#'+mode).waitFor({state:'visible'});
    const opsBefore=requests.filter(r=>r.action==='operate').length,terminalsBefore=requests.filter(r=>r.action==='openTerminal').length;
    if(mode==='logs'){
      await frame.locator('#log-lines').filter({hasText:'服务启动'}).waitFor();
      for(const label of ['更多','显示','筛选']){await frame.locator('.log-tools').getByText(label,{exact:true}).click();assert.equal(await frame.locator('.log-tools details[open]').count(),1,'log popovers never overlap');}await frame.locator('.log-tools').getByText('筛选',{exact:true}).click();
      await appendLive([...Array.from({length:4000},(_,i)=>({stream:'stdout',text:'INFO fixture '+i+' '+(i%7===0?'长内容 '.repeat(25):'')})),{stream:'stderr',text:'ERROR off-page UNIQUE'}]);
      await frame.locator('#log-scope').filter({hasText:'4,005条'}).waitFor();assert(await frame.locator('#log-viewport').evaluate(e=>e.scrollTop+e.clientHeight>=e.scrollHeight-8),'follows new output');
      await frame.locator('#log-viewport').focus();await page.keyboard.press('PageUp');await frame.locator('#log-latest').waitFor();await page.waitForTimeout(400);
      const anchor=await frame.locator('#log-lines').evaluate(el=>{const v=el.parentElement,top=v.getBoundingClientRect().top,line=[...el.querySelectorAll('.log-line')].find(e=>e.getBoundingClientRect().top>=top);return {id:line.dataset.id,top:line.getBoundingClientRect().top,viewportTop:top,width:v.clientWidth,height:v.clientHeight,scroll:v.scrollTop};});
      for(const height of [620,780]){await page.setViewportSize({...page.viewportSize(),height});await page.waitForTimeout(400);const line=frame.locator('.log-line[data-id="'+anchor.id+'"]');assert.equal(await line.count(),1,'height-only reflow retains the visible record');const next=await frame.locator('body').evaluate((_,id)=>{const e=[...document.querySelectorAll('.log-line')].find(n=>n.dataset.id===id),v=document.getElementById('log-viewport');return e?{top:e.getBoundingClientRect().top,viewportTop:v.getBoundingClientRect().top,width:v.clientWidth,height:v.clientHeight,scroll:v.scrollTop}:null;},anchor.id);assert(next,'height reflow retains current attached record');assert(Math.abs(next.top-anchor.top)<=2,'height-only reflow preserves wrapped record position: '+JSON.stringify({height,anchor,next}));}
      const reading=await frame.locator('#log-lines').evaluate(el=>{const top=el.parentElement.getBoundingClientRect().top,line=[...el.querySelectorAll('.log-line')].find(e=>e.getBoundingClientRect().bottom>top);return {id:line.dataset.id,relativeTop:line.getBoundingClientRect().top-top};});
      const originalWidth=page.viewportSize().width;
      for(const width of [900,400,originalWidth,900,400,originalWidth]){await page.setViewportSize({width,height:780});await page.waitForTimeout(500);const line=frame.locator('.log-line[data-id="'+reading.id+'"]');assert.equal(await line.count(),1,'width reflow retains the actual visible record');const relativeTop=await frame.locator('body').evaluate((_,id)=>{const e=[...document.querySelectorAll('.log-line')].find(n=>n.dataset.id===id);return e?e.getBoundingClientRect().top-document.getElementById('log-viewport').getBoundingClientRect().top:null;},reading.id);assert.notEqual(relativeTop,null,'width reflow retains current attached record');assert(Math.abs(relativeTop-reading.relativeTop)<=2,'width reflow preserves actual record offset: '+JSON.stringify({width,reading,relativeTop}));}
      // A previous measurement can disagree with the actual wrapped DOM before the next paint.
      const redrawReading=await frame.locator('#log-lines').evaluate(el=>{el.querySelector('.log-line').style.minHeight='140px';const v=el.parentElement.getBoundingClientRect(),line=[...el.querySelectorAll('.log-line')].find(e=>{const r=e.getBoundingClientRect();return r.bottom>v.top&&r.top<v.bottom;});return {id:line.dataset.id,relativeTop:line.getBoundingClientRect().top-v.top};});
      for(let cycle=0;cycle<40;cycle++){
        await appendLive([{stream:'stdout',text:'INFO background '+cycle+' '+ '长内容 '.repeat(25)}]);await page.waitForTimeout(180);
        const relativeTop=await frame.locator('body').evaluate((_,id)=>{const line=[...document.querySelectorAll('.log-line')].find(e=>e.dataset.id===id);return line?line.getBoundingClientRect().top-document.getElementById('log-viewport').getBoundingClientRect().top:null;},redrawReading.id);
        assert.notEqual(relativeTop,null,'background repaint keeps the actual reading record');assert(Math.abs(relativeTop-redrawReading.relativeTop)<=2,'background repaint keeps record offset: '+JSON.stringify({cycle,redrawReading,relativeTop}));
      }
      await page.setViewportSize({width:900,height:780});await page.waitForTimeout(500);
      await page.emulateMedia({reducedMotion:'no-preference'});
      const detailReading=await frame.locator('#log-lines').evaluate(el=>{const top=el.parentElement.getBoundingClientRect().top,line=[...el.querySelectorAll('.log-line')].find(e=>e.getBoundingClientRect().top>=top);return {id:line.dataset.id,top:line.getBoundingClientRect().top-top};});
      await frame.evaluate(({id})=>{
        const detail=document.getElementById('log-detail'),widths=window.detailWidths=[],until=performance.now()+600;
        const sample=()=>requestAnimationFrame(()=>{widths.push(detail.getBoundingClientRect().width);if(performance.now()<until)sample();});sample();
        document.querySelector(`.log-line[data-id="${CSS.escape(id)}"] .log-time`).click();
      },detailReading);
      await page.waitForTimeout(60);
      await frame.locator('#log-detail-close').dispatchEvent('click');
      assert(await frame.locator('#log-detail').evaluate(el=>el.inert),'Closing detail stops input immediately');
      await page.waitForTimeout(45);
      await frame.evaluate(id=>document.querySelector(`.log-line[data-id="${CSS.escape(id)}"] .log-time`).click(),detailReading.id);
      await page.waitForTimeout(650);
      assert(await frame.locator('#log-detail').isVisible(),'A stale close cannot hide a reopened detail');
      assert((await frame.evaluate(()=>new Set(detailWidths.map(Math.round)).size))>3,'Detail passes through real intermediate widths');
      const detailTop=await frame.locator('body').evaluate((_,id)=>{const line=document.querySelector(`.log-line[data-id="${CSS.escape(id)}"]`);return line?.getBoundingClientRect().top-document.getElementById('log-viewport').getBoundingClientRect().top;},detailReading.id);
      assert(Math.abs(detailTop-detailReading.top)<=2,'Animated detail retains the reading record offset: '+JSON.stringify({detailReading,detailTop}));
      await frame.locator('#log-detail-close').click();await frame.locator('#log-detail').waitFor({state:'hidden'});
      await page.emulateMedia({reducedMotion:'reduce'});
      await frame.locator('#log-query').fill('unique');await frame.locator('#log-matches').filter({hasText:'1处'}).waitFor();
      await frame.locator('#log-next-match').click();await frame.locator('#log-lines').filter({hasText:'off-page UNIQUE'}).waitFor();
      await frame.locator('#log-query').fill('no-match');await frame.locator('#log-matches').filter({hasText:'0处'}).waitFor();
      await frame.locator('#log-query').fill('refused');await frame.locator('#log-matches').filter({hasText:'1处'}).waitFor();await frame.locator('#log-lines mark').filter({hasText:'refused'}).waitFor();await frame.locator('#log-next-match').click();
      await frame.waitForFunction(()=>{const v=document.getElementById('log-viewport'),line=[...document.querySelectorAll('.log-line')].find(e=>e.querySelector('mark')?.textContent==='refused');if(!line)return false;const a=line.getBoundingClientRect(),b=v.getBoundingClientRect();return v.scrollTop<1||Math.abs(a.top+a.height/2-b.top-b.height/2)<3;});
      const before=await frame.locator('#log-viewport').evaluate(e=>e.scrollTop);await appendLive([{stream:'stdout',text:'INFO received while reading'}]);await frame.locator('#log-latest').filter({hasText:'41条新日志'}).waitFor();assert.equal(await frame.locator('#log-viewport').evaluate(e=>e.scrollTop),before,'incoming output keeps pinned reading position');
      await frame.locator('#log-query').fill('');await frame.locator('.log-map-thumb').focus();await page.keyboard.press('Home');await frame.locator('#log-lines').filter({hasText:'服务启动'}).waitFor();
      await frame.locator('.log-time').first().click();await frame.locator('#log-detail').waitFor({state:'visible'});await frame.locator('#log-detail-close').click();
      await frame.locator('#log-latest').click();await frame.locator('#log-lines').filter({hasText:'received while reading'}).waitFor();assert(await frame.locator('#log-latest').isHidden());
      assert((await frame.locator('.log-line').count())<100,'DOM stays limited to the visible window');
    }
    for(const t of ['light','dark','dopamine'])for(const width of [200,320,400,900]){
      await page.setViewportSize({width,height:780});await page.evaluate(tokens=>window.post('themeChanged',{tokens}),themes[t]);
      if(mode==='logs')await frame.waitForFunction(()=>{const v=document.getElementById('log-viewport'),last=[...document.querySelectorAll('.log-line')].at(-1);return last?.textContent.includes('received while reading')&&v.scrollTop+v.clientHeight>=v.scrollHeight-8;});
      assert(await frame.locator('body').evaluate(e=>e.scrollWidth<=e.clientWidth),mode+' '+t+' overflow '+width);
      await page.screenshot({path:'artifacts/docker-ui/'+mode+'-'+t+'-'+width+'.png'});
    }
    if(mode==='logs'){
      await frame.locator('.log-map-thumb').focus();await page.keyboard.press('Home');
      await frame.locator('#log-lines').filter({hasText:'服务启动'}).waitFor();
      const initial=await frame.locator('.log-line').first().getAttribute('data-id'),thumb=await frame.locator('.log-map-thumb').boundingBox(),rail=await frame.locator('.log-minimap').boundingBox();
      await page.mouse.move(thumb.x+thumb.width/2,thumb.y+thumb.height/2);await page.mouse.down();await page.mouse.move(thumb.x+thumb.width/2,rail.y+rail.height*.7);
      capturedBytes.set('earlier',bytesFor([{stream:'stdout',text:'2026-01-01T00:00:00Z INFO earlier history'}]));logCapture.resources.push({id:'earlier',start:0,end:2000000000,tail:true,status:{state:'completed',finished:true}});
      await frame.locator('#log-scope').filter({hasText:'4,047条'}).waitFor();await frame.locator('.log-map-thumb').focus();await page.keyboard.press('Escape');await page.mouse.up();
      await frame.waitForFunction(id=>{const e=[...document.querySelectorAll('.log-line')].find(n=>n.dataset.id===id);if(!e)return false;const v=document.getElementById('log-viewport').getBoundingClientRect(),r=e.getBoundingClientRect();return r.top>=v.top-2&&r.top<v.top+65;},initial);
      await page.setViewportSize({width:900,height:780});await page.evaluate(tokens=>window.post('themeChanged',{tokens}),themes.dark);
      await frame.locator('#log-query').fill('UNIQUE');await frame.locator('#log-matches').filter({hasText:'1处'}).waitFor();await frame.locator('.log-tools details').first().locator('summary').click();await frame.locator('#log-only').check();
      await frame.locator('#log-scope').filter({hasText:'1条'}).waitFor();assert(!(await frame.locator('#log-scope').innerText()).includes('01-01'));assert((await frame.locator('#log-scope').innerText()).includes('接收时间'));await page.screenshot({path:'artifacts/docker-ui/logs-search.png'});
      await frame.locator('.log-tools details').first().locator('summary').click();await frame.locator('.log-time').first().click();await frame.locator('#log-detail').waitFor({state:'visible'});await page.screenshot({path:'artifacts/docker-ui/logs-detail.png'});await frame.locator('#log-detail-close').click();
      await frame.locator('#log-query').fill('not-found');await frame.locator('#log-empty').waitFor({state:'visible'});await page.screenshot({path:'artifacts/docker-ui/logs-empty.png'});
      await frame.locator('#log-query').fill('UNIQUE');await frame.locator('#log-scope').filter({hasText:'1条'}).waitFor();await frame.locator('.log-tools details').last().locator('summary').click();await page.screenshot({path:'artifacts/docker-ui/logs-more.png'});await frame.locator('.log-tools details').last().locator('summary').click();
    }
  }
  await page.evaluate(()=>window.post('flush',{id:'candidate-source',freeze:true}));await page.waitForFunction(()=>window.flushed.some(f=>f.id==='candidate-source'));
  const before=requests.length,prepared=await page.evaluate(()=>window.prepared);await page.evaluate(s=>{window.mount=s;window.candidate=true;window.post('mount',{data:s,tokens:window.tokens,readOnly:true});},snapshot);await page.waitForFunction(count=>window.prepared>count,prepared);assert.equal(requests.length,before,'Candidate must not poll or send UI events: '+JSON.stringify(requests.slice(before)));
  assert.equal(await frame.locator('#log-query').inputValue(),'UNIQUE','candidate restores the fixed query from the original capture');
  await page.evaluate(()=>{window.candidate=false;window.post('surfaceReady');window.post('flush',{id:'move',freeze:true});});await page.waitForFunction(()=>window.flushed.some(f=>f.id==='move'));
  const frozen=requests.length;await new Promise(r=>setTimeout(r,1200));assert.equal(requests.length,frozen,'Frozen surface must not keep polling: '+JSON.stringify(requests.slice(frozen)));
  await page.evaluate(()=>window.post('transferCancelled'));await new Promise(r=>setTimeout(r,1200));assert(requests.length>frozen);
  snapshot={...snapshot,containerId:second,name:'other-container',logCapture:{...logCapture,generation:2,navigation:{}}};logCapture=snapshot.logCapture;await page.evaluate(s=>window.post('mount',{data:s,tokens:window.tokens,readOnly:false}),snapshot);await frame.locator('#title').filter({hasText:'other-container'}).waitFor();assert.equal(await frame.locator('#log-query').inputValue(),'');assert.equal(await frame.locator('#log-range').inputValue(),'latest');assert(!(await frame.locator('#log-only').isChecked()),'another container starts without prior filters');
  assert.deepEqual(errors,[]);console.log('Docker UI: inline tree, ports, log reading anchors, history, queries, export, themes, narrow layouts and transfer passed.');
}catch(error){await page.screenshot({path:'artifacts/docker-ui/logs-failure.png'});console.error('Log page errors:',errors);console.error('Log layout:',await page.frames()[1].locator('#log-viewport').evaluate(e=>({height:e.clientHeight,width:e.clientWidth,scroll:e.scrollTop,notice:document.querySelector('#log-notice').textContent})));throw error;}finally{await browser.close();await new Promise(r=>server.close(r));}
