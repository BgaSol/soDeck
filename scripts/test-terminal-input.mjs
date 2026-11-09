// Shipped xterm + terminal UI + real host clipboard transport, with deterministic clipboard/PTY endpoints.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createServer} from 'vite';
import {chromium} from 'playwright';
const build=spawnSync(process.execPath,['build.mjs'],{cwd:'plugins/terminal',stdio:'inherit',windowsHide:true});
if(build.status!==0)throw new Error('Terminal UI build failed; install its locked dependencies first');
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1024,height:700}});
const desktopArg=process.argv.indexOf('--desktop-script');
if(desktopArg!==-1)await page.addInitScript({content:fs.readFileSync(process.argv[desktopArg+1],'utf8')});
const errors=[];page.on('pageerror',error=>errors.push(String(error)));
let requestedPlatform='Win32';
const harness=`<!doctype html><html data-theme="dopamine"><head><meta charset="utf-8"></head><body><button id="outside">外部焦点</button><iframe id="terminal" style="width:900px;height:480px;border:0"></iframe><script type="module">
import '/style.css';import {ClipboardBridge} from '/clipboard.ts';import {themeTokens} from '/theme.ts';
window.writes=[];window.clipboard='中文🙂\\nsecond line';window.copies=[];window.reads=0;window.delayRead=false;window.flushed=[];
window.resizes=[];window.holdResize=false;
const host=new ClipboardBridge(async()=>{window.reads++;return window.delayRead?new Promise(resolve=>window.finishRead=resolve):window.clipboard;},async text=>window.copies.push(text));
const frame=document.querySelector('iframe'),token='test-surface';let generation=1,visible=true,offset=0;
const post=message=>frame.contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:token,...message},location.origin);
window.testPost=post;window.setTheme=theme=>{document.documentElement.dataset.theme=theme;post({type:'themeChanged',tokens:themeTokens()});};
window.addEventListener('message',async event=>{
 if(event.source!==frame.contentWindow||event.data?.channel!=='sodeck-ui-v2')return;
 const message=event.data;
 if(message.type==='ready'){post({type:'mount',viewId:'A',context:{generation},data:{streamId:'pty'},tokens:themeTokens(),capabilities:{clipboard:1},visible:true});return;}
 if(message.type==='prepared'){window.ready=true;return;}
 if(message.type==='flushed'){window.flushed.push(message.id);return;}
 if(message.type!=='request')return;
 try{
  let data={};
  if(message.method==='clipboard'){const revision=generation;data=await host.request(token,message.params,()=>visible&&generation===revision&&document.hasFocus()&&document.activeElement===frame);}
  if(message.method==="settings"){window.appearanceReports?.push(message.params);}
  if(message.method==='data'){
   const p=message.params;
   if(p.op==='write')window.writes.push(p.data);
   if(p.op==='resize'){window.resizes.push({cols:p.cols,rows:p.rows});if(window.holdResize){window.resizeWaiting=true;await new Promise(resolve=>window.finishResize=()=>{window.holdResize=false;window.resizeWaiting=false;resolve();});}}
   if(p.op==='attach'){post({type:'data',data:{kind:'caughtUp',packet:1,offset:0,data:[]}});}
  }
  post({type:'response',id:message.id,data});
 }catch(error){post({type:'response',id:message.id,error:String(error)});}
});
frame.src='/__terminal__/index.html';
window.hideTerminal=()=>{visible=false;generation++;host.discard(token);post({type:'visibilityChanged',visible:false});};
window.showTerminal=()=>{visible=true;post({type:'visibilityChanged',visible:true});};
</script></body></html>`;
await page.route(url,route=>route.fulfill({contentType:'text/html',body:harness}));
let rejectSourceCodeFont=false;
await page.route(url+'__terminal__/**',route=>{
 const relative=new URL(route.request().url()).pathname.replace('/__terminal__/','');const filename=path.join('plugins/terminal/ui',relative);
 if(rejectSourceCodeFont&&relative.startsWith('fonts/source-code-pro-'))return route.abort();
 let body=fs.readFileSync(filename);if(relative==='vendor/xterm.js')body=Buffer.concat([body,Buffer.from('\n;window.RealTerminal=window.Terminal;window.Terminal=class extends window.RealTerminal{constructor(options){super(options);window.testTerminal=this;}};')]);
 if(relative==='index.html')body=Buffer.from(body.toString().replace('<body>',`<body><script>Object.defineProperty(navigator,'platform',{configurable:true,value:'${requestedPlatform}'});Object.defineProperty(navigator,'userAgentData',{configurable:true,value:undefined});</script>`));
 return route.fulfill({contentType:{'.js':'text/javascript','.css':'text/css','.html':'text/html'}[path.extname(filename)],body});
});
const frame=page.frameLocator('iframe');
const bytes=()=>page.evaluate(()=>new TextDecoder().decode(Uint8Array.from(window.writes.flat())));
const reset=()=>page.evaluate(()=>{window.writes=[];window.copies=[];window.reads=0;window.delayRead=false;});
const focus=()=>frame.locator('.xterm-helper-textarea').focus();
const term=async code=>{const child=page.frames().find(f=>f.url().includes('/__terminal__/index.html'));return child.evaluate(code);};
try{
 await page.goto(url);await page.waitForFunction(()=>window.ready);await focus();
 await page.evaluate(()=>window.testPost({type:'flush',id:'sync',freeze:false}));await page.waitForFunction(()=>window.flushed.includes('sync'));
 await page.keyboard.type('after-sync');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'after-sync');await reset();
 await page.evaluate(()=>window.testPost({type:'flush',id:'move',freeze:true}));await page.waitForFunction(()=>window.flushed.includes('move'));
 await page.keyboard.type('frozen');assert.equal(await bytes(),'');await page.evaluate(()=>new Promise(resolve=>{document.querySelector('iframe').contentWindow.addEventListener('sodeck:transferCancelled',()=>resolve(),{once:true});window.testPost({type:'transferCancelled'});}));await focus();await page.keyboard.type('resumed');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'resumed');await reset();
 for(const [key,expected]of [['Control+r','\x12'],['Control+u','\x15'],['Control+p','\x10'],['F5','\x1b[15~'],['F12','\x1b[24~'],['Shift+Tab','\x1b[Z']]){
  await page.keyboard.press(key);await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),expected,`${key} still reaches the terminal`);await reset();
 }
 // Composition is owned by the IME; local clipboard shortcuts must not intervene.
 await frame.locator('.xterm-helper-textarea').dispatchEvent('compositionstart');
 await page.keyboard.press('Control+v');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await page.evaluate(()=>window.reads),0);
 await frame.locator('.xterm-helper-textarea').dispatchEvent('compositionend');await reset();
 await page.keyboard.press('Control+v');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'中文🙂\rsecond line');assert.equal(await page.evaluate(()=>window.reads),1);await reset();
 await term(()=>new Promise(resolve=>window.testTerminal.write('\x1b[?2004h',resolve)));await page.keyboard.press('Control+Shift+v');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'\x1b[200~中文🙂\rsecond line\x1b[201~');await reset();
 await term(()=>new Promise(resolve=>window.testTerminal.write('copy 中文🙂',resolve)));await term(()=>window.testTerminal.select(0,0,5));await page.keyboard.press('Control+c');await page.waitForFunction(()=>window.copies.length===1);assert.equal((await page.evaluate(()=>window.copies))[0],'copy ');assert.equal(await bytes(),'');assert.equal(await term(()=>window.testTerminal.hasSelection()),false);
 await page.keyboard.press('Control+c');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'\x03');await reset();
 // Menu actions do not read early and Escape does not reach the remote program.
 await frame.locator('#terminal').click({button:'right'});assert.equal(await page.evaluate(()=>window.reads),0);await page.keyboard.press('Escape');assert.equal(await frame.locator('.terminal-menu').count(),0);assert.equal(await bytes(),'');
 await frame.locator('#terminal').click({button:'right'});await frame.getByRole('menuitem',{name:/^粘贴/}).click();await page.waitForFunction(()=>window.writes.length>0);assert.equal(await page.evaluate(()=>window.reads),1);await reset();
 // Mouse-reporting programs own right click; Shift explicitly opens the local menu.
 await term(()=>new Promise(resolve=>window.testTerminal.write('\x1b[?1000h',resolve)));
 await frame.locator('#terminal').click({button:'right'});assert.equal(await frame.locator('.terminal-menu').count(),0);assert.equal(await page.evaluate(()=>window.reads),0);
 await frame.locator('#terminal').click({button:'right',modifiers:['Shift']});await frame.getByRole('menu').waitFor();await page.keyboard.press('Escape');
 await term(()=>new Promise(resolve=>window.testTerminal.write('\x1b[?1000l',resolve)));await reset();
 // Auto-repeat and overlapping shortcut presses share one native read and one paste.
 await focus();await page.evaluate(()=>window.delayRead=true);await page.keyboard.down('Control');await page.keyboard.down('v');await page.keyboard.down('v');await page.keyboard.up('v');await page.keyboard.up('Control');
 await page.keyboard.press('Control+v');await page.waitForFunction(()=>Boolean(window.finishRead));assert.equal(await page.evaluate(()=>window.reads),1);
 await page.evaluate(()=>{window.finishRead('single-flight');window.finishRead=null;});await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'\x1b[200~single-flight\x1b[201~');await reset();
 // A late native clipboard response must never cross a visibility/context change.
 await focus();await page.evaluate(()=>window.delayRead=true);await page.keyboard.press('Control+v');await page.waitForFunction(()=>Boolean(window.finishRead));await page.evaluate(()=>{window.hideTerminal();window.finishRead('must-not-send');});await page.waitForTimeout(80);assert.equal(await bytes(),'');await page.evaluate(()=>window.showTerminal());await reset();await focus();
 await term(()=>new Promise(resolve=>window.testTerminal.write(Array.from({length:80},(_,i)=>`scrollback ${i}\r\n`).join(''),resolve)));
 await frame.locator('.xterm-screen').hover();await page.mouse.wheel(0,-300);
 const child=page.frames().find(f=>f.url().includes('/__terminal__/index.html'));
 await child.waitForFunction(()=>window.testTerminal.buffer.active.viewportY<window.testTerminal.buffer.active.baseY);
 const slider=await frame.locator('.xterm-scrollable-element > .scrollbar.vertical > .slider').evaluate(node=>{const css=getComputedStyle(node);return{visibleWidth:parseFloat(css.width)-parseFloat(css.borderLeftWidth)-parseFloat(css.borderRightWidth),radius:css.borderRadius,clip:css.backgroundClip};});
 assert.deepEqual(slider,{visibleWidth:6,radius:'10px',clip:'padding-box'});
 await term(()=>window.testTerminal.select(0,0,5));
 const geometry=()=>({cols:window.testTerminal.cols,rows:window.testTerminal.rows,selection:window.testTerminal.getSelection(),scroll:window.testTerminal.buffer.active.viewportY});
 const before=await term(geometry);
 for(const theme of ['light','dark','dopamine']){await page.evaluate(theme=>window.setTheme(theme),theme);await page.waitForTimeout(20);assert.deepEqual(await term(geometry),before);}
 assert.equal(await term(()=>window.testTerminal.options.theme.selectionBackground),'#c8bdf4');
 assert.equal(await term(()=>window.testTerminal.options.theme.scrollbarSliderBackground),'#92907b');
 assert.equal(await term(()=>window.testTerminal.options.theme.scrollbarSliderHoverBackground),'#6f715e');
 assert.equal(await term(()=>window.testTerminal.options.theme.scrollbarSliderActiveBackground),'#5346a6');assert.deepEqual(errors,[]);
 await page.evaluate(()=>window.appearanceReports=[]);
 const settings={pageId:'appearance',definitionToken:'test-appearance',revision:'1',values:{font:{source:'preset',id:'ibm-plex-mono'},fontSize:18,lineHeight:1.3,paddingBottom:40,cursorStyle:'bar',cursorBlink:false}};
 await page.evaluate(settings=>window.testPost({type:'settingsChanged',settings}),settings);
 await page.waitForFunction(()=>window.appearanceReports.some(r=>r.revision==='1'));
 const appearance=await term(()=>({size:window.testTerminal.options.fontSize,lineHeight:window.testTerminal.options.lineHeight,family:window.testTerminal.options.fontFamily,padding:document.querySelector('#terminal').style.paddingBottom,cursor:window.testTerminal.options.cursorStyle}));
 assert.equal(appearance.size,18);assert.equal(appearance.lineHeight,1.3);assert.equal(appearance.padding,'40px');assert.equal(appearance.cursor,'bar');assert(appearance.family.includes('SoDeckFont'));
 assert((await term(geometry)).rows<before.rows);assert.equal(await bytes(),'');
 await page.evaluate(settings=>window.testPost({type:'settingsChanged',settings:{...settings,revision:'0',values:{...settings.values,fontSize:12}}}),settings);
 assert.equal(await term(()=>window.testTerminal.options.fontSize),18);
 // A transient resource failure can be retried without saving another revision.
 rejectSourceCodeFont=true;
 const retrySettings={...settings,revision:'2',values:{...settings.values,font:{source:'preset',id:'source-code-pro'}}};
 await page.evaluate(settings=>window.testPost({type:'settingsChanged',settings}),retrySettings);
 await page.waitForFunction(()=>window.appearanceReports.some(r=>r.revision==='2'&&r.error));
 rejectSourceCodeFont=false;
 await page.evaluate(settings=>window.testPost({type:'settingsChanged',settings}),retrySettings);
 await page.waitForFunction(()=>window.appearanceReports.some(r=>r.revision==='2'&&!r.error));
 // A slow PTY resize must not hold local xterm geometry behind the input queue.
 const initialCols=await term(()=>window.testTerminal.cols);
 await page.evaluate(()=>{window.resizes=[];window.holdResize=true;document.querySelector('iframe').style.width='800px';});
 await page.waitForFunction(()=>window.resizeWaiting);
 await page.evaluate(async()=>{for(let i=0;i<20;i++){document.querySelector('iframe').style.width=(790-i*10)+'px';await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));}});
 await page.frames().find(f=>f.url().includes('/__terminal__/index.html')).waitForFunction(cols=>window.testTerminal.cols<cols-10,initialCols,{timeout:2000});
 const latestSize=await term(()=>({cols:window.testTerminal.cols,rows:window.testTerminal.rows}));
 const fittedBox=await term(()=>{const host=document.querySelector('#terminal'),style=getComputedStyle(host),screen=document.querySelector('.xterm-screen').getBoundingClientRect();return {width:screen.width,height:screen.height,rootHeight:document.querySelector('.xterm').getBoundingClientRect().height,availableWidth:host.clientWidth-parseFloat(style.paddingLeft)-parseFloat(style.paddingRight),availableHeight:host.clientHeight-parseFloat(style.paddingTop)-parseFloat(style.paddingBottom)};});
 assert(fittedBox.width<=fittedBox.availableWidth+1&&fittedBox.height<=fittedBox.availableHeight+1,`xterm must fit the actual content box: ${JSON.stringify(fittedBox)}`);
 assert(Math.abs(fittedBox.rootHeight-fittedBox.height)<=1,`xterm's viewport must end at the last complete row: ${JSON.stringify(fittedBox)}`);
 await reset();await focus();await page.keyboard.type('resize-input');
 await page.evaluate(()=>window.finishResize());await page.waitForFunction(()=>window.writes.length>0);
 await page.waitForFunction(size=>window.resizes.some(v=>v.cols===size.cols&&v.rows===size.rows),latestSize);
 assert.equal(await bytes(),'resize-input');assert((await page.evaluate(()=>window.resizes.length))<=2,'Only the in-flight and latest resize should be sent');
 await page.evaluate(()=>{window.holdResize=true;document.querySelector('iframe').style.width='500px';});await page.waitForFunction(()=>window.resizeWaiting);
 const hiddenCols=await term(()=>window.testTerminal.cols),hiddenRequests=await page.evaluate(()=>window.resizes.length);
 await page.evaluate(async()=>{window.hideTerminal();document.querySelector('iframe').style.width='450px';await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));window.finishResize();});
 assert.equal(await term(()=>window.testTerminal.cols),hiddenCols);assert.equal(await page.evaluate(()=>window.resizes.length),hiddenRequests);
 await page.evaluate(()=>window.showTerminal());await page.frames().find(f=>f.url().includes('/__terminal__/index.html')).waitForFunction(cols=>window.testTerminal.cols<cols,hiddenCols);
 await page.waitForFunction(()=>window.resizes.length>2);
 await page.evaluate(()=>{window.holdResize=true;document.querySelector('iframe').style.width='400px';});await page.waitForFunction(()=>window.resizeWaiting);
 const unmountRequests=await page.evaluate(()=>window.resizes.length);
 await page.evaluate(async()=>{window.testPost({type:'unmount'});await new Promise(resolve=>requestAnimationFrame(resolve));window.finishResize();document.querySelector('iframe').style.width='350px';await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));});
 assert.equal(await page.evaluate(()=>window.resizes.length),unmountRequests,'A late resize must not enqueue work after unmount');assert.deepEqual(errors,[]);
 for(const [platform,key]of [['Linux','Control+Shift+v'],['MacIntel','Meta+v']]){
   requestedPlatform=platform;await page.reload();await page.waitForFunction(()=>window.ready);await focus();await page.keyboard.press(key);await page.waitForFunction(()=>window.writes.length>0);assert.equal(await page.evaluate(()=>window.reads),1);await reset();
   if(platform==='Linux'){await page.keyboard.press('Control+v');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'\x16');assert.equal(await page.evaluate(()=>window.reads),0);}
   else {await page.keyboard.press('Control+c');await page.waitForFunction(()=>window.writes.length>0);assert.equal(await bytes(),'\x03');}
 }
 requestedPlatform='Win32';await page.reload();await page.waitForFunction(()=>window.ready);await focus();await frame.locator('#terminal').click({button:'right'});
 fs.mkdirSync('artifacts/terminal-ui',{recursive:true});await page.screenshot({path:'artifacts/terminal-ui/clipboard-menu.png'});
 console.log('Terminal input passed: real xterm copy/Ctrl+C/paste/bracketed paste, local menu, no duplicate input, late paste cancellation, theme keeps geometry; no page errors');
}catch(error){console.error({errors,frames:page.frames().map(f=>f.url())});throw error;}finally{await browser.close();await vite.close();}
