import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createServer} from 'node:http';
import {chromium} from 'playwright';
import {packageRoot,context,until} from './docker-process.mjs';
import {readTestThemeTokens} from './test-theme-tokens.mjs';
export async function verifyDockerPortsUI(){
  const root=packageRoot(),producer={kind:'plugin',pluginId:'sodeck.network-proxy',pluginInstanceId:'network-instance',viewId:'manager'};
  const target={host:'127.0.0.1',port:8080},binding={containerPort:80,protocol:'tcp',hostIp:'0.0.0.0',hostPort:8080};
  let rules=[],subscriptions=0,unsubscriptions=0,copy='',mutations=[];
  let snapshot={kind:'container',context,containerId:'a'.repeat(64),endpoint:'unix:///var/run/docker.sock',engine:{id:'engine',name:'Engine'},revision:'1',state:'ready',status:'running',busy:false,navigation:{section:'ports'}};
  const portRows=[{key:'binding',containerPort:'80/tcp',binding,target},{key:'udp',containerPort:'53/udp',binding:null,target:null,reason:'当前快捷隧道仅支持 TCP'},{key:'unpublished',containerPort:'3306/tcp',binding:null,target:null,reason:'此端口未发布到服务器，暂不支持快捷创建'}];
  const rule=id=>({version:1,rule:{id,revision:1,type:'tcpForward',name:'Docker · 服务 '+id,bind:{host:'127.0.0.1',port:0},target},run:null});
  const record=(topic,key,value)=>({topic,key,value,present:true,revision:'1',producer});
  const records=()=>[record('network.tunnelService.v1','current',{version:1,ready:true,publishState:'synced',observedAt:Date.now()}),...rules.map(r=>record('network.tunnelRules.v1',r.rule.id,r))];
  const server=createServer((req,res)=>{if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><html><body style="margin:0"><iframe title="端口隧道" style="width:100vw;height:100vh;border:0" src="/ui/index.html"></iframe></body></html>');return;}const file=path.resolve(root,'.'+decodeURIComponent(req.url.split('?')[0]));if(!file.startsWith(root+path.sep)||!fs.existsSync(file)){res.writeHead(404);res.end();return;}res.setHeader('Content-Type',{'.js':'text/javascript','.html':'text/html','.css':'text/css'}[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const browser=await chromium.launch({headless:true});const themes=await readTestThemeTokens(browser),page=await browser.newPage({viewport:{width:1100,height:760},reducedMotion:'reduce'}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.exposeFunction('mock',async(method,data)=>{
    if(method==='state'){
      if(data.op==='subscribe'){if(!data.acknowledgedRevision)subscriptions++;return {result:{subscriptionId:'sub-'+subscriptions,revision:'1',scope:{connected:true,frozen:false},records:records(),leaseMs:60000}};}
      if(data.op==='unsubscribe'){unsubscriptions++;return {result:{}};}
      return {result:{}};
    }
    if(method==='clipboard'){if(data.op==='beginWrite'){copy='';return {id:'clipboard'};}if(data.op==='writeChunk')copy+=data.text;return {};}
    if(method==='event'){
      if(['snapshot','resumeInput'].includes(data.action))return snapshot;
      if(data.action==='ports.list')return {rows:portRows,total:3,nextOffset:null};
      if(data.action==='section')return {rows:[],total:0,nextOffset:null};
      if(data.action==='navigation'){snapshot.navigation=data.state;return snapshot;}
      if(data.action.startsWith('tunnel.')){mutations.push(data);snapshot={...snapshot,revision:String(Number(snapshot.revision)+1),tunnel:{status:'pending'}};return snapshot;}
      throw new Error('Unexpected port event: '+data.action);
    }
    return {};
  });
  await page.addInitScript(({snapshot,tokens})=>{if(window!==top)return;window.snapshot=snapshot;window.tokens=tokens;window.post=(type,extra={})=>document.querySelector('iframe').contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:'ports',type,...extra},'*');addEventListener('message',async event=>{if(event.source!==document.querySelector('iframe')?.contentWindow||event.data?.channel!=='sodeck-ui-v2')return;const m=event.data;if(m.type==='ready')window.post('mount',{data:window.snapshot,context:window.snapshot.context,tokens:window.tokens,capabilities:{clipboard:1,sharedState:1}});if(m.type==='prepared')window.post('surfaceReady');if(m.type==='request'){try{window.post('response',{id:m.id,data:await window.mock(m.method,m.params)});}catch(e){window.post('response',{id:m.id,error:e.message});}}});},{snapshot,tokens:themes.light});
  try{
    await page.goto('http://127.0.0.1:'+server.address().port);const frame=page.frames()[1],ports=frame.locator('#port-tunnels');await ports.getByRole('button',{name:'创建隧道',exact:true}).first().waitFor();
    await ports.getByRole('button',{name:'创建隧道',exact:true}).first().click();assert.equal(mutations.length,1);assert.deepEqual(mutations[0].binding,binding);await ports.getByText('正在核对端口并办理隧道请求…').waitFor();await ports.getByRole('button',{name:'创建隧道',exact:true}).first().dispatchEvent('click');assert.equal(mutations.length,1);
    snapshot={...snapshot,tunnel:{status:'completed',result:{outcome:'accepted'}}};const running=rule('one');running.run={runId:'run-one',rule:running.rule,snapshot:{state:'running',address:{host:'127.0.0.1',port:49152},cleanupComplete:false},released:false,observedAt:Date.now()};running.check={ok:true,runId:'run-one',observedAt:Date.now()};rules=[running];
    const remount=async()=>{await page.evaluate(s=>{window.snapshot=s;window.post('mount',{data:s,context:s.context,tokens:window.tokens,capabilities:{clipboard:1,sharedState:1}});},snapshot);};
    await remount();await ports.getByText('127.0.0.1:49152',{exact:true}).waitFor();await ports.getByRole('button',{name:'复制入口',exact:true}).click();await ports.getByText('入口已复制',{exact:true}).waitFor();assert.equal(copy,'127.0.0.1:49152');await ports.getByRole('button',{name:'查看隧道',exact:true}).click();await until(async()=>mutations.at(-1)?.revealRule,v=>v==='one');assert.equal(mutations.at(-1).revealRule,'one');
    snapshot={...snapshot,tunnel:null};rules=[rule('one'),rule('two')];await remount();const select=ports.getByRole('combobox');await select.waitFor();await select.selectOption('two');await ports.getByRole('button',{name:'启动隧道',exact:true}).click();await until(async()=>mutations.at(-1)?.selectedRule,v=>v?.id==='two');assert.deepEqual(mutations.at(-1).selectedRule,{id:'two',revision:1});
    snapshot={...snapshot,tunnel:null};rules=[running];await remount();await ports.getByRole('button',{name:'复制入口',exact:true}).waitFor();const before=unsubscriptions;await page.evaluate(()=>window.post('visibilityChanged',{visible:false}));await page.waitForFunction(()=>document.querySelector('iframe').contentDocument.querySelector('#port-tunnels').hidden);assert(unsubscriptions>before);await page.evaluate(()=>window.post('visibilityChanged',{visible:true}));await ports.getByRole('button',{name:'复制入口',exact:true}).waitFor();
    fs.mkdirSync('artifacts/docker-ports-ui',{recursive:true});
    for(const [theme,tokens]of Object.entries(themes))for(const width of [1100,360]){await page.setViewportSize({width,height:760});await page.evaluate(tokens=>window.post('themeChanged',{tokens}),tokens);await ports.scrollIntoViewIfNeeded();assert(await ports.evaluate(el=>el.scrollWidth<=el.clientWidth+2),theme+' port rows overflow');await ports.screenshot({path:`artifacts/docker-ports-ui/${theme}-${width}.png`});}
    assert.equal((await ports.getByText('当前快捷隧道仅支持 TCP',{exact:true}).count()),1);assert.equal(errors.length,0,errors.join('\n'));assert(subscriptions>=4);
    console.log('Docker ports UI: shared-state subscription, create-once, copy, reveal, explicit candidate selection, hide/resume, unavailable ports, three themes and narrow layouts passed.');
  }finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
}
