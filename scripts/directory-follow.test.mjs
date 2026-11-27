import assert from 'node:assert/strict';
import {test} from 'node:test';
import {DirectoryTarget,latestLocation,validateLocation,connectDirectoryFollow} from '../plugins/files/ui/directory-follow.js';
import {createLocationPublication} from '../plugins/terminal/ui-src/location-publication.js';

const context={workspaceId:'w',serverId:'s',connectionId:'c',generation:1};
const id=n=>`00000000-0000-4000-8000-${String(n).padStart(12,'0')}`;
const value=(n,path='/srv/app')=>({version:1,publicationId:id(n),path,pathStyle:'posix',filesystemView:'session-default'});
const record=(revision,producer='custom',path='/srv/app')=>({topic:'workspace.location.v1',key:'current',revision:String(revision),producer:{kind:'plugin',pluginId:producer,pluginInstanceId:'i',viewId:producer},value:value(revision,path)});
const projection=(revision,records)=>({revision:String(revision),scope:{state:'active',connected:true},records});
const tick=()=>new Promise(resolve=>setImmediate(resolve));
const deferred=()=>{let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};};

test('directory selection uses numeric core revisions, independent of producer and timestamps',()=>{
  const rows=[record(9,'terminal'),record(10,'custom')];assert.equal(latestLocation(rows).producer.pluginId,'custom');
  const state=new DirectoryTarget(context);state.receive(projection(11,rows));assert.equal(state.target.value.publicationId,id(10));
  state.handled=true;state.manual=true;state.receive(projection(12,rows));assert(state.manual);assert(state.handled);
  state.receive(projection(13,[...rows,record(13,'docker','/compose')]));assert.equal(state.target.value.path,'/compose');assert(!state.handled);assert(!state.manual);
});
test('source disappearance and reset never fall back to an older producer',()=>{
  const state=new DirectoryTarget(context);state.receive(projection(10,[record(9),record(10,'docker')]));state.handled=true;
  state.receive(projection(11,[record(9)]));assert.equal(state.target,null);
  state.receive(null);assert(!state.valid);state.receive(projection(12,[record(9)]));assert.equal(state.target,null);
  state.receive(projection(13,[record(13)]));assert.equal(state.target.revision,'13');
});
test('manual checkpoints survive restore and cannot cross Context generations',()=>{
  const state=new DirectoryTarget(context);state.receive(projection(10,[record(10)]));state.handled=true;state.manual=true;
  const restored=new DirectoryTarget(context,state.snapshot());restored.receive(projection(11,[record(10)]));assert(restored.handled);assert(restored.manual);assert.equal(restored.target.revision,'10');
  const fresh=new DirectoryTarget({...context,generation:2},state.snapshot());assert.equal(fresh.seen,null);
});
test('latest malformed directory blocks older valid locations; byte limits and filesystem proof apply',()=>{
  const state=new DirectoryTarget(context),bad=record(10);bad.value.version=99;state.receive(projection(10,[record(9),bad]));assert.equal(state.target,null);assert.match(state.error,/版本/);
  assert.throws(()=>validateLocation({...value(1),path:'relative'}));
  assert.throws(()=>validateLocation({...value(1),path:'/'+ '中'.repeat(1400)}));
  assert.throws(()=>validateLocation({...value(1),filesystemView:'shell-reported'}));
  assert.equal(validateLocation({...value(1),filesystemView:'shell-reported',witness:{path:'/tmp/identity',value:'token'}}).path,'/srv/app');
});

