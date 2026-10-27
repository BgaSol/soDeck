import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {launch,rpc,pluginRpc,pluginPages,until,stop,nativeWindow,delay} from './harness.mjs';
const mode=process.argv.includes('--baseline')?'baseline':'verified';
const output=resolve(`.local/test-results/t015/terminal-${mode}-${Date.now()}`);await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u','tests/core/ssh_fixture.py',join(output,'fixture')],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,PYTHONPATH:resolve('.local/python')}});
let stdout='',stderr='';fixture.stdout.on('data',b=>stdout+=b);fixture.stderr.on('data',b=>stderr+=b);
const packagePath=resolve('resources/sodeck.terminal-0.8.0.sodeck-plugin');process.env.SODECK_PLUGIN_PACKAGES=packagePath;process.env.SODECK_DEV_MODE='1';
let app,page;const measurements=[];
const dpi=async(action,percent)=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File','tests/desktop/dpi-windows.ps1','-Action',action,...(percent?['-Percent',String(percent)]:[])],{windowsHide:true})).stdout);
const originalDpi=await dpi('get');
try{
  await until(()=>stdout.includes('\n'));const config=JSON.parse(stdout.split('\n')[0]);
  app=await launch({data:join(output,'data'),packagePath,shellPort:9861,pluginPort:9862});
  const server=await rpc(app.page,'servers.create',{name:'T015 geometry',host:'127.0.0.1',port:config.port,username:'test-user',authType:'password'});
  await rpc(app.page,'auth.set',{serverId:server.id,password:'isolated-test-password',remember:false});
  await rpc(app.page,'workspaces.open',{serverId:server.id});
  await app.page.getByRole('button',{name:'信任并连接',exact:true}).click();
  page=await until(async()=>(await pluginPages(app))[0]);
  // This protocol fixture has an echo PTY, not a login shell/exec implementation.
  await pluginRpc(page,'plugin.invoke',{command:'settings.shellIntegration',args:{enabled:false}});
  await until(async()=>(await pluginRpc(page,'view.initialize')).session.status!=='connecting');
  const viewId=await page.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
  await rpc(app.page,'commands.execute',{commandId:'terminal.reconnect',viewId,operationId:crypto.randomUUID()});
  await until(async()=>(await page.locator('.xterm-rows').innerText()).includes('fixture-ready'));
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.insertText(Array.from({length:120},(_,i)=>`ROW-${i.toString().padStart(3,'0')}\r\n`).join(''));
  await until(async()=>(await page.locator('.xterm-rows').innerText()).includes('ROW-119'));
  const window=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title.includes('SoDeck'));
  for(const scale of (process.argv.includes('--dpi')?[100,125,150,200]:[originalDpi.percent])){
  if(process.argv.includes('--dpi')){await dpi('set',scale);await delay(600);}
  await nativeWindow(app,'move',{WindowHandle:window.handle,X:20,Y:20});
  for(const height of [660,661,672,699,720,800,901]){
    await nativeWindow(app,'resize',{WindowHandle:window.handle,Width:1200,Height:height});await delay(180);
    const shell=await app.page.evaluate(()=>{const s=document.querySelector('.native-surface')??document.querySelector('[data-view-id]');return s?.getBoundingClientRect().toJSON();});
    const geometry=await page.evaluate(()=>{const rect=e=>e.getBoundingClientRect().toJSON();const rows=[...document.querySelectorAll('.xterm-rows>div')];return {dpi:devicePixelRatio,viewport:{width:innerWidth,height:innerHeight},content:rect(document.querySelector('.terminal-content')),grid:document.querySelector('.terminal-grid')?rect(document.querySelector('.terminal-grid')):null,screen:rect(document.querySelector('.xterm-screen')),last:rect(rows.at(-1)),rows:rows.length,tail:rows.slice(-2).map(r=>r.textContent)};});
    const native=(await nativeWindow(app,'list')).find(w=>w.handle===window.handle);
    measurements.push({height,shell,native,geometry});
    assert.equal(geometry.dpi,scale/100);
    if(mode!=='baseline')assert.ok(geometry.last.bottom<=Math.min(geometry.grid?.bottom??geometry.content.bottom,geometry.viewport.height)+.6,JSON.stringify(geometry));
  }
  await page.screenshot({path:join(output,`terminal-${scale}.png`)});
  }
  await page.screenshot({path:join(output,'terminal.png')});
  if(process.argv.includes('--behavior')){
    await nativeWindow(app,'show',{WindowHandle:window.handle});
    await page.locator('.xterm-screen').hover();await page.mouse.wheel(0,-600);await delay(250);
    const top=()=>page.locator('.xterm-rows>div').first().textContent();const historical=await top();
    await nativeWindow(app,'resize',{WindowHandle:window.handle,Width:1200,Height:930});await delay(250);assert.equal(await top(),historical);
    const original=(await rpc(app.page,'resources.list')).ptys.find(p=>p.viewId===viewId).id;
    await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:server.id,operationId:crypto.randomUUID()});
    await rpc(app.page,'views.activate',{viewId});await delay(250);assert.equal(await top(),historical);
    assert.equal((await rpc(app.page,'resources.list')).ptys.find(p=>p.viewId===viewId).id,original);
    await page.locator('.xterm-screen').hover();await page.mouse.wheel(0,100000);await delay(150);
    await page.locator('.xterm-helper-textarea').focus();await page.keyboard.press('Shift+F10');await delay(150);
    for(let i=0;i<4;i++)await nativeWindow(app,'menuKey',{WindowHandle:window.handle,KeyCode:40});await nativeWindow(app,'menuKey',{WindowHandle:window.handle,KeyCode:13});
    await until(async()=>(await page.locator('.terminal-status').innerText()).includes('字号 15'));
    const geometry=await page.evaluate(()=>{const last=[...document.querySelectorAll('.xterm-rows>div')].at(-1).getBoundingClientRect(),grid=document.querySelector('.terminal-grid').getBoundingClientRect();return {lastBottom:last.bottom,gridBottom:grid.bottom};});assert.ok(geometry.lastBottom<=geometry.gridBottom+.6);
    await writeFile(join(output,'behavior.json'),JSON.stringify({historical,geometry,ptyPreserved:true},null,2));
    console.log('Historical viewport survives resize/hide/restore; native font menu changes size and preserves a full last row');
  }
  console.log(JSON.stringify({output,mode,measurements:measurements.length,maxOverflow:Math.max(...measurements.map(m=>m.geometry.last.bottom-Math.min(m.geometry.grid?.bottom??m.geometry.content.bottom,m.geometry.viewport.height)))}));
}catch(error){if(page)await page.screenshot({path:join(output,'failure.png')}).catch(()=>{});if(app)await writeFile(join(output,'failure-state.json'),JSON.stringify(await rpc(app.page,'workbench.snapshot'),null,2));throw error;}finally{try{await dpi('set',originalDpi.percent);}finally{await stop(app);fixture.kill();await writeFile(join(output,'measurements.json'),JSON.stringify({mode,measurements,stderr,originalDpi,restoredDpi:await dpi('get')},null,2));await writeFile(`.local/test-results/t015/terminal-${mode}-latest.json`,JSON.stringify({output},null,2));}}
