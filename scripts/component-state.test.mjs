import assert from 'node:assert/strict';
import fs from 'node:fs';
import {test} from 'node:test';
const bus=new EventTarget();globalThis.addEventListener=bus.addEventListener.bind(bus);globalThis.removeEventListener=bus.removeEventListener.bind(bus);
const {SharedState}=await import('../plugins/shared/state.js');
const {parseOsc7,parseIntegration}=await import('data:text/javascript;base64,'+Buffer.from(fs.readFileSync('plugins/terminal/ui-src/location.js','utf8').replace("'./state.js'",JSON.stringify(new URL('../plugins/shared/state.js',import.meta.url).href))).toString('base64'));
const fixture=JSON.parse(fs.readFileSync('scripts/fixtures/shared-state.json','utf8'));
const tick=()=>new Promise(resolve=>setImmediate(resolve));
test('JS wire contract applies a multipart batch atomically with decimal revisions and null tombstones',async()=>{
 const requests=[];globalThis.sodeck={request:async(_,p)=>{requests.push(p);return {result:p.op==='subscribe'&&!p.acknowledgedRevision?{subscriptionId:'sub',scope:{state:'active'},revision:fixture.initialRevision,records:[],leaseMs:60000}:{leaseMs:60000}};}};
 bus.dispatchEvent(new Event('sodeck:surfaceReady'));const client=new SharedState({context:fixture.context,capabilities:{sharedState:1}});const watch=client.watch({topics:['test.v1']},()=>{});await tick();
 for(const [i,raw]of fixture.frames.entries()){watch.receive({...raw,context:fixture.context,subscriptionId:'sub',clientSubscriptionId:watch.clientId});await tick();assert.equal(watch.revision,i?raw.toRevision:fixture.initialRevision);assert.equal(watch.records.size,0);}
 watch.receive({...fixture.frames[1],context:fixture.context,subscriptionId:'sub',clientSubscriptionId:watch.clientId});await tick();assert(watch.valid);assert.equal(requests.filter(p=>p.op==='ack').length,3);assert.equal(requests.find(p=>p.op==='ack').appliedRevision,undefined);
 watch.expires=performance.now()-1;assert.equal(watch.valid,false,'expiry is immediate even before a delayed timer fires');watch.receive({...fixture.frames[1],context:fixture.context,subscriptionId:'sub',clientSubscriptionId:watch.clientId});await tick();assert.equal(requests.filter(p=>p.op==='ack').length,3);client.dispose();
});
test('disposing during initialization and reset owns all pending watches and cancels retries',async()=>{
 let release;const requests=[];globalThis.sodeck={request:async(_,p)=>{requests.push(p);if(p.op==='subscribe')return new Promise(resolve=>release=resolve);return {result:{}};}};const client=new SharedState({context:fixture.context,capabilities:{sharedState:1}});const watch=client.watch({topics:['test.v1']},()=>{});await tick();client.dispose();release({result:{}});await tick();assert(watch.disposed);assert(requests.some(p=>p.op==='unsubscribe'));
 const other=new SharedState({context:fixture.context,capabilities:{sharedState:1}});const resetting=other.watch({topics:['test.v1']},()=>{});await tick();resetting.reset(new Error('reset'));other.dispose();assert(resetting.disposed);assert.equal(other.owned.size,0);
});
test('OSC 7 verifies host, decoded absolute paths and rejects ambiguous locations',()=>{
 assert.equal(parseOsc7('file://host/%E7%9B%AE%E5%BD%95%20x','host').path,'/目录 x');assert.equal(parseOsc7('file://host/C:/Users/a','host').path,'C:\\Users\\a');for(const uri of ['file://other/path','file://user@host/path','file://host/a?b','file://host/%00','file://host/%xx','file://host/a#b'])assert.equal(parseOsc7(uri,'host'),null,uri);assert.equal(parseOsc7('file://host/path',''),null);
});
test('automatic reports bind to one shell and keep a filesystem witness',()=>{
 const hex=s=>Buffer.from(s).toString('hex');const path='/目录 with % # \' spaces';
 const report=`SODECK;token;ready;${hex(path)};${hex('/tmp/sodeck/identity')}`;
 const parsed=parseIntegration(report,'token');assert.equal(parsed.path,path);assert.equal(parsed.filesystemView,'shell-reported');assert.deepEqual(parsed.witness,{path:'/tmp/sodeck/identity',value:'token'});
 assert.equal(parseIntegration(report,'other'),null);assert.equal(parseIntegration('x'.repeat(18000),'token'),null);
 for(const value of ['ff','2f00','2f0a','abcdz',''])assert.equal(parseIntegration(`SODECK;token;ready;${value};2f`,'token').state,'unknown');
 assert.equal(parseIntegration('SODECK;token;busy;','token').activity,'busy');
});
test('old cores without shared-state capability keep the enhancement disabled without an RPC',async()=>{
 let called=false;globalThis.sodeck={request:async()=>{called=true;throw new Error('unexpected RPC');}};const client=new SharedState({context:fixture.context,capabilities:{}});assert.equal(client.available,false);await assert.rejects(client.request('get'),error=>error.code==='unsupported');assert.equal(called,false);client.dispose();
});
test('same-directory prompts after busy preserve manual browsing; explicit retry follows again',async()=>{
 let emit,count=0;const elements=[];const prior=globalThis.document;
 globalThis.document={createElement:tag=>{const element={tag,setAttribute(){},append(){},replaceChildren(){},remove(){},textContent:''};elements.push(element);return element;}};
 globalThis.FileStateClient=class {available=true;watch(_,change){emit=change;return {valid:true,dispose(){}};}dispose(){}};
 const code=fs.readFileSync('plugins/files/ui/shared-state.js','utf8').replace("import {SharedState} from './state.js';",'const SharedState=globalThis.FileStateClient;');
 const {connectFileState}=await import('data:text/javascript;base64,'+Buffer.from(code+'\n//manual busy test').toString('base64'));
 const tree={toolbar:{append(){}},setMounts(){},cancelFollow(){},async followLocation(){count++;}};
 const controller=connectFileState(tree,{capabilities:{fileLocation:1,fileLocationProof:1}},async()=>({}),async()=>{});
 const value={state:'known',path:'/cwd',pathStyle:'posix',filesystemView:'shell-reported',outputOffset:'1',terminalId:'terminal',activity:'ready'};
 const projection=v=>({scope:{state:'active',connected:true},records:[{topic:'workspace.presentation.v1',key:'current',producer:{kind:'core'},value:{activeEditorViewId:'view',activeEditorPluginId:'sodeck.terminal'}},{topic:'terminal.location.v1',key:'current',producer:{pluginId:'sodeck.terminal',viewId:'view'},value:v}]});
 try{emit(projection(value));await tick();assert.equal(count,1);tree.onManual();emit(projection({...value,state:'unknown',path:null,activity:'busy',outputOffset:'2'}));emit(projection({...value,outputOffset:'3'}));await tick();assert.equal(count,1);assert.match(elements.find(e=>e.tag==='summary').textContent,/已手动浏览/);elements.find(e=>e.textContent==='定位到终端目录').onclick();await tick();assert.equal(count,2);}finally{controller.dispose();globalThis.document=prior;delete globalThis.FileStateClient;}
});
test('background observations only cancel a pending follow, preserving manual navigation',async()=>{
 let emit,release,cancelled=0;const priorDocument=globalThis.document;
 globalThis.document={createElement:()=>({setAttribute(){},append(){},replaceChildren(){},remove(){}})};
 globalThis.FileStateClient=class {available=true;watch(_,change){emit=change;return {valid:true,dispose(){}};}dispose(){}};
 const code=fs.readFileSync('plugins/files/ui/shared-state.js','utf8').replace("import {SharedState} from './state.js';",'const SharedState=globalThis.FileStateClient;');
 const {connectFileState}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
 const tree={toolbar:{append(){}},setMounts(){},cancelFollow(){cancelled++;},followLocation:()=>new Promise(resolve=>release=resolve),message(){}};
 const controller=connectFileState(tree,{capabilities:{fileLocation:1}},async()=>({}),async()=>{});
 const empty={scope:{state:'active',connected:true},records:[]};
 try{
  emit(empty);emit(empty);controller.visibility(true);assert.equal(cancelled,0,'no producer must not cancel manual navigation');
  const active={...empty,records:[{topic:'workspace.presentation.v1',key:'current',producer:{kind:'core'},value:{activeEditorViewId:'view'}},{topic:'terminal.location.v1',key:'current',producer:{kind:'plugin',pluginId:'sodeck.terminal',pluginInstanceId:'instance',viewId:'view'},value:{state:'known',path:'/cwd',pathStyle:'posix',filesystemView:'session-default',outputOffset:'1',terminalId:'terminal'}}]};
  emit(active);controller.settings({values:{followTerminal:false}});assert.equal(cancelled,1,'switching off cancels the pending follow');release();await tick();
  emit(empty);emit(empty);assert.equal(cancelled,1,'later sampler events must not cancel a manual navigation');
 }finally{controller.dispose();globalThis.document=priorDocument;delete globalThis.FileStateClient;}
});
