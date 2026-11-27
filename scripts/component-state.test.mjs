import assert from 'node:assert/strict';
import fs from 'node:fs';
import {test} from 'node:test';
const bus=new EventTarget();globalThis.addEventListener=bus.addEventListener.bind(bus);globalThis.removeEventListener=bus.removeEventListener.bind(bus);
const {SharedState}=await import('../packages/plugin-sdk/src/state.js');
const {parseOsc7,parseIntegration,createLocation}=await import('../plugins/terminal/ui-src/location.js');
const fixture=JSON.parse(fs.readFileSync('scripts/fixtures/shared-state.json','utf8'));
const tick=()=>new Promise(resolve=>setImmediate(resolve));
test('JS wire contract applies a multipart batch atomically with decimal revisions and null tombstones',async()=>{
 const requests=[];globalThis.sodeck={request:async(_,p)=>{requests.push(p);return p.op==='subscribe'&&!p.acknowledgedRevision?{subscriptionId:'sub',scope:{state:'active'},revision:fixture.initialRevision,records:[],leaseMs:60000}:{leaseMs:60000};}};
 bus.dispatchEvent(new Event('sodeck:surfaceReady'));const client=new SharedState({context:fixture.context,capabilities:{sharedState:1}});const watch=client.watch({topics:['test.v1']},()=>{});await tick();
 for(const [i,raw]of fixture.frames.entries()){watch.receive({...raw,context:fixture.context,subscriptionId:'sub',clientSubscriptionId:watch.clientId});await tick();assert.equal(watch.revision,i?raw.toRevision:fixture.initialRevision);assert.equal(watch.records.size,0);}
 watch.receive({...fixture.frames[1],context:fixture.context,subscriptionId:'sub',clientSubscriptionId:watch.clientId});await tick();assert(watch.valid);assert.equal(requests.filter(p=>p.op==='ack').length,3);assert.equal(requests.find(p=>p.op==='ack').appliedRevision,undefined);
 watch.expires=performance.now()-1;assert.equal(watch.valid,false,'expiry is immediate even before a delayed timer fires');watch.receive({...fixture.frames[1],context:fixture.context,subscriptionId:'sub',clientSubscriptionId:watch.clientId});await tick();assert.equal(requests.filter(p=>p.op==='ack').length,3);client.dispose();
});
test('disposing during initialization and reset owns all pending watches and cancels retries',async()=>{
 let release;const requests=[];globalThis.sodeck={request:async(_,p)=>{requests.push(p);if(p.op==='subscribe')return new Promise(resolve=>release=resolve);return {};}};const client=new SharedState({context:fixture.context,capabilities:{sharedState:1}});const watch=client.watch({topics:['test.v1']},()=>{});await tick();client.dispose();release({});await tick();assert(watch.disposed);assert(requests.some(p=>p.op==='unsubscribe'));
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
test('local terminal directory context follows confirmed reports and invalidation without shared-state RPC',()=>{
 const handlers=new Map(),locations=[];let called=false;globalThis.sodeck={request:async()=>{called=true;throw new Error('unexpected RPC');}};
 const terminal={parser:{registerOscHandler:(code,handler)=>{handlers.set(code,handler);return {dispose(){}};}}};
 const location=createLocation(terminal,value=>locations.push(value));location.mount({context:fixture.context,capabilities:{},data:{streamId:'pty',openedContext:fixture.context,integration:{mode:'auto',token:'token'}}});
 location.frame('data',123);handlers.get(633)(`SODECK;token;ready;${Buffer.from('/cwd').toString('hex')};${Buffer.from('/tmp/witness').toString('hex')}`);assert.equal(locations.at(-1).state,'unknown');location.completed();assert.equal(locations.at(-1).path,'/cwd');assert.equal(locations.at(-1).outputOffset,'123');
 location.input('\r');assert.equal(locations.at(-1).activity,'busy');location.invalidate();assert.equal(locations.at(-1).state,'unknown');assert.equal(called,false);location.dispose();
});
test('disabled directory integration ignores OSC7 and OSC633 and publishes only unknown state',async()=>{
 const writes=[],locations=[],handlers=new Map();globalThis.sodeck={request:async(_,p)=>(p.op==='get'?{record:{present:true,revision:'1',value:{state:'known',path:'/old',terminalId:'pty',outputOffset:'0'}}}:writes.push(p))};
 const terminal={parser:{registerOscHandler:(id,handler)=>{handlers.set(id,handler);return {dispose(){}};}}};
 const location=createLocation(terminal,value=>locations.push(value));location.mount({context:fixture.context,capabilities:{sharedState:1},settings:{values:{directoryReportHost:'host'}},data:{streamId:'pty',openedContext:fixture.context,integration:{mode:'off',token:'token'}}});
 location.ready();await tick();
 location.frame('data',100);handlers.get(7)('file://host/should-not-follow');handlers.get(633)('SODECK;token;ready;2f776f726b;2f746d70');location.completed();location.input('command\n');await tick();
 assert(locations.some(value=>value.reasonCode==='disabled'));assert(locations.every(value=>value.state==='unknown'));assert.equal(writes.length,0);location.dispose();
});
