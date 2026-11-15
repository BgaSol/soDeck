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
const logs=[{stream:'stdout',text:new Date(Date.now()-90000).toISOString()+' INFO 服务启动成功'},{stream:'stderr',text:new Date(Date.now()-89000).toISOString()+' ERROR connection refused <script>boom()</script>'},{stream:'stdout',text:'repeat'},{stream:'stdout',text:'repeat'}];
let logCapture={generation:1,loading:false,navigation:{},resources:[{id:'history',start:0,end:2000000000,tail:true,status:{state:'completed',finished:true}},{id:'live',start:0,end:null,tail:false,status:{state:'running',finished:false}}]};
const subscriptions=new Map();
const bytesFor=rows=>Buffer.from(rows.map((row,i)=>JSON.stringify({v:1,sequence:String(i+1),source:row.stream,receivedAt:Date.now(),dataBase64:Buffer.from(row.text+'\n').toString('base64')})+'\n').join(''));
const capturedBytes=new Map([['history',bytesFor(logs)],['live',Buffer.alloc(0)]]);
async function sendFrame(id,kind,bytes=Buffer.alloc(0)) {const s=subscriptions.get(id);if(!s)return;s.packet++;s.offset+=bytes.length;await page.evaluate(frame=>window.post('data',{data:frame}),{kind,packet:s.packet,offset:s.offset,data:[...bytes],metadata:{streamId:id}});}
async function appendLive(rows) {const bytes=bytesFor(rows);capturedBytes.set('live',Buffer.concat([capturedBytes.get('live'),bytes]));await sendFrame('live','data',bytes);}
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
  window.mount=snapshot;window.tokens=tokens;window.flushed=[];window.responses=[];
  window.post=(type,extra={})=>document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'surface',type,...extra},'*');
  addEventListener('message',async e=>{
    if(e.source!==document.querySelector('iframe')?.contentWindow||e.data?.channel!=='sodeck-ui-v2')return;
    const m=e.data;
    if(m.type==='ready')window.post('mount',{data:window.mount,tokens:window.tokens,capabilities:{clipboard:1},readOnly:!!window.candidate});
    if(m.type==='prepared'&&!window.candidate)window.post('surfaceReady');
    if(m.type==='flushed')window.flushed.push(m);
    if(m.type==='request'){
      try{const data=m.method==='event'?await window.mock(m.params):m.method==='data'?await window.mockData(m.params):{};if(m.method==='event')window.responses.push(m.params);window.post('response',{id:m.id,data});}
      catch(error){window.post('response',{id:m.id,error:error.message});}
    }
  });
},{snapshot,tokens:themes.light});
const url='http://127.0.0.1:'+server.address().port;
try{
  await page.goto(url);let frame=page.frames()[1];
  await frame.locator('.container-row').first().waitFor();
  await frame.locator('#query').fill('中文');assert.equal(await frame.locator('.container-row').count(),1);
  await frame.locator('.container-select').first().focus();await page.keyboard.press('Enter');
  await frame.locator('.inline-actions').waitFor();assert(!requests.some(r=>r.action==='openContainer'),'Enter only selects');
  await frame.locator('.inline-actions').getByRole('button',{name:'详情',exact:true}).click();
  await page.waitForFunction(()=>window.responses.some(r=>r.action==='openContainer'));
  assert(requests.some(r=>r.action==='openContainer'&&r.id===id&&r.engineId==='engine'));
  await frame.locator('#query').fill('absent');await frame.locator('#empty').waitFor({state:'visible'});
  await frame.locator('#query').fill('');await frame.locator('[data-status="all"]').click();await frame.locator('.container-row').nth(1).waitFor();
  await frame.locator('.container-select').nth(1).click();await frame.locator('#query').fill('prod');
  await frame.locator('[data-id="'+id+'"] .container-select[tabindex="0"]').waitFor();
  rows.push(...Array.from({length:58},(_,i)=>({id:(i+1).toString(16).padStart(64,'0'),name:'extra-'+i,image:'extra:1',status:'running'})));
  await frame.locator('#query').fill('');
  await frame.locator('#next:not([disabled])').waitFor();await frame.locator('#next').click();await frame.locator('#page-label').filter({hasText:'51–60'}).waitFor();
  assert.equal(await frame.locator('.container-select[tabindex="0"]').count(),1,'page without the selected row must keep a keyboard entry');
  await frame.locator('#previous').click();await frame.locator('#page-label').filter({hasText:'1–50'}).waitFor();
  await frame.locator('#scroll').evaluate(e=>{e.scrollTop=180;});
  await page.evaluate(()=>window.post('flush',{id:'navigation',freeze:false}));await page.waitForFunction(()=>window.flushed.some(f=>f.id==='navigation'));
  const nav=snapshot.navigation;assert.equal(nav.scrollTop,180);assert.deepEqual(nav.listHistory,[]);
  await page.evaluate(s=>window.post('mount',{data:s,tokens:window.tokens,readOnly:false}),snapshot);
  await page.waitForFunction(()=>true);await new Promise(r=>setTimeout(r,100));assert.equal(await frame.locator('#scroll').evaluate(e=>e.scrollTop),180,'remount restores scroll after data rendering');
  rows.splice(2);await frame.locator('#query').fill('prod');await frame.locator('.container-row').first().waitFor();
  fs.mkdirSync('artifacts/docker-ui',{recursive:true});
  for(const mode of ['browser','container','logs']){
    snapshot={...snapshot,kind:mode,name:'prod-api-中文',status:'running',terminal:null,operation:null,revision:String(Number(snapshot.revision)+1)};
    await page.evaluate(s=>{window.mount=s;window.post('mount',{data:s,tokens:window.tokens,readOnly:false,capabilities:{clipboard:1}});},snapshot);
    await frame.locator('#'+mode).waitFor({state:'visible'});
    const opsBefore=requests.filter(r=>r.action==='operate').length,terminalsBefore=requests.filter(r=>r.action==='openTerminal').length;
    if(mode==='browser'){
      await frame.locator('#query').fill('');await frame.locator('[data-status="all"]').click();
      await frame.locator('[data-id="'+id+'"] .container-select').click();
      const more=frame.locator('.row-more'),summary=more.locator('summary');
      await summary.click();await more.getByRole('button',{name:'暂停',exact:true}).focus();
      const oldMenu=await more.elementHandle();snapshot={...snapshot,revision:String(Number(snapshot.revision)+1)};
      await frame.waitForFunction(menu=>!menu.isConnected,oldMenu);await oldMenu.dispose();
      assert(await more.evaluate(e=>e.open),'background revisions keep the row menu open');
      assert(await more.getByRole('button',{name:'暂停',exact:true}).evaluate(e=>e===document.activeElement),'background revisions preserve menu focus');
      await page.keyboard.press('Escape');assert(!(await more.evaluate(e=>e.open)));
      assert(await summary.evaluate(e=>e===document.activeElement),'Escape returns menu focus to its summary');
      await summary.click();await more.getByRole('button',{name:'终端选项',exact:true}).click();
      const shell=frame.locator('#browser-shell');await shell.fill('/bin/bash');await shell.evaluate(e=>e.setSelectionRange(5,9,'backward'));
      const oldShell=await shell.elementHandle();snapshot={...snapshot,revision:String(Number(snapshot.revision)+1)};
      await frame.waitForFunction(input=>!input.isConnected,oldShell);await oldShell.dispose();
      assert.deepEqual(await shell.evaluate(e=>({focused:e===document.activeElement,value:e.value,start:e.selectionStart,end:e.selectionEnd,direction:e.selectionDirection})),{focused:true,value:'/bin/bash',start:5,end:9,direction:'backward'},'background revisions preserve shell focus, value and selection');
      await more.getByRole('button',{name:'暂停',exact:true}).click();await frame.locator('#confirm').waitFor({state:'visible'});await page.keyboard.press('Escape');
      assert(await summary.evaluate(e=>e===document.activeElement),'cancelling a menu operation returns to the visible summary');
      await frame.locator('[data-id="'+id+'"] .container-select').click();
      const openedBefore=requests.filter(r=>['openContainer','openLogs','openTerminal'].includes(r.action)).length;
      await frame.locator('.inline-actions').getByRole('button',{name:'停止',exact:true}).click();await frame.locator('#confirm').waitFor({state:'visible'});
      const oldRow=await frame.locator('[data-id="'+id+'"]').elementHandle();snapshot={...snapshot,revision:String(Number(snapshot.revision)+1)};
      await frame.waitForFunction(row=>!row.isConnected,oldRow);await oldRow.dispose();
      assert(await frame.locator('#confirm-cancel').evaluate(e=>e===document.activeElement),'background revisions preserve focus in the inline confirmation');
      await page.keyboard.press('Escape');assert(await frame.locator('.inline-actions').getByRole('button',{name:'停止',exact:true}).evaluate(e=>e===document.activeElement),'Escape after a refresh returns to the recreated operation trigger');
      await frame.locator('.inline-actions').getByRole('button',{name:'停止',exact:true}).click();await frame.locator('#confirm').waitFor({state:'visible'});
      assert.equal(await frame.locator('dialog').count(),0,'confirmation stays in the sidebar');
      await frame.locator('[data-id="'+second+'"] .container-select').click();assert(await frame.locator('#confirm').isHidden());
      assert.equal(requests.filter(r=>r.action==='operate').length,opsBefore,'changing selection cancels the old confirmation');
      await frame.locator('[data-id="'+id+'"] .container-select').click();
      await frame.locator('.inline-actions').getByRole('button',{name:'停止',exact:true}).click();await frame.locator('#confirm-submit').click();
      await frame.locator('.row-result').filter({hasText:'命令成功'}).waitFor();
      assert.equal(requests.filter(r=>r.action==='operate').length,opsBefore+1);
      assert.equal(requests.filter(r=>['openContainer','openLogs','openTerminal'].includes(r.action)).length,openedBefore,'sidebar maintenance does not open central pages');
      const op=requests.filter(r=>r.action==='operate').at(-1);assert.equal(op.id,id);assert.deepEqual(op.expectedContext,context);assert.equal(op.endpoint,snapshot.endpoint);
      await frame.locator('.inline-actions').getByRole('button',{name:'启动',exact:true}).click();await frame.locator('.inline-actions').getByRole('button',{name:'停止',exact:true}).waitFor();
      await frame.locator('.inline-actions').getByRole('button',{name:'终端',exact:true}).click();await frame.locator('.row-result').filter({hasText:'终端请求待确认'}).waitFor();
      assert.equal(requests.filter(r=>r.action==='openTerminal').length,terminalsBefore+1);
      assert(await frame.locator('.inline-actions').getByRole('button',{name:'终端',exact:true}).isDisabled());
      snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),terminal:{status:'completed',containerId:id,viewId:'sidebar-terminal'}};
      await frame.locator('.row-result').filter({hasText:'终端页面已创建'}).waitFor();
      const logsBefore=requests.filter(r=>r.action==='openLogs').length;
      await frame.locator('.inline-actions').getByRole('button',{name:'日志',exact:true}).click();
      await page.waitForFunction(count=>window.responses.filter(r=>r.action==='openLogs').length>count,logsBefore);
      assert.equal(requests.filter(r=>r.action==='openLogs').length,logsBefore+1);
      assert.equal(requests.filter(r=>r.action==='openLogs').at(-1).id,id,'logs open the selected container even when a list poll follows');
    }
    if(mode==='container'){
      await frame.locator('#terminal-shell').fill('/bin/bash');
      await frame.locator('#open-terminal').click();
      assert(await frame.locator('#open-terminal').isDisabled());
      assert(await frame.locator('#terminal-shell').isDisabled());
      await frame.locator('#open-terminal').dispatchEvent('click');
      await frame.locator('#terminal-result').filter({hasText:'终端请求待确认'}).waitFor();
      assert.equal(requests.filter(r=>r.action==='openTerminal').length,terminalsBefore+1);
      assert.equal(requests.filter(r=>r.action==='openTerminal').at(-1).shell,'/bin/bash');
      snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),terminal:{status:'completed',viewId:'terminal-page'}};
      await frame.locator('#terminal-result').filter({hasText:'终端页面已创建'}).waitFor();
      await frame.getByRole('button',{name:'环境变量',exact:true}).click();await frame.locator('#fields').filter({hasText:'TOKEN'}).waitFor();assert(!(await frame.locator('#fields').innerText()).includes('a=b=c'));
      await frame.locator('#show-env').check();assert((await frame.locator('#fields').innerText()).includes('a=b=c'));
      await frame.getByRole('button',{name:'停止',exact:true}).click();await frame.locator('#confirm').waitFor({state:'visible'});await page.keyboard.press('Escape');
      assert.equal(requests.filter(r=>r.action==='operate').length,opsBefore);
      await frame.getByRole('button',{name:'停止',exact:true}).click();await frame.locator('#confirm-submit').click();
      await frame.locator('#operation').filter({hasText:'命令成功'}).waitFor();assert.equal(requests.filter(r=>r.action==='operate').length,opsBefore+1);
      await frame.locator('#read-stats').click();await frame.locator('#stats').filter({hasText:'150.5%'}).waitFor();
    }
    if(mode==='logs'){
      await frame.locator('#log-lines').filter({hasText:'服务启动'}).waitFor();
      for(const label of ['更多','显示','筛选']){await frame.locator('.log-tools').getByText(label,{exact:true}).click();assert.equal(await frame.locator('.log-tools details[open]').count(),1,'log popovers never overlap');}await frame.locator('.log-tools').getByText('筛选',{exact:true}).click();
      await appendLive([...Array.from({length:500},(_,i)=>({stream:'stdout',text:'INFO fixture '+i+' '+(i%7===0?'长内容 '.repeat(25):'')})),{stream:'stderr',text:'ERROR off-page UNIQUE'}]);
      await frame.locator('#log-scope').filter({hasText:'505条'}).waitFor();assert(await frame.locator('#log-viewport').evaluate(e=>e.scrollTop+e.clientHeight>=e.scrollHeight-8),'follows new output');
      await frame.locator('#log-viewport').focus();await page.keyboard.press('PageUp');await frame.locator('#log-latest').waitFor();await page.waitForTimeout(400);
      const anchor=await frame.locator('#log-lines').evaluate(el=>{const v=el.parentElement,top=v.getBoundingClientRect().top,line=[...el.querySelectorAll('.log-line')].find(e=>e.getBoundingClientRect().top>=top);return {id:line.dataset.id,top:line.getBoundingClientRect().top,viewportTop:top,width:v.clientWidth,height:v.clientHeight,scroll:v.scrollTop};});
      for(const height of [620,780]){await page.setViewportSize({...page.viewportSize(),height});await page.waitForTimeout(400);const line=frame.locator('.log-line[data-id="'+anchor.id+'"]');assert.equal(await line.count(),1,'height-only reflow retains the visible record');const next=await line.evaluate(e=>{const v=document.getElementById('log-viewport');return {top:e.getBoundingClientRect().top,viewportTop:v.getBoundingClientRect().top,width:v.clientWidth,height:v.clientHeight,scroll:v.scrollTop};});assert(Math.abs(next.top-anchor.top)<=2,'height-only reflow preserves wrapped record position: '+JSON.stringify({height,anchor,next}));}
      await frame.locator('#log-query').fill('unique');await frame.locator('#log-matches').filter({hasText:'1处'}).waitFor();
      await frame.locator('#log-next-match').click();await frame.locator('#log-lines').filter({hasText:'off-page UNIQUE'}).waitFor();
      await frame.locator('#log-query').fill('no-match');await frame.locator('#log-matches').filter({hasText:'0处'}).waitFor();
      await frame.locator('#log-query').fill('refused');await frame.locator('#log-next-match').click();await frame.locator('#log-lines').filter({hasText:'refused'}).waitFor();
      await frame.waitForFunction(()=>{const v=document.getElementById('log-viewport'),line=[...document.querySelectorAll('.log-line')].find(e=>e.querySelector('mark')?.textContent==='refused');if(!line)return false;const a=line.getBoundingClientRect(),b=v.getBoundingClientRect();return v.scrollTop<1||Math.abs(a.top+a.height/2-b.top-b.height/2)<3;});
      const before=await frame.locator('#log-viewport').evaluate(e=>e.scrollTop);await appendLive([{stream:'stdout',text:'INFO received while reading'}]);await frame.locator('#log-latest').filter({hasText:'1条新日志'}).waitFor();assert.equal(await frame.locator('#log-viewport').evaluate(e=>e.scrollTop),before,'incoming output keeps pinned reading position');
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
      await frame.locator('#log-scope').filter({hasText:'507条'}).waitFor();await frame.locator('.log-map-thumb').focus();await page.keyboard.press('Escape');await page.mouse.up();
      await frame.locator('.log-line[data-id="'+initial+'"]').waitFor();assert(await frame.locator('.log-line[data-id="'+initial+'"]').evaluate(e=>{const v=document.getElementById('log-viewport').getBoundingClientRect(),r=e.getBoundingClientRect();return r.top>=v.top-2&&r.top<v.top+65;}),'Escape restores the original record after history arrives during drag');
      await page.setViewportSize({width:900,height:780});await page.evaluate(tokens=>window.post('themeChanged',{tokens}),themes.dark);
      await frame.locator('#log-query').fill('UNIQUE');await frame.locator('#log-matches').filter({hasText:'1处'}).waitFor();await frame.locator('.log-tools details').first().locator('summary').click();await frame.locator('#log-only').check();
      await frame.locator('#log-scope').filter({hasText:'1条'}).waitFor();assert(!(await frame.locator('#log-scope').innerText()).includes('01-01'));assert((await frame.locator('#log-scope').innerText()).includes('接收时间'));await page.screenshot({path:'artifacts/docker-ui/logs-search.png'});
      await frame.locator('.log-tools details').first().locator('summary').click();await frame.locator('.log-time').first().click();await frame.locator('#log-detail').waitFor({state:'visible'});await page.screenshot({path:'artifacts/docker-ui/logs-detail.png'});await frame.locator('#log-detail-close').click();
      await frame.locator('#log-query').fill('not-found');await frame.locator('#log-empty').waitFor({state:'visible'});await page.screenshot({path:'artifacts/docker-ui/logs-empty.png'});
      await frame.locator('#log-query').fill('UNIQUE');await frame.locator('#log-scope').filter({hasText:'1条'}).waitFor();await frame.locator('.log-tools details').last().locator('summary').click();await page.screenshot({path:'artifacts/docker-ui/logs-more.png'});await frame.locator('.log-tools details').last().locator('summary').click();
    }
  }
  await page.evaluate(()=>window.post('flush',{id:'candidate-source',freeze:true}));await page.waitForFunction(()=>window.flushed.some(f=>f.id==='candidate-source'));
  const before=requests.length;await page.evaluate(s=>{window.mount=s;window.candidate=true;window.post('mount',{data:s,tokens:window.tokens,readOnly:true});},snapshot);await new Promise(r=>setTimeout(r,1200));assert.equal(requests.length,before,'Candidate must not poll or send UI events: '+JSON.stringify(requests.slice(before)));
  assert.equal(await frame.locator('#log-query').inputValue(),'UNIQUE','candidate restores the fixed query from the original capture');
  await page.evaluate(()=>{window.candidate=false;window.post('surfaceReady');window.post('flush',{id:'move',freeze:true});});await page.waitForFunction(()=>window.flushed.some(f=>f.id==='move'));
  const frozen=requests.length;await new Promise(r=>setTimeout(r,1200));assert.equal(requests.length,frozen,'Frozen surface must not keep polling');
  await page.evaluate(()=>window.post('transferCancelled'));await new Promise(r=>setTimeout(r,1200));assert(requests.length>frozen);
  snapshot={...snapshot,containerId:second,name:'other-container',logCapture:{...logCapture,generation:2,navigation:{}}};logCapture=snapshot.logCapture;await page.evaluate(s=>window.post('mount',{data:s,tokens:window.tokens,readOnly:false}),snapshot);await frame.locator('#title').filter({hasText:'other-container'}).waitFor();assert.equal(await frame.locator('#log-query').inputValue(),'');assert.equal(await frame.locator('#log-range').inputValue(),'latest');assert(!(await frame.locator('#log-only').isChecked()),'another container starts without prior filters');
  assert.deepEqual(errors,[]);console.log('Docker UI: inline maintenance, terminal, refresh focus, filters, keyboard, confirmation, environment visibility, logs, stats, themes, narrow layouts and transfer passed.');
}finally{await browser.close();await new Promise(r=>server.close(r));}
