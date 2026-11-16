import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {chromium} from 'playwright';

const server=await createServer({server:{host:'127.0.0.1',port:0}});await server.listen();
const browser=await chromium.launch({headless:true}),page=await browser.newPage();
const url=server.resolvedUrls.local[0];
const errors=[];page.on('pageerror',e=>errors.push(String(e)));
await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<html><body><div id="app"></div><script type="module">
import {Shell} from '/shell.ts';import {ConnectionsUi} from '/connections.ts';import '/style.css';
let views=[],gate;window.focuses=[];window.hidden=[];
const context={workspaceId:'w',serverId:'s',connectionId:'a',generation:1};
let state={window:{id:'main',role:'main',activeWorkspace:null},workspaces:[{context,name:'test',endpoint:'ops@test',connected:true}]};
window.__TAURI_INTERNALS__={invoke:async(command,args)=>{if(args.command?.op==='focus'){focuses.push(args.command.context);state.window.activeWorkspace='w';return views;}if(args.command?.op==='snapshot')return structuredClone(views);if(args.command?.op==='start')return await new Promise(resolve=>gate=resolve);throw new Error(command);}};
const workspaces={setHostCallbacks(changed,selected){this.changed=changed;this.selected=selected;changed(state);},setHostVisible(v){hidden.push(!v);},async refresh(){this.changed(state);}};
window.shell=new Shell(document.querySelector('#app'),true);window.connection=new ConnectionsUi('main',()=>{},e=>errors.push(e));shell.attach(workspaces,connection);
window.gated=()=>!!gate;window.finish=()=>{views=[{attemptId:'a',serverId:'s',windowId:'main',state:'connected',context}];const finish=gate;gate=undefined;finish?.(structuredClone(views));};
window.clear=()=>{views=[];focuses.length=0;};window.passive=async()=>{views=[{attemptId:'a',serverId:'s',windowId:'main',state:'connecting'}];await connection.refresh();views=[{attemptId:'a',serverId:'s',windowId:'main',state:'connected',context}];await connection.refresh();};
window.ready=true;
</script></body></html>`}));
try {
 await page.goto(url);await page.waitForFunction(()=>window.ready);
 assert.equal(await page.locator('#management-console').isVisible(),true);
 assert.equal(await page.locator('#return-workbench').isVisible(),false,'No return action without an active workspace');
 assert.deepEqual(await page.locator('[data-console-page]').evaluateAll(nodes=>nodes.map(node=>node.dataset.consolePage)),['connections','plugins','messages','logs','status','settings','updates'],'Visual and keyboard order follow the navigation groups');
 for(const width of [1100,720,600]){
  await page.setViewportSize({width,height:720});
  for(const destination of ['connections','plugins','settings','messages','logs','status','updates']){
   await page.locator(`[data-console-page=${destination}]`).focus();await page.keyboard.press('Enter');
   assert.equal(await page.locator('.console-nav [aria-current="page"]').getAttribute('data-console-page'),destination);
   const gap=await page.evaluate(()=>{
    const content=document.querySelector(shell.route.page==='connections'?'#server-catalog':'#console-page');
    return content.getBoundingClientRect().top-document.querySelector('.console-main').getBoundingClientRect().top;
   });
   assert.equal(gap,0,`${destination} at ${width}px starts directly at the top of the content area`);
  }
 }
 await page.setViewportSize({width:1100,height:720});await page.locator('[data-console-page=connections]').click();
 await page.evaluate(()=>{window.connecting=connection.connect('s');});await page.waitForFunction(()=>gated());
 await page.evaluate(()=>finish());await page.evaluate(()=>connecting);
 await page.waitForFunction(()=>shell.route.kind==='workbench');
 assert.equal(await page.evaluate(()=>focuses.length),1,'Immediate success without an observed connecting snapshot activates once');
 await page.evaluate(()=>connection.refresh());assert.equal(await page.evaluate(()=>focuses.length),1,'Repeated snapshots do not activate twice');
 for(const width of [1100,720,600]){
  await page.setViewportSize({width,height:720});
  await page.locator('#catalog-tab').click();await page.locator('[data-console-page=logs]').click();
  assert.equal(await page.locator('.console-nav #return-workbench').isVisible(),true);
  await page.locator('#return-workbench').focus();await page.keyboard.press('Enter');
  assert.equal(await page.evaluate(()=>shell.route.kind),'workbench','Return action restores the active workspace with the keyboard');
  assert.equal(await page.evaluate(()=>hidden.at(-1)),false,'Returning makes existing plugin surfaces visible');
  await page.locator('#catalog-tab').click();
  assert.deepEqual(await page.evaluate(()=>shell.route),{kind:'console',page:'logs'},'SoDeck restores the last management page');
  await page.locator('#new-workspace').click();
  assert.deepEqual(await page.evaluate(()=>shell.route),{kind:'console',page:'connections'},'Main + explicitly opens connections');
 }
 assert.equal(await page.locator('#new-workspace').getAttribute('aria-label'),'连接服务器');
 assert.equal(await page.locator('#new-workspace').getAttribute('aria-haspopup'),null,'Main + navigates to a page');
 await page.setViewportSize({width:1100,height:720});
 await page.evaluate(()=>{shell.navigate('connections');clear();window.connecting=connection.connect('s');});await page.waitForFunction(()=>gated());
 await page.locator('[data-console-page=logs]').click();await page.evaluate(()=>finish());await page.evaluate(()=>connecting);
 assert.deepEqual(await page.evaluate(()=>shell.route),{kind:'console',page:'logs'});
 assert.equal(await page.evaluate(()=>focuses.length),0,'Late success cannot steal navigation');
 await page.evaluate(()=>{shell.navigate('connections');clear();window.connecting=connection.connect('s');});await page.waitForFunction(()=>gated());
 await page.evaluate(()=>shell.selectionChanged());await page.evaluate(()=>finish());await page.evaluate(()=>connecting);
 assert.equal(await page.evaluate(()=>focuses.length),0,'Choosing another target cancels the previous presentation intent');
 assert.equal(await page.evaluate(()=>shell.route.kind),'console');
 await page.evaluate(()=>{clear();return passive();});assert.equal(await page.evaluate(()=>focuses.length),0,'Passive snapshots cannot activate a workspace');
 await page.evaluate(()=>connection.connect('s'));await page.waitForFunction(()=>shell.route.kind==='workbench');
 assert.equal(await page.evaluate(()=>focuses.length),1,'Explicit selection reuses an existing online session');
 assert.deepEqual(errors,[]);
 console.log('Shell navigation: seven pages without a heading, remembered management page, keyboard return at three widths, immediate/late success and online reuse passed');
} finally {await browser.close();await server.close();}
