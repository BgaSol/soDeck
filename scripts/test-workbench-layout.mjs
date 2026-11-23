import {fixtureBinary} from './cargo-fixture.mjs';
import {completedAction} from './component-command-fixture.mjs';
// Browser input and iframes use the real core and a real UI v2 plugin process.
// Separate browser pages stand in for native windows; this is not an OS drag test.
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import {createServer} from 'vite';
import {chromium} from 'playwright';
import {recordTrace} from './ui-trace.mjs';

assert.equal(spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--bin','catalog-fixture','--bin','workspace-ui-fixture','--locked'],{stdio:'inherit'}).status,0);
const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-layout-ui-')),plugin=path.join(root,'plugin');fs.mkdirSync(plugin);
const suffix=process.platform==='win32'?'.exe':'';
fs.copyFileSync(fixtureBinary('workspace-ui-fixture'),path.join(plugin,`fixture${suffix}`));
if(process.platform!=='win32')fs.chmodSync(path.join(plugin,`fixture${suffix}`),0o755);
fs.cpSync('scripts/fixtures/workspace-ui/ui',path.join(plugin,'ui'),{recursive:true});
// Candidate UI restores state without publishing selection or scroll mutations.
const view=path.join(plugin,'ui/view.js');fs.writeFileSync(view,fs.readFileSync(view,'utf8').replace("function position() {","function position() { if(current?.readOnly)return;").replace("addEventListener('sodeck:themeChanged'","addEventListener('sodeck:surfaceReady',()=>{current.readOnly=false;});\n  addEventListener('sodeck:themeChanged'"));
const manifest=JSON.parse(fs.readFileSync('scripts/fixtures/workspace-ui/manifest.json','utf8'));
for(const id of ['spare-one','spare-two'])manifest.ui.views.push({id,title:id,icon:'folder',container:'sidebar',instancePolicy:'perWorkspace',defaultPinned:true});
const target=spawnSync('rustc',['-vV'],{encoding:'utf8'}).stdout.match(/^host: (.+)$/m)[1];
fs.writeFileSync(path.join(plugin,'plugin.json'),JSON.stringify({...manifest,target,entry:`fixture${suffix}`}));
const child=spawn(fixtureBinary('catalog-fixture'),[root],{windowsHide:true,stdio:['pipe','pipe','inherit']});
const queue=[];readline.createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line),p=queue.shift();r.error?p.reject(new Error(r.error)):p.resolve(r.value);});
child.on('exit',()=>{for(const p of queue.splice(0))p.reject(new Error('Core fixture exited'));});
const call=(command,args={})=>new Promise((resolve,reject)=>{queue.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const server=await createServer({server:{host:'127.0.0.1',port:0}});await server.listen();const url=server.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true}),pages=new Map(),errors=[];
const traces=[];
let delayMount=false,holdMount=true;
const refresh=async()=>{for(const page of pages.values())await page.evaluate(()=>manager.refresh());};
const wait=async(fn)=>{const end=Date.now()+10000;while(!await fn()){assert(Date.now()<end,'Layout operation did not settle');await new Promise(r=>setTimeout(r,25));}};
async function makePage(window) {
  const page=await browser.newPage({viewport:{width:1280,height:800}});pages.set(window,page);traces.push(await recordTrace(page.context(),'layout-'+window));page.on('pageerror',e=>errors.push(String(e)));
  await page.exposeFunction('bridge',async(command,args={})=>{
    if(command==='workbench_return_window') {
      const movements=await call('workbench_fixture_return_window',{...args,window});
      await refresh();
      for(const movement of movements)await call('workbench_fixture_drain',{movement});
      const end=Date.now()+10000;
      while(true){try{await call('workbench_fixture_commit_many',{movements});break;}catch(error){if(Date.now()>end)throw error;await refresh();await new Promise(r=>setTimeout(r,30));}}
      await refresh();return null;
    }
    if(command==='workbench_move') {
      let target=args.target??'layout-secondary';
      const movement=await call('workbench_fixture_prepare',{...args,window,target});
      target=movement.target;
      if(!pages.has(target))await makePage(target);await refresh();
      await call('workbench_fixture_drain',{movement});await refresh();
      const end=Date.now()+10000;
      while(true){try{await call('workbench_fixture_commit',{movement});break;}catch(error){if(Date.now()>end)throw error;await refresh();await new Promise(r=>setTimeout(r,30));}}
      await refresh();return null;
    }
    if(['clipboard_text','close_register_host'].includes(command))return null;
    return call(command,{...args,window});
  });
  await page.route(url+'__plugin__/**',route=>{const p=path.join(plugin,decodeURIComponent(new URL(route.request().url()).pathname).split('/').slice(3).join('/'));let body=fs.readFileSync(p);if((delayMount||holdMount)&&path.basename(p)==='view.js'){assert(body.toString().includes('sodeck.prepared(true);'));body=Buffer.from(body.toString().replace('sodeck.prepared(true);',holdMount?'window.finishMount=()=>sodeck.prepared(true);':'setTimeout(()=>sodeck.prepared(true),800);'));}return route.fulfill({contentType:({'.html':'text/html','.js':'text/javascript','.css':'text/css'})[path.extname(p)],body});});
  await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<html lang="zh-CN"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab" role="tab">服务器管理</button><div class="tab-navigation"><div id="server-tabs" class="server-tabs" role="tablist"></div><div class="tab-controls"><span class="tab-arrows" hidden></span><button data-tab-action="pick" aria-haspopup="dialog" aria-expanded="false" hidden><span class="tab-count"></span></button></div></div><button id="new-workspace" class="sd-icon" aria-label="选择服务器"></button><div id="drag-region" class="drag-region"></div></header><main class="workspace"><div id="server-catalog"></div><div id="workspace-content"></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';import {tabControls} from '/tab-strip.ts';import {icon} from '/ui.ts';document.querySelector('.topbar .tab-controls').outerHTML=tabControls('服务器标签');document.querySelector('#new-workspace').innerHTML=icon('plus');window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:()=>1,invoke:async(command,args)=>{if(command==='plugin:event|listen')return 1;if(command==='plugin:event|unlisten')return;return bridge(command,args);}};window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),text=>document.querySelector('#notice').textContent=text,bridge);await manager.start();window.ready=true;</script></body></html>`}));
  await page.goto(url);await page.waitForFunction(()=>window.ready);return page;
}
const ui=()=>call('component_ui',{window:'main',operation:'snapshot',args:{}});
let workspace;
try {
  await call('fixture_load_plugin',{directory:plugin});
  const catalog=await call('server_catalog');const saved=await call('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'布局验收',host:'fixture.invalid',port:22,username:'ops',authentication:{type:'password',secret:{action:'clear'}},route:[]}}});
  workspace=await call('workspace_fixture_open',{serverId:saved.selectedId,connectionId:'layout-session',endpoint:'ops@fixture.invalid:22'});
  const main=await makePage('main'),wid=workspace.context.workspaceId;
  await wait(async()=>await main.locator('.component-surface iframe').count()===2);
  await main.waitForFunction(()=>[...document.querySelectorAll('.component-surface iframe')].every(f=>f.contentWindow.finishMount));
  for(const theme of ['light','dark','dopamine']) {
    await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    const loading=await main.locator('.component-surface').evaluateAll(nodes=>{const probe=document.createElement('div');probe.style.background='var(--canvas)';document.body.append(probe);const canvas=getComputedStyle(probe).backgroundColor;probe.remove();return nodes.map(node=>({frame:getComputedStyle(node.querySelector('iframe')).visibility,background:getComputedStyle(node).backgroundColor,canvas,width:node.querySelector('iframe').clientWidth,height:node.querySelector('iframe').clientHeight}));});
    assert(loading.every(surface=>surface.frame==='hidden'), 'Plugin documents stay concealed until the themed mount is ready: '+theme);
    assert(loading.every(surface=>surface.background===surface.canvas&&surface.width>0&&surface.height>0), 'Loading surfaces retain real dimensions and the current theme background: '+theme);
    const chrome=await main.locator('.component-tabs').first().evaluate(node=>{
      const css=getComputedStyle(document.documentElement),probe=document.createElement('div');probe.style.background='var(--panel)';document.body.append(probe);const panel=getComputedStyle(probe).backgroundColor;probe.remove();
      return {background:getComputedStyle(node).backgroundColor,panel,shadow:getComputedStyle(node).boxShadow,line:getComputedStyle(node,'::after').height,focus:getComputedStyle(node,'::after').backgroundColor,accent:css.getPropertyValue('--accent').trim()};
    });
    assert.equal(chrome.background,chrome.panel,'Shared component tabs use the neutral panel in '+theme);assert.equal(chrome.shadow,'none','Focus uses the existing divider instead of a second line');assert.equal(chrome.line,theme==='dopamine'?'2px':'1px');
  }
  holdMount=false;
  await main.evaluate(()=>document.querySelectorAll('.component-surface iframe').forEach(f=>f.contentWindow.finishMount()));
  await wait(async()=>await main.locator('.component-surface iframe').evaluateAll(frames=>frames.every(f=>getComputedStyle(f).visibility==='visible')));
  const initial=(await ui()).workspaces[wid],terminal=initial.editor;
  const originalGroup=Object.values(initial.host.groups).find(g=>g.viewIds.includes(terminal)).id;
  const frame=main.frameLocator(`.component-surface[data-view-id="${terminal}"] iframe`);
  await frame.locator('textarea').fill('draft survives moving');
  const before=await main.locator(`.component-surface[data-view-id="${terminal}"] iframe`).elementHandle();
  const action=async value=>{await call('component_ui',{window:'main',operation:'action',args:{context:workspace.context,action:value}});await refresh();};
  await main.locator(".component-tabs").first().focus();await main.keyboard.press("Shift+F10");await main.getByRole('menuitem',{name:'左右分屏',exact:true}).click();
  await wait(async()=>await main.locator('.view-group').count()===2);
  const emptyGroup=(await ui()).workspaces[wid].host.focusedGroupId;
  await main.locator(`[data-group="${emptyGroup}"] .component-tabs`).focus();await main.keyboard.press("Shift+F10");await main.getByRole('menuitem',{name:'取消此分屏',exact:true}).click();
  await wait(async()=>await main.locator('.view-group').count()===1&&await main.locator('.component-close-dialog').count()===0);
  assert.equal((await ui()).workspaces[wid].host.groups[emptyGroup],undefined);
  assert(await before.evaluate(el=>el.isConnected),'Closing an empty split keeps the other page mounted');
  assert.equal(await frame.locator('textarea').inputValue(),'draft survives moving');
  assert.equal(await main.locator('.tab-menu-button,[data-component-action="group-menu"]').count(),0);
  await main.locator(`.component-tab[data-view="${terminal}"] [role="tab"]`).focus();await main.keyboard.press('Shift+F10');
  assert.deepEqual(await main.getByRole('menuitem').allTextContents(),['移到右侧分屏','移到下方分屏','移到新窗口','关闭标签']);
  fs.mkdirSync('artifacts/workbench-layout',{recursive:true});await main.screenshot({path:'artifacts/workbench-layout/page-menu.png'});
  await main.getByRole('menuitem',{name:'移到右侧分屏',exact:true}).click();
  await wait(async()=>await main.locator('.view-group').count()===2);
  const soleSplit=(await ui()).workspaces[wid].host.focusedGroupId;
  assert.deepEqual((await ui()).workspaces[wid].host.groups[originalGroup].viewIds,[],'Splitting the sole page preserves its empty source');
  const header=await main.locator(`[data-group="${soleSplit}"] .component-tabs`).boundingBox();await main.mouse.click(header.x+header.width-12,header.y+header.height/2,{button:'right'});
  assert.deepEqual(await main.getByRole('menuitem').allTextContents(),['左右分屏','上下分屏','取消此分屏']);
  await main.screenshot({path:'artifacts/workbench-layout/group-menu.png'});
  await main.getByRole('menuitem',{name:'取消此分屏',exact:true}).click();
  await wait(async()=>await main.locator('.view-group').count()===1);
  assert(await before.evaluate(el=>el.isConnected),'Merging a populated split keeps its iframe');
  assert.equal(await frame.locator('textarea').inputValue(),'draft survives moving');
  assert.equal(await main.locator('.component-close-dialog').count(),0,'Cancelling a split never closes its page');
  await main.locator(".component-tabs").first().focus();await main.keyboard.press("Shift+F10");await main.getByRole('menuitem',{name:'左右分屏',exact:true}).click();
  await wait(async()=>await main.locator('.view-group').count()===2);
  const split=(await ui()).workspaces[wid].host,second=split.focusedGroupId;
  await main.locator(`[data-group="${second}"] [data-component-action="create"]`).click();
  await wait(async()=>await main.locator('.component-surface iframe').count()===3);
  await main.locator(`[data-group="${second}"] .component-tabs`).focus();await main.keyboard.press("Shift+F10");await main.getByRole('menuitem',{name:'上下分屏',exact:true}).click();
  await wait(async()=>await main.locator('.view-group').count()===3);
  assert(await before.evaluate(el=>el.isConnected),'Same-window layout keeps the original iframe');
  const handle=main.locator('[data-split-ratio]').first();await handle.focus();await main.keyboard.press('ArrowRight');
  await wait(async()=>Number(await handle.getAttribute('aria-valuenow'))===55);
  const rect=await handle.boundingBox();await main.mouse.move(rect.x+2,rect.y+60);await main.mouse.down();await main.mouse.move(rect.x+100,rect.y+60);await main.keyboard.press('Escape');await main.mouse.up();
  await wait(async()=>Number(await handle.getAttribute('aria-valuenow'))===55);
  const host=(await ui()).workspaces[wid].host;
  await action({op:'zoom',groupId:host.focusedGroupId});assert.equal(await main.locator('.view-group').count(),1);await action({op:'zoom',groupId:host.focusedGroupId});assert.equal(await main.locator('.view-group').count(),3);
  const visibleSurfaces=()=>main.evaluate(()=>[...document.querySelectorAll('.component-surface:not([hidden]) iframe')].map(frame=>{const r=frame.getBoundingClientRect();return {width:r.width,height:r.height,hit:document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)===frame};}));
  await wait(async()=>{const surfaces=await visibleSurfaces();return surfaces.length===3&&surfaces.every(s=>s.width>100&&s.height>100&&s.hit);});
  for(const theme of ['light','dark','dopamine']){await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);fs.mkdirSync('artifacts/workbench-layout',{recursive:true});await main.screenshot({path:`artifacts/workbench-layout/${theme}.png`});}
  await main.setViewportSize({width:540,height:360});await wait(async()=>await main.locator('.view-group').count()===1);
  const compact=main.getByRole('button',{name:'其他分屏（2）',exact:true});await compact.focus();await main.keyboard.press('Enter');
  assert.equal(await main.getByRole('menuitem').count(),2);await main.keyboard.press('ArrowDown');await main.keyboard.press('Enter');
  await wait(async()=>(await ui()).workspaces[wid].host.focusedGroupId!==host.focusedGroupId);
  await main.screenshot({path:'artifacts/workbench-layout/compact.png'});
  await main.setViewportSize({width:1280,height:800});await wait(async()=>await main.locator('.view-group').count()===3);
  delayMount=true;
  await main.locator(`[data-group="${originalGroup}"] [data-component-action="create"]`).click();
  await wait(async()=>((await ui()).workspaces[wid].host.groups[originalGroup]?.viewIds.length??0)===2);
  const created=(await ui()).workspaces[wid].host.groups[originalGroup].activeViewId;
  await wait(()=>main.evaluate(id=>[...manager.components.shells.values()].some(shell=>{const surface=shell.surfaces.get(id);return surface?.mounted&&surface.focusPending===undefined&&surface.frame.dataset.stateReady==='true'&&shell.layoutFrame===undefined;}),created));
  delayMount=false;
  await action({op:'focusGroup',groupId:host.focusedGroupId});
  await wait(()=>main.evaluate(()=>[...manager.components.shells.values()].every(shell=>shell.layoutFrame===undefined)));
  await main.locator(`.component-tab[data-view="${terminal}"] [role="tab"]`).click({button:'right'});
  assert.equal((await ui()).workspaces[wid].host.groups[originalGroup].activeViewId===terminal,false,'Right-clicking an inactive page keeps the active page');
  await main.getByRole('menuitem',{name:'移到新窗口',exact:true}).click();
  await wait(async()=>pages.has('layout-secondary')&&await pages.get('layout-secondary').locator(`.component-surface[data-view-id="${terminal}"] iframe`).count()===1&&await main.locator(`.component-surface[data-view-id="${terminal}"]`).count()===0);
  const secondary=pages.get('layout-secondary');await refresh();
  assert.equal(await secondary.frameLocator(`.component-surface[data-view-id="${terminal}"] iframe`).locator('textarea').inputValue(),'draft survives moving');
  assert.equal(await main.locator(`.component-surface[data-view-id="${terminal}"]`).count(),0);
  await secondary.locator(`.component-tab[data-view="${terminal}"]`).click({button:'right'});await secondary.getByRole('menuitem',{name:'返回原窗口',exact:true}).click();
  await wait(async()=>await main.locator(`.component-surface[data-view-id="${terminal}"] iframe`).count()===1&&await secondary.locator(`.component-surface[data-view-id="${terminal}"]`).count()===0);await refresh();
  assert.equal(await main.frameLocator(`.component-surface[data-view-id="${terminal}"] iframe`).locator('textarea').inputValue(),'draft survives moving');
  assert((await ui()).workspaces[wid].host.groups[originalGroup].viewIds.includes(terminal),'Return uses the original group despite a different focused group');
  const catalog2=await call('server_catalog');
  const saved2=await call('change_catalog',{revision:catalog2.revision,change:{op:'saveServer',draft:{name:'第二份工作台',host:'second.invalid',port:22,username:'ops',authentication:{type:'password',secret:{action:'clear'}},route:[]}}});
  const secondWorkspace=await call('workspace_fixture_open',{serverId:saved2.selectedId,connectionId:'layout-session-2',endpoint:'ops@second.invalid:22'});
  await refresh();
  await wait(async()=>await main.locator(`.server-workbench[data-id="${secondWorkspace.context.workspaceId}"] .component-tab`).count()===1);
  await main.evaluate(wid=>manager.components.moveHost(wid,'layout-secondary'),wid);
  await main.evaluate(wid=>manager.components.moveHost(wid,'layout-secondary'),secondWorkspace.context.workspaceId);
  await refresh();
  assert.equal((await call('workspace_snapshot',{window:'layout-secondary'})).workspaces.length,2);
  await secondary.evaluate(()=>manager.components.returnWindow());
  await wait(async()=>{const state=await call('workspace_snapshot',{window:'main'});return state.workspaces.length===2&&state.incomingWorkspaces.length===0;});
  assert.equal((await call('workspace_snapshot',{window:'layout-secondary'})).workspaces.length,0);
  await call('workspace_activate',{window:'main',id:wid});await refresh();
  const tool=initial.left,toolKey=initial.views.find(v=>v.id===tool).definitionKey;
  const toolFrame=await main.locator(`.component-surface[data-view-id="${tool}"] iframe`).elementHandle();
  assert.equal(await main.locator('[data-component-action="panel-menu"]').count(),0,'Tool panels expose a title and collapse action');
  await action({op:'moveView',viewId:tool,groupId:(await ui()).workspaces[wid].host.focusedGroupId});
  await wait(async()=>Object.values((await ui()).workspaces[wid].host.groups).some(g=>g.viewIds.includes(tool)));
  assert(await toolFrame.evaluate(el=>el.isConnected),'Tool docking keeps the same iframe and business instance');
  await main.locator(`.component-tab[data-view="${tool}"]`).click({button:'right'});
  await main.getByRole("menuitem",{name:"停靠到侧栏",exact:true}).click();await main.locator(`.server-workbench[data-id="${wid}"] [data-tool-key="${toolKey}"]`).click({button:'right'});await main.getByRole("menuitem",{name:"移到右侧",exact:true}).click();
  await wait(async()=>(await ui()).workspaces[wid].right===tool&&await main.locator(`.server-workbench[data-id="${wid}"] .left-rail [data-key="${toolKey}"]`).count()===0);
  assert.equal(await main.locator(`.server-workbench[data-id="${wid}"] .left-rail [data-key="${toolKey}"]`).count(),0);
  await main.locator(`.server-workbench[data-id="${wid}"] .right-rail [data-key="${toolKey}"]`).click();
  await wait(async()=>(await ui()).workspaces[wid].right===null);
  assert((await ui()).workspaces[wid].host.right.viewIds.includes(tool),'Toggling a relocated tool preserves its actual dock');
  const board=main.locator(`.server-workbench[data-id="${wid}"]`),spareKey=`${manifest.id}:spare-one`,secondKey=`${manifest.id}:spare-two`;
  const slots=()=>ui().then(s=>s.workspaces[wid]);
  const toolsBefore=await slots(),countBefore=toolsBefore.views.length;
  const dragButton=async(key,side,y,cancel)=>{
    await refresh();
    const from=await board.locator(`[data-tool-key="${key}"]`).boundingBox(),to=await board.locator(`.${side}-rail`).boundingBox();
    await main.mouse.move(from.x+from.width/2,from.y+from.height/2);await main.mouse.down();
    await main.mouse.move(to.x+to.width/2,to.y+y,{steps:12});
    if(cancel==='escape')await main.keyboard.press('Escape');
    if(cancel==='blur')await main.evaluate(()=>dispatchEvent(new Event('blur')));
    await main.mouse.up();
  };
  await dragButton(spareKey,'right',140);
  await wait(async()=>(await slots()).tools.find(t=>t.key===spareKey)?.location.side==='right');
  await board.locator(`.right-rail [data-tool-key="${spareKey}"]`).waitFor({state:'visible'});
  assert.equal((await slots()).views.length,countBefore,'Dragging an uncreated tool does not create a business view');
  assert.equal((await slots()).right,null,'Dragging a collapsed slot keeps the destination collapsed');
  const beforeFirst=await board.locator(`[data-tool-key="${spareKey}"]`).boundingBox(),rail=await board.locator('.right-rail').boundingBox();
  await dragButton(secondKey,'right',beforeFirst.y-rail.y+2);
  await wait(async()=>{const keys=(await slots()).host.right.entries.map(t=>t.definitionKey);return keys.indexOf(secondKey)<keys.indexOf(spareKey);});
  const reordered=(await slots()).host;
  for(const cancel of ['escape','blur']){await dragButton(spareKey,'left',140,cancel);assert.deepEqual((await slots()).host,reordered,'Cancelled gestures never commit');}
  await dragButton(spareKey,'left',140,'escape');
  const button=await board.locator(`[data-tool-key="${spareKey}"]`).boundingBox(),leftRail=await board.locator('.left-rail').boundingBox();
  await main.mouse.move(button.x+button.width/2,button.y+button.height/2);await main.mouse.down();await main.mouse.move(leftRail.x+leftRail.width/2,leftRail.y+150,{steps:10});
  const currentHost=(await slots()).host;
  await action({op:'focusGroup',groupId:Object.keys(currentHost.groups).find(id=>id!==currentHost.focusedGroupId)});
  const changed=(await slots()).host;await main.mouse.up();assert.deepEqual((await slots()).host,changed,'A stale drag cannot overwrite a newer core layout');
  await dragButton(toolKey,'left',180);
  await wait(async()=>(await slots()).tools.find(t=>t.key===toolKey)?.location.side==='left');
  assert.equal((await slots()).left,null,'Moving a hidden existing tool does not reveal it');
  await board.locator(`.left-rail [data-tool-key="${toolKey}"]`).click();
  await wait(async()=>(await slots()).left===tool&&await board.locator(`.left-rail [data-tool-key="${toolKey}"]`).getAttribute('aria-expanded')==='true');
  await main.evaluate(wid=>{
    const samples=window.motionSamples={errors:[],widths:[],done:false},until=performance.now()+600;
    const board=document.querySelector(`.server-workbench[data-id="${CSS.escape(wid)}"]`);
    const sample=()=>requestAnimationFrame(()=>setTimeout(()=>{
      for(const surface of document.querySelectorAll('.component-surface:not([hidden])')){
        const id=surface.dataset.viewId,tab=document.querySelector(`[data-view="${CSS.escape(id)}"] [role="tab"][aria-selected="true"]`),frame=surface.querySelector('iframe');
        if(!frame)continue;const rect=frame.getBoundingClientRect();if(rect.width<=0||rect.height<=0)continue;
        const slots=tab?[tab.closest('.view-group').querySelector('.central-slot')]:[...document.querySelectorAll('.workbench-panel .component-slot')];
        const distance=Math.min(...slots.map(slot=>{const r=slot.getBoundingClientRect();return Math.max(Math.abs(r.left-rect.left),Math.abs(r.top-rect.top),Math.abs(r.width-rect.width),Math.abs(r.height-rect.height));}));
        if(distance>1)samples.errors.push({id,distance});
      }
      samples.widths.push(Math.round(board.querySelector('.left-panel').getBoundingClientRect().width));
      if(performance.now()<until)sample();else samples.done=true;
    },0));sample();
  },wid);
  await board.locator(`.left-rail [data-tool-key="${toolKey}"]`).click();
  await wait(async()=>(await slots()).left===null);
  await board.locator(`.left-rail [data-tool-key="${toolKey}"]`).click();
  await wait(async()=>(await slots()).left===tool);
  await main.waitForFunction(()=>motionSamples.done);
  const samples=await main.evaluate(()=>motionSamples);
  assert.deepEqual(samples.errors,[],'Iframe rectangles follow their live slots on every rendered frame, including reversal');
  assert(new Set(samples.widths).size>3,'Panels pass through intermediate widths instead of jumping: '+JSON.stringify(samples.widths));
  assert(await toolFrame.evaluate(el=>el.isConnected),'Motion never replaces the tool iframe');
  await dragButton(toolKey,'right',180);
  await wait(async()=>(await slots()).right===tool&&(await slots()).left===null&&await board.locator(`.right-rail [data-tool-key="${toolKey}"]`).getAttribute('aria-expanded')==='true');
  await main.waitForFunction(id=>{const surface=document.querySelector(`.component-surface[data-view-id="${CSS.escape(id)}"]`);return surface&&!surface.hidden&&!surface.inert&&surface.getBoundingClientRect().width>0;},tool);
  assert(await toolFrame.evaluate(el=>el.isConnected),'Button movement retains the original iframe');
  await board.locator(`[data-tool-key="${spareKey}"]`).focus();await main.keyboard.press('Shift+F10');
  await main.getByRole('menuitem',{name:'移到左侧',exact:true}).click();
  await wait(async()=>(await slots()).tools.find(t=>t.key===spareKey)?.location.side==='left');
  assert.equal((await slots()).views.length,countBefore,'Keyboard movement also keeps empty slots uncreated');
  for(const theme of ['light','dark','dopamine']){await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);await main.screenshot({path:`artifacts/workbench-layout/tools-${theme}.png`});}

  await main.setViewportSize({width:1100,height:720});
  const crowdedGroup=(await slots()).host.focusedGroupId;
  for(let i=0;i<50;i++)await completedAction(call,{window:'main',context:workspace.context,requestId:'overflow-'+i},await call('component_ui',{window:'main',operation:'command',args:{op:'submit',name:'fixture.create.v1',context:workspace.context,requestId:'overflow-'+i,actionKey:manifest.id+':new-page',groupId:crowdedGroup}}));
  await refresh();await wait(async()=>(await slots()).host.groups[crowdedGroup].viewIds.length>=50);
  const crowdedHeader=board.locator('[data-group="'+crowdedGroup+'"] .component-tabs');
  await crowdedHeader.locator('[data-tab-action="pick"]').click();
  const count=(await slots()).host.groups[crowdedGroup].viewIds.length;
  assert.equal(await main.locator('.tab-picker-row').count(),count,'Picker is scoped to this group');
  await main.locator('.tab-picker input').fill('终端');assert((await main.locator('.tab-picker-row').count())>=50);
  await main.locator('[data-tab-picker-action="select"]').first().click();await main.waitForTimeout(300);
  const originalActive=(await slots()).host.groups[crowdedGroup].activeViewId;
  await crowdedHeader.locator('[data-tab-action="forward"]').click();await main.waitForTimeout(300);
  const browsed=await crowdedHeader.locator('.component-tab-list').evaluate(n=>n.scrollLeft);await refresh();
  assert.equal(await crowdedHeader.locator('.component-tab-list').evaluate(n=>n.scrollLeft),browsed);
  assert.equal((await slots()).host.groups[crowdedGroup].activeViewId,originalActive);
  assert(await toolFrame.evaluate(el=>el.isConnected),'Many tabs retain the current iframe');
  for(const width of [1100,720])for(const theme of ['light','dark','dopamine']){
    await main.setViewportSize({width,height:720});await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    fs.mkdirSync('artifacts/tab-overflow',{recursive:true});await main.screenshot({path:'artifacts/tab-overflow/pages-'+theme+'-'+width+'.png'});
    assert.equal(await main.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
  }
  assert.deepEqual(errors,[]);
  console.log('Passed: empty split close, nested splits, independent creation, stable iframe, keyboard/escape resize, zoom, compact recovery, UI v2 page transfer/source return, two-workspace atomic window return and draft continuity; three theme screenshots saved.');
} catch(error) {
  for(const finish of traces)await finish(true);
  console.error('Layout failure',JSON.stringify((await ui()).transfers),errors);
  for(const [name,page] of pages)console.error(name,JSON.stringify(await page.evaluate(()=>({notice:document.querySelector('#notice')?.textContent,shells:[...manager.components.shells.values()].map(s=>({id:s.workspace.context.workspaceId,surfaces:[...s.surfaces.values()].map(v=>({id:v.view.id,mounted:v.mounted,readOnly:v.readOnly,flushing:Boolean(v.flushing),error:v.node.querySelector('.component-error')?.textContent}))}))}))));
  throw error;
} finally {
  for(const finish of traces)await finish(false);
  await browser.close();await server.close();child.stdin.end();await new Promise(r=>child.exitCode===null?child.once('exit',r):r());fs.rmSync(root,{recursive:true,force:true});
}