function fileFixture(resolve=async(_,location)=>({path:location.path,attrs:{kind:'directory'}})){
  const elements=[],previous=globalThis.document;globalThis.document={createElement:()=>{const e={setAttribute(){},append(){},remove(){}};elements.push(e);return e;}};
  let emit,epoch=0,position='/manual',scroll=17,focused='input';const errors=[];
  const tree={toolbar:{append(){}},cancelNavigation(){epoch++;},navigation(){const e=epoch;return()=>{if(e!==epoch)throw Object.assign(new Error('cancelled'),{code:'cancelled'});};},async locateResolved(info,{check,focus}){check();assert.equal(focus,false);position=info.path;scroll=42;},};
  const client={watch(_,fn){emit=fn;return {dispose(){}};},dispose(){}};
  const control=connectDirectoryFollow(tree,{context,visible:true,data:{}},{client,data:resolve,save:async enabled=>control.settings({values:{followTerminal:enabled}})});
  control.lifecycle({active:true});
  return {control,tree,elements,emit:(...args)=>emit(...args),get position(){return position;},get scroll(){return scroll;},get focused(){return focused;},errors,dispose(){control.dispose();globalThis.document=previous;}};
}
test('manual browsing survives visibility without a new directory, explicit same-path publication follows',async()=>{
  const f=fileFixture();try{
    f.emit(projection(1,[record(1)]));await tick();assert.equal(f.position,'/srv/app');
    f.tree.onManualNavigation();f.control.lifecycle({visible:false});f.control.lifecycle({visible:true});await tick();assert.equal(f.control.snapshot().manual,true);
    f.emit(projection(2,[record(2)]));await tick();assert.equal(f.control.snapshot().manual,false);assert.equal(f.focused,'input');
  }finally{f.dispose();}
});
test('slow old resolution cannot override newer position or manual navigation',async()=>{
  const slow=deferred();let first=true;const f=fileFixture(async(_,v)=>first?(first=false,await slow.promise):({path:v.path,attrs:{kind:'directory'}}));
  try{f.emit(projection(1,[record(1,'a','/old')]));f.emit(projection(2,[record(2,'b','/new')]));await tick();assert.equal(f.position,'/new');f.tree.onManualNavigation();slow.resolve({path:'/old',attrs:{kind:'directory'}});await tick();assert.equal(f.position,'/new');assert.equal(f.focused,'input');}finally{f.dispose();}
});
test('hidden and frozen receivers only act on the latest unhandled directory after activation',async()=>{
  const f=fileFixture();try{f.control.lifecycle({visible:false});f.emit(projection(1,[record(1,'a','/old')]));f.emit(projection(2,[record(2,'b','/new')]));await tick();assert.equal(f.position,'/manual');f.control.lifecycle({visible:true,frozen:true});await tick();assert.equal(f.position,'/manual');f.control.lifecycle({frozen:false});await tick();assert.equal(f.position,'/new');}finally{f.dispose();}
});
test('file targets fail without replacing the previous directory and do not retry on refresh',async()=>{
  let calls=0;const f=fileFixture(async()=>{calls++;return {path:'/file',attrs:{kind:'file'}};});try{f.emit(projection(1,[record(1)]));await tick();f.emit(projection(2,[record(1)]));await tick();assert.equal(f.position,'/manual');assert.equal(calls,1);}finally{f.dispose();}
});

function publisherFixture({get,put}={}){
  const records=new Map(),writes=[],errors=[];let revision=0;
  const client={async request(op,p){if(op==='get'){if(get){const intercepted=await get(p);if(intercepted)return intercepted;}return {record:records.get(p.topic)||{revision:'0',present:false}};}writes.push(p);const r={revision:String(++revision),present:true,value:Object.fromEntries(Object.entries(p.value).sort(([a],[b])=>a.localeCompare(b)))};records.set(p.topic,r);if(put)await put(p);return {revision:r.revision};},dispose(){}};
  const mount={context,data:{openedContext:context,streamId:'pty'}};
  const publisher=createLocationPublication(mount,{client,error:e=>errors.push(e)});
  const observe=(path='/srv/app',offset=1)=>publisher.observe({state:'known',path,pathStyle:'posix',filesystemView:'session-default',terminalId:'pty',outputOffset:String(offset)});
  return {publisher,observe,writes,errors,records};
}
test('terminal ordinary same-directory prompts do not publish again; explicit activation does',async()=>{
  const f=publisherFixture();try{await f.publisher.ready();f.observe();await tick();f.publisher.observe({state:'unknown',activity:'busy',terminalId:'pty',outputOffset:'2'});f.observe('/srv/app',3);await tick();assert.equal(f.writes.filter(p=>p.topic==='workspace.location.v1').length,1);f.publisher.activate();await tick();const writes=f.writes.filter(p=>p.topic==='workspace.location.v1');assert.equal(writes.length,2);assert.notEqual(writes[0].value.publicationId,writes[1].value.publicationId);}finally{f.publisher.dispose();}
});
test('replacement during CAS get discards the stale unsent path',async()=>{
  const delay=deferred();let block=false;const f=publisherFixture({get:async p=>{if(block&&p.topic==='workspace.location.v1'){block=false;return delay.promise;}}});
  try{await f.publisher.ready();block=true;f.observe('/old');await tick();f.observe('/new',2);delay.resolve({record:{present:false,revision:'0'}});await tick();await tick();const paths=f.writes.filter(p=>p.topic==='workspace.location.v1').map(p=>p.value.path);assert.deepEqual(paths,['/new']);}finally{f.publisher.dispose();}
});
test('lost publication reply confirms the original ID instead of publishing a second event',async()=>{
  let lost=true;const f=publisherFixture({put:async p=>{if(lost&&p.topic==='workspace.location.v1'){lost=false;throw new Error('lost reply');}}});try{await f.publisher.ready();f.observe();await tick();assert.equal(f.writes.filter(p=>p.topic==='workspace.location.v1').length,1);assert.equal(f.errors.length,0);}finally{f.publisher.dispose();}
});
test('publication errors are bounded and never block local terminal observations',async()=>{
  let calls=0;const f=publisherFixture({get:async()=>{calls++;throw new Error('offline');}});try{await f.publisher.ready();f.observe();await tick();assert(calls<=8);assert(f.errors.length>0);}finally{f.publisher.dispose();}
});
