import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {createServer} from 'node:http';
import {chromium} from 'playwright';
import {packageMenuAssets} from './plugin-menu-assets.mjs';
import {packageStyles} from '../packages/ui/package.mjs';
import {readTestThemeTokens} from './test-theme-tokens.mjs';
const root=path.resolve('plugins/mcp'), assets=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-mcp-ui-'));
await packageMenuAssets(assets);packageStyles(assets);
fs.copyFileSync('plugins/shared/clipboard.js',path.join(assets,'ui/clipboard-bridge.js'));
const server=createServer((req,res)=>{
  if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><body style="margin:0"><iframe title="MCP 状态" style="border:0;width:100vw;height:100vh" src="/ui/index.html"></iframe>');return;}
  const url=req.url.split('?')[0],base=['/ui/text-menu.js','/ui/shared.css','/ui/clipboard-bridge.js'].includes(url)?assets:root,file=path.join(base,url);
  if(!file.startsWith(base+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}
  res.setHeader('Content-Type',{'.js':'text/javascript','.html':'text/html','.css':'text/css'}[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const browser=await chromium.launch({headless:true}),themes=await readTestThemeTokens(browser),page=await browser.newPage({viewport:{width:320,height:850}});
let snapshots=0,configs=0,clipboard='',actions=[],fail=false,hold;
const data={service:'ready',endpoint:'http://127.0.0.1:19846/mcp',target:{state:'connected',target:{name:'prod-consul-01',endpoint:'192.0.2.11:22',username:'ops'}},evidenceDirectory:'/home/ops/.local/share/sodeck/plugin-data/sodeck.mcp/evidence',tasks:[{taskId:'t1',kind:'exec',requestId:'collect-consul-logs',native:{state:'running'},done:false,evidence:{complete:false}},{taskId:'t2',kind:'download',requestId:'consul-evidence',native:{state:'completed'},done:true,evidence:{complete:true}},{taskId:'t3',kind:'exec',requestId:'inspect-network',native:{state:'failed',exitCode:7},done:true,evidence:{complete:false,error:{message:'输出超出额度，请导出文件后下载。'}}}]};
const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.exposeFunction('mcpRequest',async(method,p)=>{
  if(method==='clipboard'){if(p.op==='beginWrite'){clipboard='';return {id:'clip'};}if(p.op==='writeChunk'){clipboard+=p.text;return {};}return {};}
  if(method==='data'){actions.push(p);return {};}
  actions.push(p.action);
  if(p.action==='config'){configs++;return {config:'[mcp_servers.sodeck]\nurl = "http://127.0.0.1:19846/mcp"\nhttp_headers = { Authorization = "Bearer illustrative-secret" }'};}
  if(p.action==='snapshot'){snapshots++;if(hold)await new Promise(r=>{hold=r;});if(fail)throw new Error('核心暂不可用，请稍后刷新。');}
  return structuredClone(data);
});
await page.addInitScript(()=>{
  if(window!==top)return;
  addEventListener('message',async e=>{const m=e.data;if(m?.channel!=='sodeck-ui-v2')return;
    if(m.type==='ready')e.source.postMessage({channel:m.channel,type:'mount',surfaceId:'test',visible:true,capabilities:{clipboard:1},tokens:{}},'*');
    if(m.type==='request')try{const data=await window.mcpRequest(m.method,m.params);e.source.postMessage({channel:m.channel,type:'response',surfaceId:'test',id:m.id,data},'*');}catch(error){e.source.postMessage({channel:m.channel,type:'response',surfaceId:'test',id:m.id,error:error.message},'*');}
  });
});
const frame=page.frameLocator('iframe');
const message=(type,extra={})=>page.evaluate(m=>document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'test',...m},'*'),{type,...extra});
try{
  await page.goto('http://127.0.0.1:'+server.address().port);await frame.locator('#server').filter({hasText:'prod-consul-01'}).waitFor();
  assert.equal(configs,0,'snapshot never discloses configuration');
  await frame.getByRole('button',{name:'取消 collect-consul-logs'}).focus();await frame.getByRole('button',{name:'刷新',exact:true}).click();
  await frame.getByRole('button',{name:'取消 collect-consul-logs'}).focus();const before=snapshots;await page.waitForFunction(()=>document.querySelector('iframe').contentDocument.activeElement?.textContent==='取消');
  await new Promise(r=>setTimeout(r,1700));assert(snapshots>before);assert.equal(await frame.locator(':focus').textContent(),'取消');
  await frame.getByRole('button',{name:'取消 collect-consul-logs'}).click();await page.waitForFunction(()=>!document.querySelector('iframe').contentDocument.querySelector('#tasks button').disabled);assert(actions.includes('cancel'));
  await frame.getByRole('button',{name:'复制 Codex 配置'}).click();await frame.locator('#notice').filter({hasText:'配置已复制'}).waitFor();assert(clipboard.includes('illustrative-secret'));
  await frame.locator('summary').click();await frame.locator('#config').filter({hasText:'illustrative-secret'}).waitFor();
  await message('visibilityChanged',{visible:false});await page.waitForFunction(()=>document.querySelector('iframe').contentDocument.querySelector('#config').textContent==='');const hiddenCount=snapshots;await new Promise(r=>setTimeout(r,1700));assert.equal(snapshots,hiddenCount);
  await message('visibilityChanged',{visible:true});await frame.locator('#server').filter({hasText:'prod-consul-01'}).waitFor();
  await frame.getByRole('button',{name:'打开证据目录'}).click();await page.waitForFunction(()=>!document.querySelector('iframe').contentDocument.querySelector('#reveal').disabled);assert(actions.some(p=>p?.op==='pluginData'&&p.action==='reveal'));
  fs.mkdirSync('artifacts/mcp-ui',{recursive:true});
  for(const [name,tokens]of Object.entries(themes))for(const width of [200,320,640]){
    await page.setViewportSize({width,height:1000});await message('themeChanged',{tokens});
    assert(await frame.locator('body').evaluate(el=>getComputedStyle(el).backgroundColor!== 'rgba(0, 0, 0, 0)'));
    assert(await frame.locator('main').evaluate(el=>el.ownerDocument.documentElement.scrollWidth<=innerWidth+1));
    if(width===320)await page.screenshot({path:`artifacts/mcp-ui/${name}.png`,fullPage:true});
  }
  fail=true;await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#notice').filter({hasText:'核心暂不可用'}).waitFor();fail=false;
  data.tasks=[];await frame.getByRole('button',{name:'刷新',exact:true}).click();await frame.locator('#empty').waitFor();
  hold=true;await frame.getByRole('button',{name:'刷新',exact:true}).click();while(typeof hold!=='function')await new Promise(r=>setTimeout(r,10));await message('visibilityChanged',{visible:false});await message('visibilityChanged',{visible:true});hold();hold=null;const resume=snapshots;await new Promise(r=>setTimeout(r,1700));assert(snapshots>resume,'in-flight visibility change resumes polling');
  await message('unmount');const unmounted=snapshots;await new Promise(r=>setTimeout(r,1700));assert.equal(snapshots,unmounted);assert.deepEqual(errors,[]);
  console.log('MCP UI: themes, narrow layouts, focus, config, cancel, reveal, failure, empty, visibility and unmount passed');
}finally{await browser.close();await new Promise(r=>server.close(r));assert.equal(path.dirname(assets),os.tmpdir());fs.rmSync(assets,{recursive:true,force:true});}
