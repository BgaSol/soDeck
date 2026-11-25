import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {chromium} from 'playwright';

const server=await createServer({server:{host:'127.0.0.1',port:0}});await server.listen();
const browser=await chromium.launch({headless:true}),page=await browser.newPage();
const url=server.resolvedUrls.local[0],errors=[];page.on('pageerror',error=>errors.push(String(error)));
await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<html><body><script type="module">
import {DesktopUi} from '/desktop.ts';
window.state={generation:1,visible:true,blocked:false,navigation:{id:1,target:'messages'},background:null};
window.navigations=[];window.visibility=[];window.notices=[];window.calls=[];window.allow=true;
let sequence=0;const callbacks=new Map(),listeners=new Map();
window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:(_,id)=>listeners.delete(id)};
window.__TAURI_INTERNALS__={transformCallback:callback=>{const id=++sequence;callbacks.set(id,callback);return id;},invoke:async(command,args={})=>{
 calls.push({command,args});
 if(command==='plugin:event|listen'){const id=++sequence;listeners.set(id,args);return id;}
 if(command==='plugin:event|unlisten')return;
 if(command==='desktop_state'||command==='desktop_ready')return structuredClone(state);
 if(command==='desktop_navigation_done'){
   if(state.generation===args.generation&&state.navigation?.id===args.id)state.navigation=null;return;
 }
 if(command==='desktop_background_done'){if(window.failNative){window.failNative=false;throw Error('macOS presentation rejected');}if(state.background?.id===args.id){state.background=null;if(args.allow)state.visible=false;}return;}
 throw Error(command);
}};
window.ui=new DesktopUi({label:'main'},{setWindowVisible:v=>visibility.push(v)},target=>{navigations.push(target);if(window.replaceDuringNavigation){state.navigation={id:3,target:'messages'};window.replaceDuringNavigation=false;}},async()=>allow,message=>notices.push(message));
await ui.listen();await ui.ready();window.ready=true;
</script></body></html>`}));
try {
  await page.goto(url);await page.waitForFunction(()=>window.ready);
  assert.deepEqual(await page.evaluate(()=>navigations),['messages'],'Startup consumes queued tray navigation');
  await page.evaluate(async()=>{state.blocked=true;state.navigation={id:2,target:'settings'};await ui.refresh();});
  assert.equal(await page.evaluate(()=>navigations.length),1,'Close decisions defer menu navigation');
  await page.evaluate(async()=>{state.blocked=false;window.replaceDuringNavigation=true;await ui.refresh();});
  assert.equal(await page.evaluate(()=>state.navigation.id),3,'An old acknowledgement preserves a later menu choice');
  await page.evaluate(()=>ui.refresh());
  assert.deepEqual(await page.evaluate(()=>navigations),['messages','settings','messages']);
  await page.evaluate(async()=>{state.background={id:4,generation:1};await ui.refresh();});
  assert.equal(await page.evaluate(()=>state.visible),false);
  assert.equal(await page.evaluate(()=>visibility.at(-1)),false);
  await page.evaluate(async()=>{state.visible=true;state.background={id:5,generation:1};allow=false;await ui.refresh();});
  assert.equal(await page.evaluate(()=>state.visible),true,'Blocked modal leaves the native window visible');
  assert.equal(await page.evaluate(()=>visibility.at(-1)),true);
  assert.equal(await page.evaluate(()=>notices.length),1);
  await page.evaluate(async()=>{state.visible=true;state.background={id:6,generation:1};allow=true;window.failNative=true;await ui.refresh();});
  assert.equal(await page.evaluate(()=>visibility.at(-1)),true,'Native failure restores actual plugin visibility after preflight');
  assert.equal(await page.evaluate(()=>notices.at(-1)), 'Error: macOS presentation rejected');
  await page.evaluate(()=>ui.dispose());
  assert.deepEqual(errors,[]);
  console.log('Desktop shell UI: startup navigation, close priority, late acknowledgement, background visibility and modal refusal passed.');
}finally{await browser.close();await server.close();}
