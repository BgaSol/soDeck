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

assert.equal(spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--bin','catalog-fixture','--bin','workspace-ui-fixture','--locked'],{stdio:'inherit'}).status,0);
const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-layout-ui-')),plugin=path.join(root,'plugin');fs.mkdirSync(plugin);
const suffix=process.platform==='win32'?'.exe':'';
fs.copyFileSync(`src-tauri/target/debug/workspace-ui-fixture${suffix}`,path.join(plugin,`fixture${suffix}`));
if(process.platform!=='win32')fs.chmodSync(path.join(plugin,`fixture${suffix}`),0o755);
fs.cpSync('scripts/fixtures/workspace-ui/ui',path.join(plugin,'ui'),{recursive:true});
for(const file of ['bridge.js','view.js']) {const p=path.join(plugin,'ui',file);fs.writeFileSync(p,fs.readFileSync(p,'utf8').replaceAll('sodeck-ui-v1','sodeck-ui-v2'));}
// Candidate UI restores state without publishing selection or scroll mutations.
const view=path.join(plugin,'ui/view.js');fs.writeFileSync(view,fs.readFileSync(view,'utf8').replace("function position() {","function position() { if(current?.readOnly)return;").replace("addEventListener('sodeck:themeChanged'","addEventListener('sodeck:surfaceReady',()=>{current.readOnly=false;});\n  addEventListener('sodeck:themeChanged'"));
const manifest=JSON.parse(fs.readFileSync('scripts/fixtures/workspace-ui/manifest.json','utf8'));manifest.ui.apiVersion='2';
const target=spawnSync('rustc',['-vV'],{encoding:'utf8'}).stdout.match(/^host: (.+)$/m)[1];
fs.writeFileSync(path.join(plugin,'plugin.json'),JSON.stringify({...manifest,target,entry:`fixture${suffix}`}));
const child=spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${suffix}`),[root],{windowsHide:true,stdio:['pipe','pipe','inherit']});
const queue=[];readline.createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line),p=queue.shift();r.error?p.reject(new Error(r.error)):p.resolve(r.value);});
child.on('exit',()=>{for(const p of queue.splice(0))p.reject(new Error('Core fixture exited'));});
const call=(command,args={})=>new Promise((resolve,reject)=>{queue.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const server=await createServer({server:{host:'127.0.0.1',port:0}});await server.listen();const url=server.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true}),pages=new Map(),errors=[];
const refresh=async()=>{for(const page of pages.values())await page.evaluate(()=>manager.refresh());};
const wait=async(fn)=>{const end=Date.now()+10000;while(!await fn()){assert(Date.now()<end,'Layout operation did not settle');await new Promise(r=>setTimeout(r,25));}};
async function makePage(window) {
  const page=await browser.newPage({viewport:{width:1280,height:800}});pages.set(window,page);page.on('pageerror',e=>errors.push(String(e)));
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
  await page.route(url+'__plugin__/**',route=>{const p=path.join(plugin,decodeURIComponent(new URL(route.request().url()).pathname).split('/').slice(3).join('/'));return route.fulfill({contentType:({'.html':'text/html','.js':'text/javascript','.css':'text/css'})[path.extname(p)],body:fs.readFileSync(p)});});
  await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<html lang="zh-CN"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab" role="tab">服务器管理</button><div id="server-tabs" class="server-tabs" role="tablist"></div><div class="drag-region"></div></header><main class="workspace"><div id="server-catalog"></div><div id="workspace-content" hidden></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:()=>1,invoke:async(command,args)=>{if(command==='plugin:event|listen')return 1;if(command==='plugin:event|unlisten')return;return bridge(command,args);}};window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),document.querySelector('#server-catalog'),text=>document.querySelector('#notice').textContent=text,bridge);await manager.start();window.ready=true;</script></body></html>`}));
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
  const initial=(await ui()).workspaces[wid],terminal=initial.editor;
  const originalGroup=Object.values(initial.host.groups).find(g=>g.viewIds.includes(terminal)).id;
  const frame=main.frameLocator(`.component-surface[data-view-id="${terminal}"] iframe`);
  await frame.locator('textarea').fill('draft survives moving');
  const before=await main.locator(`.component-surface[data-view-id="${terminal}"] iframe`).elementHandle();
  const action=async value=>{await call('component_ui',{window:'main',operation:'action',args:{context:workspace.context,action:value}});await refresh();};
  await main.locator('[data-component-action="group-menu"]').click();await main.getByRole('menuitem',{name:'左右拆分',exact:true}).click();
  await wait(async()=>await main.locator('.view-group').count()===2);
  const split=(await ui()).workspaces[wid].host,second=split.focusedGroupId;
  await main.locator(`[data-group="${second}"] [data-component-action="create"]`).click();
  await wait(async()=>await main.locator('.component-surface iframe').count()===3);
  await main.locator(`[data-group="${second}"] [data-component-action="group-menu"]`).click();await main.getByRole('menuitem',{name:'上下拆分',exact:true}).click();
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
  await main.setViewportSize({width:540,height:360});await wait(async()=>await main.locator('.view-group').count()===1);await main.setViewportSize({width:1280,height:800});await wait(async()=>await main.locator('.view-group').count()===3);
  await main.locator(`[data-group="${originalGroup}"] [data-component-action="create"]`).click();
  await wait(async()=>((await ui()).workspaces[wid].host.groups[originalGroup]?.viewIds.length??0)===2);
  await action({op:'focusGroup',groupId:host.focusedGroupId});
  await main.locator(`.component-tab[data-view="${terminal}"]`).click({button:'right'});
  await main.getByRole('menuitem',{name:'页面移至新窗口',exact:true}).click();
  await wait(async()=>pages.has('layout-secondary')&&await pages.get('layout-secondary').locator(`.component-surface[data-view-id="${terminal}"] iframe`).count()===1&&await main.locator(`.component-surface[data-view-id="${terminal}"]`).count()===0);
  const secondary=pages.get('layout-secondary');await refresh();
  assert.equal(await secondary.frameLocator(`.component-surface[data-view-id="${terminal}"] iframe`).locator('textarea').inputValue(),'draft survives moving');
  assert.equal(await main.locator(`.component-surface[data-view-id="${terminal}"]`).count(),0);
  await secondary.locator(`.component-tab[data-view="${terminal}"]`).click({button:'right'});await secondary.getByRole('menuitem',{name:'页面返回来源',exact:true}).click();
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
  await secondary.locator('.tab-menu-button').first().click();
  await secondary.getByRole('menuitem',{name:'此窗口全部返回主窗口',exact:true}).click();
  await wait(async()=>{const state=await call('workspace_snapshot',{window:'main'});return state.workspaces.length===2&&state.incomingWorkspaces.length===0;});
  assert.equal((await call('workspace_snapshot',{window:'layout-secondary'})).workspaces.length,0);
  await call('workspace_activate',{window:'main',id:wid});await refresh();
  const tool=initial.left,toolKey=initial.views.find(v=>v.id===tool).definitionKey;
  const toolFrame=await main.locator(`.component-surface[data-view-id="${tool}"] iframe`).elementHandle();
  await main.locator(`.server-workbench[data-id="${wid}"] [data-component-action="panel-menu"][data-key="${toolKey}"]`).click();
  await main.getByRole('menuitem',{name:'放入主区域',exact:true}).click();
  await wait(async()=>Object.values((await ui()).workspaces[wid].host.groups).some(g=>g.viewIds.includes(tool)));
  assert(await toolFrame.evaluate(el=>el.isConnected),'Tool docking keeps the same iframe and business instance');
  await main.locator(`.component-tab[data-view="${tool}"]`).click({button:'right'});
  await main.getByRole('menuitem',{name:'停靠到右侧',exact:true}).click();
  await wait(async()=>(await ui()).workspaces[wid].right===tool);
  await main.locator(`.server-workbench[data-id="${wid}"] .left-rail [data-key="${toolKey}"]`).click();
  await wait(async()=>(await ui()).workspaces[wid].right===null);
  assert((await ui()).workspaces[wid].host.right.viewIds.includes(tool),'Toggling a relocated tool preserves its actual dock');
  assert.deepEqual(errors,[]);
  console.log('Passed: nested splits, independent creation, stable iframe, keyboard/escape resize, zoom, compact recovery, UI v2 page transfer/source return, two-workspace atomic window return and draft continuity; three theme screenshots saved.');
} catch(error) {
  console.error('Layout failure',JSON.stringify((await ui()).transfers),errors);
  for(const [name,page] of pages)console.error(name,JSON.stringify(await page.evaluate(()=>({notice:document.querySelector('#notice')?.textContent,shells:[...manager.components.shells.values()].map(s=>({id:s.workspace.context.workspaceId,surfaces:[...s.surfaces.values()].map(v=>({id:v.view.id,mounted:v.mounted,readOnly:v.readOnly,flushing:Boolean(v.flushing),error:v.node.querySelector('.component-error')?.textContent}))}))}))));
  throw error;
} finally {
  await browser.close();await server.close();child.stdin.end();await new Promise(r=>child.exitCode===null?child.once('exit',r):r());fs.rmSync(root,{recursive:true,force:true});
}
