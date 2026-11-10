import assert from 'node:assert/strict';
import fs from 'node:fs';import path from 'node:path';
import {spawnSync} from 'node:child_process';import {createServer} from 'vite';import {chromium} from 'playwright';
const build=spawnSync(process.execPath,['build.mjs'],{cwd:'plugins/terminal',stdio:'inherit',windowsHide:true});assert.equal(build.status,0);
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1000,height:700}}),errors=[];page.on('pageerror',error=>errors.push(String(error)));
await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html data-theme="light"><meta charset="utf-8"><style>html,body{margin:0;height:100%;}iframe{border:0;width:100%;height:100%;}</style><iframe></iframe><script type="module">
import '/style.css';import {themeTokens} from '/theme.ts';
const frame=document.querySelector('iframe'),token='history-test';const post=message=>frame.contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:token,...message},location.origin);let offset=0,packet=0,checkpoint={offset:0,data:[],metadata:{}},upload,ui;const acks=new Map(),flushes=new Map();window.writes=[];window.metadata=[];
const send=async(kind,data=[],metadata,at=offset)=>{const id=++packet;await new Promise(resolve=>{acks.set(id,resolve);post({type:'data',data:{kind,offset:at,packet:id,data,metadata}});});};
window.recovery=async available=>{await send('recovery',[],{available});if(!available)await send('caughtUp');};
window.flush=()=>new Promise(resolve=>{const id=crypto.randomUUID();flushes.set(id,resolve);post({type:'flush',id,freeze:false});});
window.remount=()=>{window.savedForRemount=structuredClone(ui);window.ready=false;frame.src='/__terminal__/index.html?remount';};
window.emit=async text=>{const bytes=new TextEncoder().encode(text);for(let pos=0;pos<bytes.length;pos+=16384){const data=Array.from(bytes.subarray(pos,pos+16384));offset+=data.length;const id=++packet;await new Promise(resolve=>{acks.set(id,resolve);post({type:'data',data:{kind:'data',offset,packet:id,data}});});}};
window.setTheme=theme=>{document.documentElement.dataset.theme=theme;post({type:'themeChanged',tokens:themeTokens()});};
addEventListener('message',event=>{if(event.source!==frame.contentWindow||event.data?.channel!=='sodeck-ui-v2')return;const m=event.data;if(m.type==='ready'){post({type:'mount',viewId:'A',context:{generation:1},data:{streamId:'pty'},tokens:themeTokens(),capabilities:{clipboard:1},visible:true});return;}if(m.type==='prepared'){window.ready=true;return;}if(m.type==='flushed'){flushes.get(m.id)?.(m.error);flushes.delete(m.id);return;}if(m.type!=='request')return;const p=m.params;if(m.method==='data'){if(p.op==='attach')void(async()=>{await send('reset',[],{...checkpoint.metadata,ui},checkpoint.offset);for(let pos=0;pos<checkpoint.data.length;pos+=16384)await send('snapshot',checkpoint.data.slice(pos,pos+16384),undefined,checkpoint.offset);await send('restore',[],undefined,checkpoint.offset);await send('caughtUp');})();if(p.op==='ack'){acks.get(p.packet)?.();acks.delete(p.packet);}if(p.op==='write')window.writes.push(p.data);if(p.op==='metadata'){window.metadata.push(p.metadata);ui=p.metadata;}if(p.op==='checkpoint'){if(p.begin)upload={offset:p.offset,data:[],metadata:p.metadata};upload.data.push(...p.data);if(p.done){checkpoint=upload;upload=undefined;}}}post({type:'response',id:m.id,data:{}});});frame.src='/__terminal__/index.html';
</script></html>`}));
await page.route(url+'__terminal__/**',route=>{const relative=new URL(route.request().url()).pathname.replace('/__terminal__/',''),filename=path.join('plugins/terminal/ui',relative);let body=fs.readFileSync(filename);if(relative==='vendor/xterm.js')body=Buffer.concat([body,Buffer.from('\n;window.RealTerminal=window.Terminal;window.Terminal=class extends window.RealTerminal{constructor(options){super(options);window.testTerminal=this;}};')]);return route.fulfill({contentType:{'.js':'text/javascript','.css':'text/css','.html':'text/html','.woff2':'font/woff2'}[path.extname(filename)]||'application/octet-stream',body});});
const child=()=>page.frames().find(f=>f.url().includes('/__terminal__/index.html')),frame=page.frameLocator('iframe');
const count=frame.locator('.search-count'),input=frame.locator('.terminal-search input');
const query=async text=>{await input.fill(text);await count.filter({hasText:/\d|没有匹配/}).waitFor();};
const searchAlignment=async()=>{
  const geometry=await frame.locator('.terminal-search').evaluate(panel=>{
    const rect=el=>{const r=el.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2,width:r.width,height:r.height};};
    return {input:rect(panel.querySelector('input')),count:rect(panel.querySelector('.search-count')),buttons:[...panel.querySelectorAll('.search-button')].map(el=>({button:rect(el),icon:el.querySelector('svg')?rect(el.querySelector('svg')):null}))};
  });
  for(const {button,icon}of geometry.buttons){
    assert.equal(button.width,button.height,'Search controls use square hit areas');
    assert.equal(button.height,geometry.input.height,'Input and search buttons have the same height');
    if(icon){assert(Math.abs(icon.x-button.x)<.25,'Search icon is horizontally centered');assert(Math.abs(icon.y-button.y)<.25,'Search icon is vertically centered');}
    if(page.viewportSize().width>310)assert(Math.abs(button.y-geometry.input.y)<.25,'Search controls share the input centerline');
  }
  if(page.viewportSize().width>600)assert(Math.abs(geometry.count.y-geometry.input.y)<.25,'Match count shares the input centerline');
  if(page.viewportSize().width<=310){const positions=geometry.buttons.map(({button})=>button.x);assert(Math.abs((positions[1]-positions[0])-(positions[3]-positions[2]))<.25,'Stacked search buttons are evenly spaced');}
};
try{
  await page.goto(url);await page.waitForFunction(()=>window.ready);await frame.locator('.xterm-helper-textarea').focus();
  await page.evaluate(()=>window.emit(Array.from({length:120},(_,id)=>`\x1b[32mdw@dw:~$\x1b[0m journalctl --no-pager · ${id}\r\n`).join('')));
  fs.mkdirSync('artifacts/terminal-search',{recursive:true});
  for(const theme of ['dark','light','dopamine']){
    await page.evaluate(theme=>window.setTheme(theme),theme);await page.waitForTimeout(250);
    await page.screenshot({path:`artifacts/terminal-search/minimap-${theme}.png`});
    assert.equal(await frame.locator('.minimap-viewport').evaluate(el=>getComputedStyle(el).backgroundColor),'rgba(0, 0, 0, 0)',`${theme}: reading position must not cover the terminal preview with a theme selection fill`);
    await page.keyboard.press('Control+Shift+f');await input.fill('journalctl');await count.filter({hasText:/\d+ \/ 120/}).waitFor();
    await page.mouse.move(0,0);await frame.locator('.terminal-search').screenshot({path:`artifacts/terminal-search/popover-${theme}.png`});await searchAlignment();
    await input.fill('missing-layout-query');await count.filter({hasText:'没有匹配'}).waitFor();await searchAlignment();
    await frame.locator('[aria-label="关闭搜索（Esc）"]').click();
  }
  await page.evaluate(async()=>{await window.emit(Array.from({length:100500},(_,id)=>`${String(id).padStart(6,'0')} ${[14320,41982,88127].includes(id)?`\x1b[31mERROR ${id===41982?'TIMEOUT':'timeout'} 中文🙂\x1b[0m`:'INFO heartbeat received'}\r\n`).join(''));});
  await page.keyboard.press('Control+Shift+f');await input.waitFor();await input.fill('timeout');await count.filter({hasText:'1 / 3'}).waitFor();
  assert.equal((await child().evaluate(()=>window.testTerminal.options.scrollback)),100000);
  await page.keyboard.press('Enter');await count.filter({hasText:'2 / 3'}).waitFor();await page.keyboard.press('Shift+Enter');await count.filter({hasText:'1 / 3'}).waitFor();
  await input.fill('TIMEOUT');await count.filter({hasText:'1 / 3'}).waitFor();await frame.locator('.search-case').click();await count.filter({hasText:'1 / 1'}).waitFor();
  await frame.locator('.search-case').click();await input.fill('中文🙂');await count.filter({hasText:/[123] \/ 3/}).waitFor();
  await input.fill('does-not-exist');await count.filter({hasText:'没有匹配'}).waitFor();await input.fill('timeout');await count.filter({hasText:/[123] \/ 3/}).waitFor();
  const timing=await child().evaluate(async()=>{
    const input=document.querySelector('.terminal-search input'),count=document.querySelector('.search-count'),samples=[];
    const terms=[...Array.from({length:25},(_,n)=>({kind:'rare',query:String(14000+n)})),...Array.from({length:25},(_,n)=>({kind:'missing',query:`missing-${n}`})),...Array.from({length:25},(_,n)=>({kind:'common',query:'INFO heartbeat received'.slice(Math.floor(n/5),Math.floor(n/5)+5+n%5)})),...'abcdefghijklmnopqrstuvwxyz'.slice(0,25).split('').map(query=>({kind:'short',query}))];
    if(new Set(terms.map(t=>t.query)).size!==100)throw new Error('Benchmark queries must be distinct');
    for(const term of terms){const start=performance.now();await new Promise((resolve,reject)=>{let began=false;const deadline=setTimeout(()=>{observer.disconnect();reject(new Error('Search timing timeout: '+term.query));},5000);const observer=new MutationObserver(()=>{const text=count.textContent;if(text.includes('正在查找'))began=true;if(began&&(text==='没有匹配'||/^\d+ \/ [\d,]+$/.test(text))){observer.disconnect();clearTimeout(deadline);resolve();}});observer.observe(count,{childList:true,subtree:true,characterData:true});input.value=term.query;input.dispatchEvent(new Event('input',{bubbles:true}));});samples.push({...term,ms:performance.now()-start});}
    const stats=values=>{const sorted=values.slice().sort((a,b)=>a-b);return {p50:sorted[Math.floor(sorted.length*.5)],p95:sorted[Math.ceil(sorted.length*.95)-1],max:sorted.at(-1)};};return {browser:navigator.userAgent,rows:window.testTerminal.buffer.active.length,cols:window.testTerminal.cols,queries:terms.length,groups:Object.fromEntries(['rare','missing','common','short'].map(kind=>[kind,stats(samples.filter(s=>s.kind===kind).map(s=>s.ms))])),samples};
  });fs.mkdirSync('artifacts/terminal-history',{recursive:true});fs.writeFileSync('artifacts/terminal-history/ui-search.json',JSON.stringify(timing,null,2)+'\n');
  await input.fill('timeout');await count.filter({hasText:/[123] \/ 3/}).waitFor();
  const before=await child().evaluate(()=>window.testTerminal.buffer.active.getLine(window.testTerminal.buffer.active.viewportY+Math.floor(window.testTerminal.rows/2)).translateToString(true));
  await page.evaluate(()=>window.emit('new output\r\n'.repeat(300)));await count.filter({hasText:/[123] \/ 3/}).waitFor();const after=await child().evaluate(()=>window.testTerminal.buffer.active.getLine(window.testTerminal.buffer.active.viewportY+Math.floor(window.testTerminal.rows/2)).translateToString(true));assert.equal(after,before,'Output must preserve the reading position');
  await input.focus();await page.keyboard.press('Escape');assert.equal(await frame.locator('.terminal-search').isVisible(),false);assert.equal(await frame.locator('.terminal-minimap').isVisible(),true);
  await page.keyboard.press('Control+f');await page.waitForFunction(()=>window.writes.flat().includes(6));assert.equal(await page.evaluate(()=>window.writes.flat().filter(b=>b===6).length),1);
  await page.keyboard.press('Control+Shift+f');await input.waitFor();assert.equal(await input.inputValue(),'timeout');await count.filter({hasText:/[123] \/ 3/}).waitFor();
  await input.fill('saved');await count.filter({hasText:'没有匹配'}).waitFor();await page.evaluate(()=>window.emit('saved saved saved\r\n'));await count.filter({hasText:'1 / 3'}).waitFor();await input.press('Enter');await count.filter({hasText:'2 / 3'}).waitFor();
  assert.equal(await page.evaluate(()=>window.flush()),undefined);await page.evaluate(()=>window.remount());await page.waitForFunction(()=>window.ready);await count.filter({hasText:'2 / 3'}).waitFor();assert.equal(await input.inputValue(),'saved','Same-row current result survives remount');
  // Composition must not start queries or send Enter to the shell.
  await input.dispatchEvent('compositionstart');await input.fill('composition-only');await input.press('Enter');assert.match(await count.textContent(),/2 \/ 3/);await input.dispatchEvent('compositionend');await count.filter({hasText:'没有匹配'}).waitFor();
  await input.fill('saved');await count.filter({hasText:'1 / 3'}).waitFor();await input.evaluate(el=>{for(let i=0;i<7;i++)el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));});await count.filter({hasText:'2 / 3'}).waitFor();
  await input.press('Escape');const rail=frame.locator('.terminal-minimap'),thumb=frame.locator('.minimap-viewport');await thumb.focus();await thumb.press('Home');await thumb.press('ArrowDown');assert.equal(await child().evaluate(()=>window.testTerminal.buffer.active.viewportY),3);await thumb.press('End');
  const box=await rail.boundingBox();await page.mouse.click(box.x+box.width/2,box.y+box.height/2);const middle=await child().evaluate(()=>window.testTerminal.buffer.active.viewportY);assert(middle>40000&&middle<60000,'Minimap click maps to history rows');const drag=await thumb.boundingBox();await page.mouse.move(drag.x+drag.width/2,drag.y+drag.height/2);
  assert.equal(await thumb.evaluate(el=>getComputedStyle(el).backgroundColor),'rgba(0, 0, 0, 0)','Hovered viewport remains transparent');
  await page.mouse.down();await page.mouse.move(box.x-30,box.y+20);
  assert.equal(await rail.evaluate(el=>el.classList.contains('is-dragging')),true,'Drag feedback persists outside the preview');
  assert.deepEqual(await thumb.evaluate(el=>{const style=getComputedStyle(el);return [style.backgroundColor,style.cursor];}),['rgba(0, 0, 0, 0)','grabbing'],'Dragging viewport remains transparent with a grabbing cursor');
  await page.mouse.up();assert.equal(await rail.evaluate(el=>el.classList.contains('is-dragging')),false,'Releasing outside clears drag feedback');assert((await child().evaluate(()=>window.testTerminal.buffer.active.viewportY))<middle,'Minimap drag updates reading position');
  await thumb.focus();assert.equal(await thumb.evaluate(el=>getComputedStyle(el).outlineOffset),'-2px','Keyboard focus stays inside the clipped preview');
  await page.keyboard.press('Control+Shift+f');await input.fill('timeout');await count.filter({hasText:/[123] \/ 3/}).waitFor();
  fs.mkdirSync('artifacts/terminal-search',{recursive:true});
  await frame.locator('[aria-label="下一处（Enter）"]').click();await count.filter({hasText:/[123] \/ 3/}).waitFor();
  for(const theme of ['light','dark','dopamine']){await page.evaluate(theme=>window.setTheme(theme),theme);await page.waitForTimeout(250);await page.screenshot({path:`artifacts/terminal-search/${theme}.png`});}
  await page.setViewportSize({width:320,height:700});await input.waitFor();await page.waitForTimeout(250);await count.filter({hasText:/[123] \/ [123]/}).waitFor();await searchAlignment();await page.screenshot({path:'artifacts/terminal-search/narrow.png'});
  const overflow=await child().evaluate(()=>{const p=document.querySelector('.terminal-search').getBoundingClientRect(),host=document.querySelector('#terminal').getBoundingClientRect();return p.right>host.right||p.left<host.left;});assert.equal(overflow,false,'Narrow floating search must remain within the terminal');
  await page.setViewportSize({width:240,height:700});await page.waitForTimeout(250);await count.filter({hasText:/\d|没有匹配/}).waitFor();await searchAlignment();
  assert.equal(await frame.locator('.terminal-search').evaluate(panel=>{const p=panel.getBoundingClientRect();return [...panel.querySelectorAll('input,.search-button,.search-count')].some(el=>{const r=el.getBoundingClientRect();return r.left<p.left||r.right>p.right;});}),false,'Stacked search controls stay inside the popup');
  await frame.locator('.terminal-search').screenshot({path:'artifacts/terminal-search/popover-small.png'});
  await page.setViewportSize({width:1000,height:700});
  const write=async text=>child().evaluate(text=>new Promise(resolve=>window.testTerminal.write(text,resolve)),text);
  await write('\x1b[?1049h\x1b[2J\x1b[Halternate needle');await input.fill('needle');await count.filter({hasText:'1 / 1'}).waitFor();assert.equal(await frame.locator('.search-scope').textContent(),'当前应用屏幕');await write('\x1b[?1049l');await count.filter({hasText:'没有匹配'}).waitFor();
  await write('\x1b[3J\x1b[2J\x1b[Hold needle\rnew text\x1b[K');await count.filter({hasText:'没有匹配'}).waitFor();await input.fill('new text');await count.filter({hasText:'1 / 1'}).waitFor();
  await write('\x1b[H\x1b[Lreplacement\x1b[K');await count.filter({hasText:'1 / 1'}).waitFor();await write('\x1b[2;1H\x1b[M');await count.filter({hasText:'没有匹配'}).waitFor();
  await write('\x1b[H'+ 'x'.repeat(await child().evaluate(()=>window.testTerminal.cols-3))+'WRAPNEEDLE\r\n');await input.fill('WRAPNEEDLE');await count.filter({hasText:'1 / 1'}).waitFor();await page.setViewportSize({width:700,height:700});await count.filter({hasText:'1 / 1'}).waitFor();await page.waitForTimeout(250);assert.match(await count.textContent(),/1 \/ 1/,'Resize reflow preserves a completed soft-wrap match');
  await page.evaluate(()=>window.recovery(false));await frame.locator('#status').filter({hasText:'历史恢复暂不可用'}).waitFor();await page.evaluate(()=>window.recovery(true));assert.equal(await frame.locator('#status').isVisible(),false,'Recovery warning clears only when recovery resumes');
  assert.deepEqual(errors,[]);console.log('Terminal search UI passed: 100k history, Unicode/case/navigation, remount current result, IME, minimap gestures/keys, live reading anchor, shell shortcut, alternate/erase/insert/delete/reflow, themes and narrow layout.');
}catch(error){console.log(JSON.stringify({count:await count.textContent(),query:await input.inputValue(),saved:await page.evaluate(()=>window.savedForRemount),errors}));fs.mkdirSync('artifacts/terminal-search',{recursive:true});await page.screenshot({path:'artifacts/terminal-search/failure.png'});throw error;}finally{await browser.close();await vite.close();}
