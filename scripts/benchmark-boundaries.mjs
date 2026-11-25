import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import {createServer} from 'node:http';import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {parseArgs} from 'node:util';
import {build} from 'vite';
const {values}=parseArgs({options:{report:{type:'string'},process:{type:'boolean'}}});
const report={kind:'boundary-render-benchmark',platform:process.platform,arch:process.arch,node:process.version,measurements:[]};
const emit=value=>{report.measurements.push(value);console.log(JSON.stringify(value));};
const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..'),{chromium}=createRequire(path.join(repo,'package.json'))('playwright');
const server=createServer((req,res)=>{const url=new URL(req.url,'http://localhost');if(url.pathname==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><html><head><meta charset="utf-8"></head><body></body></html>');return;}const file=path.resolve(repo,'.'+decodeURIComponent(url.pathname));if(!file.startsWith(repo+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}res.setHeader('Content-Type',file.endsWith('.css')?'text/css':'text/javascript');res.end(fs.readFileSync(file));});
await new Promise(r=>server.listen(0,'127.0.0.1',r));let browser;
try{
 browser=await chromium.launch({headless:true});const base='http://127.0.0.1:'+server.address().port;
 emit({scope:'synthetic Chromium render+forced layout; no native WebView, remote IO, or bridge latency',cpu:os.cpus()[0].model,browser:browser.version(),repetitions:30,warmups:2});
 const page=await browser.newPage({viewport:{width:1000,height:780},reducedMotion:'reduce'});await page.goto(base);await page.addStyleTag({path:path.join(repo,'plugins/files/ui/style.css')});await page.addScriptTag({url:base+'/plugins/files/ui/tree.js'});
 for(const count of [100,1000,5000]){
  const result=await page.evaluate(({count})=>{
   document.body.innerHTML='<main id="root" style="width:340px;height:780px"></main>';
   const noop=()=>{}, tree=new FileTree(document.querySelector('#root'),{data:async()=>({}),event:async()=>({}),request:async()=>({}),openFile:noop,download:noop,message:noop,state:null});
   const root=tree.node('/',{kind:'directory'},'/');root.loaded=true;tree.expanded.add('/');tree.selected='/';tree.selection.add('/');
   for(let i=0;i<count;i++){const p='/file-'+String(i).padStart(6,'0');tree.node(p,{kind:'file',size:'64'});root.children.push(p);}
   const samples=[];for(let i=0;i<32;i++){const start=performance.now();tree.render(false);tree.tree.getBoundingClientRect();const elapsed=performance.now()-start;if(i>=2)samples.push(elapsed);}
   samples.sort((a,b)=>a-b);const result={kind:'file-tree',entries:count,domRows:tree.tree.querySelectorAll('.tree-row').length,p50Ms:samples[14],p95Ms:samples[28],maxMs:samples.at(-1),samplesMs:samples};tree.dispose();return result;
  },{count});emit(result);
 }
 await page.close();
 const logs=await browser.newPage({viewport:{width:1000,height:780},reducedMotion:'reduce'});await logs.goto(base);await logs.addStyleTag({path:path.join(repo,'plugins/docker/ui/logs.css')});
 for(const count of [50000,200000]){
  const result=await logs.evaluate(async({count})=>{
   const {createLogRenderer}=await import('/plugins/docker/ui/logs-renderer.js'),{createLogClient}=await import('/plugins/docker/ui/logs-client.js');
   document.body.innerHTML='<div class="log-viewport" style="height:740px;width:960px;position:relative"><div id="body"></div></div>';
   const viewport=document.querySelector('.log-viewport'),root=document.querySelector('#body'),ids=Array.from({length:count},(_,i)=>String(i+1));let revision='',requests=0;
   window.sodeck={request:async(method,p)=>{requests++;if(p.op==='layout'){const next=Math.min(count,p.start+900);return{revision,total:count,next,ids:ids.slice(p.start,next)};}if(p.op==='window'){const next=Math.min(count,p.start+p.count);return{revision,total:count,rows:ids.slice(p.start,next).map((id,k)=>{const text='INFO '+ 'message '.repeat((p.start+k)%50===0?40:4);return{id,text,length:text.length,matches:[],time:'2026-11-24T00:00:00Z',receivedAt:0,source:'stdout',level:'INFO'};})};}throw Error(p.op);}};
   const client=createLogClient(()=>true),renderer=createLogRenderer(viewport,root,()=>{}),samples=[];let lastRequests=0;
   for(let i=0;i<32;i++){renderer.reset();revision=String(i+1);const before=requests,start=performance.now();await renderer.paint(client,{revision,following:true,mode:'latest'},false);root.getBoundingClientRect();const elapsed=performance.now()-start;if(i>=2)samples.push(elapsed);lastRequests=requests-before;}
   samples.sort((a,b)=>a-b);const result={kind:'docker-logs',records:count,domRows:root.querySelectorAll('.log-line').length,requestsPerRevision:lastRequests,p50Ms:samples[14],p95Ms:samples[28],maxMs:samples.at(-1),samplesMs:samples};renderer.dispose();client.dispose();return result;
  },{count});emit(result);
 }
 await logs.close();
 const buildDirectory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-boundary-bench-'));
 try {
  const entry=path.join(buildDirectory,'core.ts');fs.writeFileSync(entry,'import {logResults} from '+JSON.stringify(path.join(repo,'src/views.ts').replaceAll('\\','/'))+';\nimport {updateDOM} from '+JSON.stringify(path.join(repo,'src/dom-update.ts').replaceAll('\\','/'))+';\nexport function patch(root:HTMLElement,logs:any[]){updateDOM(root,logResults(logs));}');
  const built=await build({root:repo,configFile:false,logLevel:'silent',build:{write:false,minify:false,lib:{entry,name:'CoreLogProbe',formats:['iife']}}});
  const code=(Array.isArray(built)?built[0]:built).output.find(item=>item.type==='chunk').code;
  const page=await browser.newPage({viewport:{width:1000,height:780},reducedMotion:'reduce'});
  await page.goto(base);await page.addScriptTag({content:code});await page.addStyleTag({path:path.join(repo,'src/console.css')});
  for(const size of [120,8192]){
   emit(await page.evaluate(({size})=>{
    document.body.innerHTML='<div id="log-results" class="log-results" style="width:960px;height:740px;overflow:auto"></div>';
    const root=document.querySelector('#log-results'),message='message '.repeat(Math.ceil(size/8)).slice(0,size),records=Array.from({length:532},(_,i)=>({id:String(i),receivedAt:1795478400000+i*1000,source:'plugin',pluginId:'sodeck.docker',instanceId:'probe',level:'info',message:i+' '+message,fields:{sequence:i}})),samples=[];
    for(let i=0;i<32;i++){const start=performance.now();CoreLogProbe.patch(root,records.slice(i,i+500));void root.scrollHeight;const elapsed=performance.now()-start;if(i>=2)samples.push(elapsed);}
    samples.sort((a,b)=>a-b);return {kind:'core-logs',records:500,messageBytes:size,domRows:root.querySelectorAll('.log-row').length,p50Ms:samples[14],p95Ms:samples[28],maxMs:samples.at(-1),samplesMs:samples};
   },{size}));
  }
  await page.close();
 } finally {fs.rmSync(buildDirectory,{recursive:true,force:true});}

}finally{if(browser)await browser.close();await new Promise(r=>server.close(r));}

if(values.process){
 const {createDockerLogFixture}=await import('./docker-logs-fixture.mjs');const {until}=await import('./docker-process.mjs');
 const fixture=await createDockerLogFixture();
 try {
  await fixture.event({action:'logs.open',requestId:'benchmark',intentSequence:'1',range:{kind:'latest'}});
  await until(()=>fixture.read({op:'status'}),s=>s.ready);fixture.append('INFO benchmark',10000);
  const state=await until(()=>fixture.read({op:'status'}),s=>s.windowTotal===10000&&s.total===10000,30000);
  const samples=[];let requests=0;
  for(let i=0;i<32;i++){let start=0,total=0;requests=0;const began=performance.now();do{const page=await fixture.read({op:'layout',revision:state.revision,start});if(page.next<=start)throw Error('layout did not progress');start=page.next;total=page.total;requests++;}while(start<total);if(i>=2)samples.push(performance.now()-began);}
  samples.sort((a,b)=>a-b);emit({kind:'docker-layout-plugin-process',records:10000,requests,p50Ms:samples[14],p95Ms:samples[28],maxMs:samples.at(-1),samplesMs:samples,scope:'actual Docker process RPC; fixture core, no Tauri WebView bridge'});
 } finally {await fixture.close();}
}
if(values.report){fs.mkdirSync(path.dirname(values.report),{recursive:true});fs.writeFileSync(values.report,JSON.stringify(report,null,2)+'\n');}
