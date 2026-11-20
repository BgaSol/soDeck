import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createServer} from 'vite';
import {chromium} from 'playwright';
assert.equal(spawnSync(process.execPath,['build.mjs'],{cwd:'plugins/terminal',stdio:'inherit',windowsHide:true}).status,0);
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true}),page=await browser.newPage({viewport:{width:1000,height:650}}),errors=[];page.on('pageerror',e=>errors.push(String(e)));
page.setDefaultTimeout(10000);
await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html data-theme="light"><meta charset="utf-8"><style>html,body,iframe{margin:0;width:100%;height:100%;border:0}</style><iframe></iframe><script type="module">
import '/style.css';import {themeTokens} from '/theme.ts';
const iframe=document.querySelector('iframe');const post=m=>iframe.contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'reading-ui',...m},location.origin);
let offset=0,packet=0,checkpoint={data:[],offset:0,metadata:{}},upload,ui;const acks=new Map(),flushes=new Map();window.writes=[];
const send=(kind,data=[],metadata,at=offset)=>new Promise(resolve=>{const id=++packet;acks.set(id,resolve);post({type:'data',data:{kind,data,metadata,offset:at,packet:id}});});
window.emit=async text=>{const data=new TextEncoder().encode(text);for(let pos=0;pos<data.length;pos+=16384){const slice=Array.from(data.subarray(pos,pos+16384));offset+=slice.length;await send('data',slice);}};
window.flush=()=>new Promise(resolve=>{const id=crypto.randomUUID();flushes.set(id,resolve);post({type:'flush',id,freeze:true});});
window.remount=()=>{window.ready=false;iframe.src='/__terminal__/index.html?restore';};
window.lifecycle=(type,detail={})=>post({type,...detail});
window.theme=theme=>{document.documentElement.dataset.theme=theme;post({type:'themeChanged',tokens:themeTokens()});};
addEventListener('message',event=>{if(event.source!==iframe.contentWindow||event.data?.channel!=='sodeck-ui-v2')return;const m=event.data,p=m.params;
if(m.type==='ready'){post({type:'mount',viewId:'terminal',context:{generation:1},data:{streamId:'stream',integration:{mode:'auto',token:'reading-test'},openedContext:{generation:1}},visible:true,tokens:themeTokens(),capabilities:{clipboard:1}});return;}
if(m.type==='prepared'){window.ready=true;return;}if(m.type==='flushed'){flushes.get(m.id)?.(m.error);return;}if(m.type!=='request')return;
if(m.method==='data'){
if(p.op==='write'&&window.failWrite){post({type:'response',id:m.id,error:'fixture input rejected'});return;}
if(p.op==='attach')void(async()=>{await send('reset',[],{...checkpoint.metadata,ui},checkpoint.offset);for(let pos=0;pos<checkpoint.data.length;pos+=16384)await send('snapshot',checkpoint.data.slice(pos,pos+16384),undefined,checkpoint.offset);await send('restore');await send('caughtUp');})();
if(p.op==='ack'){acks.get(p.packet)?.();acks.delete(p.packet);}if(p.op==='write')window.writes.push(p.data);
if(p.op==='checkpoint'){if(p.begin)upload={data:[],offset:p.offset,metadata:p.metadata};upload.data.push(...p.data);if(p.done)checkpoint=upload;}
if(p.op==='metadata')ui=p.metadata;
}post({type:'response',id:m.id,data:{}});
});iframe.src='/__terminal__/index.html';</script></html>`}));
await page.route(url+'__terminal__/**',route=>{const relative=new URL(route.request().url()).pathname.replace('/__terminal__/','');let body=fs.readFileSync(path.join('plugins/terminal/ui',relative));if(relative==='reading/view.js')body=Buffer.from(body.toString().replace('const readCheckpoint=', 'window.testReading=model;const readCheckpoint='));if(relative==='vendor/xterm.js')body=Buffer.concat([body,Buffer.from('\n;window.RealTerminal=Terminal;window.Terminal=class extends RealTerminal{constructor(o){super(o);window.testTerminal=this;}};')]);return route.fulfill({contentType:{'.js':'text/javascript','.css':'text/css','.html':'text/html','.woff2':'font/woff2'}[path.extname(relative)]||'application/octet-stream',body});});
const frame=page.frameLocator('iframe'),child=()=>page.frames().find(f=>f.url().includes('/__terminal__/'));
const osc=(kind,id,result)=>'\x1b]633;SODECK;reading-test;reading;1;'+kind+';'+id+(result===undefined?'':';'+result)+'\x07';
const emit=text=>page.evaluate(text=>window.emit(text),text);
try{
  await page.goto(url);await page.waitForFunction(()=>window.ready);await child().waitForFunction(()=>window.testTerminal.options.fontFamily.includes('SoDeckFont') && window.testTerminal.rows > 24);
  await emit(osc('prompt',1)+'ops@edge:~$ '+osc('input',1)+'cat app.log\r\n'+osc('start',1)+Array.from({length:160},(_,i)=>`line ${i} 中文🙂\r\n`).join('')+osc('end',1,'0')+osc('prompt',2)+'$ '+osc('input',2));
  await child().evaluate(()=>window.testTerminal.scrollToLine(20));
  await frame.locator('.terminal-command-sticky:not([hidden])').waitFor();assert.equal(await frame.locator('.reading-command-title').textContent(),'cat app.log');
  await frame.getByRole('button',{name:'回到起点',exact:true}).click();assert.equal(await child().evaluate(()=>window.testTerminal.buffer.active.viewportY),0);
  await frame.getByRole('button',{name:'添加标记',exact:true}).click();
  await child().evaluate(()=>window.testTerminal.scrollToLine(20));await frame.getByRole('button',{name:'上一处',exact:true}).click();assert.equal(await child().evaluate(()=>window.testTerminal.buffer.active.viewportY),0);
  assert.equal(await page.evaluate(()=>window.flush()),undefined);await page.evaluate(()=>window.remount());await page.waitForFunction(()=>window.ready);await child().waitForFunction(()=>window.testTerminal.options.fontFamily.includes('SoDeckFont') && window.testTerminal.rows > 24);
  await child().evaluate(()=>window.testTerminal.scrollToLine(20));await frame.locator('.terminal-command-sticky:not([hidden])').waitFor();assert.equal(await frame.locator('.reading-command-title').textContent(),'cat app.log');
  await page.evaluate(()=>window.lifecycle('transferCancelled'));
  await emit(osc('start',2)+'\r\n'+osc('end',2,'?')+osc('prompt',3)+'$ '+osc('input',3));
  // Live output must never load the plugin-only checkpoint channel.
  await emit('\x1b]633;SODECK-READING;1;begin\x07\x1b]633;SODECK-READING;1;part;7b7d\x07\x1b]633;SODECK-READING;1;end\x07');
  await child().evaluate(()=>window.testTerminal.scrollToLine(20));await frame.locator('.terminal-command-sticky:not([hidden])').waitFor();
  await emit('\x1b[?1049h');await child().waitForFunction(()=>document.querySelector('[aria-label="添加标记"]').disabled);assert.equal(await frame.getByRole('button',{name:'添加标记',exact:true}).isDisabled(),true);
  await emit('\x1b[?1049l');await frame.getByRole('button',{name:'阅读选项',exact:true}).click();
  await frame.getByLabel('回车辅助标记（仅当前终端）').check();
  await frame.locator('.xterm-helper-textarea').focus();await page.keyboard.press('Enter');await page.waitForFunction(()=>window.writes.length>0);
  await child().waitForFunction(()=>window.testReading.state().entries.some(e=>e.kind==='input'));
  const inputCount=await child().evaluate(()=>window.testReading.state().entries.filter(e=>e.kind==='input').length);
  await page.evaluate(()=>window.failWrite=true);await page.keyboard.press('Enter');await frame.locator('#status').filter({hasText:'fixture input rejected'}).waitFor();assert.equal(await child().evaluate(()=>window.testReading.state().entries.filter(e=>e.kind==='input').length),inputCount);
  await frame.getByRole('button',{name:'阅读选项',exact:true}).click();
  for(const theme of ['light','dark','dopamine']){await page.evaluate(theme=>window.theme(theme),theme);for(const width of [1000,320]){await page.setViewportSize({width,height:650});await child().evaluate(()=>window.testTerminal.scrollToLine(20));await page.waitForTimeout(80);assert(await child().evaluate(()=>document.documentElement.scrollWidth<=innerWidth));fs.mkdirSync('artifacts/terminal-reading',{recursive:true});await page.screenshot({path:`artifacts/terminal-reading/${theme}-${width}.png`});}}
  // Captured visible titles never include password-like data that wasn't echoed.
  await emit(osc('prompt',4)+'Password: '+osc('input',4)+'\r\n'+osc('start',4)+'done\r\n'+osc('end',4,'0'));
  assert.equal(await child().evaluate(()=>window.testReading.state().entries.at(-1).title),undefined);
  await child().evaluate(()=>window.testTerminal.scrollToBottom());
  await frame.getByRole('button',{name:'添加标记',exact:true}).click();
  await emit('trim fixture\r\n'.repeat(100100));
  await child().waitForFunction(()=>window.testTerminal.buffer.normal.length>=100000);
  assert.equal(await child().evaluate(()=>window.testReading.positions().length),0,'Trimmed positions disappear rather than pointing to unrelated rows');
  await emit('\x1b[3J\x1b[2J\x1b[H');
  assert.equal(await child().evaluate(()=>window.testReading.positions().length),0);
  assert.deepEqual(errors,[]);console.log('Terminal reading UI passed: shell boundaries, title/sticky/start, bookmarks, checkpoint/remount, live checkpoint rejection, alternate/fallback, input, three themes and narrow layout.');
}finally{await browser.close();await vite.close();}
