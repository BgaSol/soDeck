import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { transform } from 'esbuild';
import { runInNewContext } from 'node:vm';

const source = await readFile(new URL('../src/worker.ts',import.meta.url),'utf8');
async function fixture(version='0.2.0', override) {
  const records=[], leases=new Set(), ptys=new Map(), settings=new Map(), views=new Map(); let sequence=0;
  const call = async (method,params) => {
    records.push({method,params});
    const customized=await override?.(method,params); if(customized!==undefined)return customized;
    switch(method) {
      case 'storage.get': return settings.get(params.key) ?? null;
      case 'storage.set': settings.set(params.key,params.value);return {saved:true};
      case 'connections.acquire': {const leaseId=`lease-${++sequence}`;leases.add(leaseId);return {leaseId,connectionId:'shared-connection',generation:1};}
      case 'connections.release': leases.delete(params.leaseId);return {released:true};
      case 'pty.open': {const ptyId=`pty-${++sequence}`;ptys.set(ptyId,{ptyId,state:'running'});return {ptyId,generation:1,streamId:ptyId};}
      case 'pty.close': ptys.delete(params.ptyId);return {closed:true};
      case 'operations.cancel': return {canceled:true};
      case 'resources.list': return {ptys:[...ptys.values()]};
      case 'views.open': {const viewId=`view-${++sequence}`;views.set(viewId,{...params,viewId});return {viewId};}
      case 'views.bindResources': return {bound:true};
      case 'views.update': {const view={...views.get(params.viewId),...params};views.set(params.viewId,view);return view;}
      case 'views.close': views.delete(params.viewId);return {closed:true};
      default:throw new Error(`Unexpected method ${method}`);
    }
  };
  const context={sodeck:{call}};
  const {code}=await transform(source,{loader:'ts',format:'iife',target:'es2022',define:{__TERMINAL_VERSION__:JSON.stringify(version)}});
  runInNewContext(code,context);
  const worker=context.sodeckPlugin; await worker.prepare({});await worker.activate({});
  return {worker,records,leases,ptys,settings,views};
}

test('real worker opens independent PTYs on shared connection and closes only its lease',async()=>{
  const f=await fixture();
  const first=await f.worker.invoke('sessions.create',{serverId:'one',workspaceId:'one'});
  const second=await f.worker.invoke('sessions.create',{serverId:'one',workspaceId:'one'});
  assert.notEqual(first.ptyId,second.ptyId);assert.equal(first.connectionId,second.connectionId);
  await f.worker.invoke('sessions.bind',{sessionId:first.sessionId,viewId:'view-one'});
  assert.equal((await f.worker.invoke('sessions.list',{workspaceId:'one'}))[0].viewId,'view-one');
  await f.worker.invoke('sessions.close',{sessionId:first.sessionId});
  assert.equal(f.ptys.size,1);assert.equal(f.leases.size,1);
  await f.worker.stop();assert.equal(f.ptys.size,0);assert.equal(f.leases.size,0);
});

test('failed PTY creation releases the acquired lease',async()=>{
  const f=await fixture('0.2.0',method=>{if(method==='pty.open')throw {code:'PTY_REFUSED',message:'fixture'};});
  await assert.rejects(f.worker.invoke('sessions.create',{serverId:'one'}),error=>error.code==='PTY_REFUSED');
  assert.equal(f.leases.size,0);assert.equal(f.ptys.size,0);
});

for(const failure of ['views.open','views.bindResources','views.update'])test(`failed ${failure} compensates resources and only the new view`,async()=>{
  let fail=false;
  const f=await fixture('0.2.1',method=>{if(fail&&method===failure)throw {code:'FIXTURE_FAILURE'};});
  const retained=await f.worker.invoke('command.execute',{commandId:'terminal.new',context:{workspaceId:'one',serverId:'one',operationId:'retained'}});
  fail=true;
  await assert.rejects(f.worker.invoke('command.execute',{commandId:'terminal.new',context:{workspaceId:'one',serverId:'one',operationId:'failed'}}),e=>e.code==='FIXTURE_FAILURE');
  assert.equal(f.ptys.size,1);assert.equal(f.leases.size,1);assert.equal(f.views.size,1);assert.ok(f.views.has(retained.viewId));
  assert.equal((await f.worker.invoke('sessions.list',{})).length,1);
  await f.worker.stop();
});

test('late cancellation after view allocation compensates the view and its resources',async()=>{
  let entered,release;const started=new Promise(r=>entered=r),blocked=new Promise(r=>release=r);
  const f=await fixture('0.2.1',async method=>{if(method==='views.bindResources'){entered();await blocked;}});
  const result=f.worker.invoke('command.execute',{commandId:'terminal.new',context:{workspaceId:'one',serverId:'one',operationId:'late'}});
  const rejected=assert.rejects(result,e=>e.code==='CANCELED');
  await started;await f.worker.invoke('lifecycle.cancel',{operationId:'late'});release();await rejected;
  assert.equal(f.views.size,0);assert.equal(f.ptys.size,0);assert.equal(f.leases.size,0);
});

test('canceling one pending acquisition releases its late lease without affecting another session',async()=>{
  let entered, release;
  const started=new Promise(resolve=>{entered=resolve;});
  const blocked=new Promise(resolve=>{release=resolve;});
  const f=await fixture('0.2.0',async(method,params)=>{if(method==='connections.acquire'&&params.operationId==='cancel-one'){entered();await blocked;}});
  const opening=f.worker.invoke('sessions.create',{serverId:'one',operationId:'cancel-one'});
  const rejected=assert.rejects(opening,error=>error.code==='CANCELED');
  await started;
  const other=await f.worker.invoke('sessions.create',{serverId:'one',operationId:'keep-two'});
  await f.worker.invoke('sessions.cancelOpen',{operationId:'cancel-one'});release();await rejected;
  assert.equal(f.ptys.size,1);assert.equal(f.leases.size,1);
  assert.ok(f.ptys.has(other.ptyId));
  assert.equal(f.records.filter(r=>r.method==='pty.open'&&r.params.operationId==='cancel-one').length,0);
  assert.equal((await f.worker.invoke('sessions.list',{})).length,1);
  await f.worker.stop();
});

test('canceling while PTY is pending closes the late PTY and its lease',async()=>{
  let entered, release;
  const started=new Promise(resolve=>{entered=resolve;});
  const blocked=new Promise(resolve=>{release=resolve;});
  const f=await fixture('0.2.0',async(method,params)=>{if(method==='pty.open'&&params.operationId==='cancel-pty'){entered();await blocked;}});
  const opening=f.worker.invoke('sessions.create',{serverId:'one',operationId:'cancel-pty'});
  const rejected=assert.rejects(opening,error=>error.code==='CANCELED');
  await started;await f.worker.invoke('sessions.cancelOpen',{operationId:'cancel-pty'});release();await rejected;
  assert.equal(f.ptys.size,0);assert.equal(f.leases.size,0);
  assert.equal((await f.worker.invoke('sessions.list',{})).length,0);
  assert.equal(f.records.filter(r=>r.method==='pty.close').length,1);
});

test('cancel-before-create reaches core and cleanup failures stay visible',async()=>{
  const canceled=new Set();
  const f=await fixture('0.2.0',(method,params)=>{
    if(method==='operations.cancel')canceled.add(params.operationId);
    if(method==='connections.acquire'&&canceled.has(params.operationId))throw {code:'CANCELED'};
  });
  await f.worker.invoke('sessions.cancelOpen',{operationId:'early'});
  await assert.rejects(f.worker.invoke('sessions.create',{serverId:'one',operationId:'early'}),error=>error.code==='CANCELED');
  assert.equal(f.leases.size,0);assert.equal(f.ptys.size,0);
  const broken=await fixture('0.2.0',method=>{if(method==='pty.open')throw {code:'CANCELED'};if(method==='connections.release')throw {code:'IO_ERROR'};});
  await assert.rejects(broken.worker.invoke('sessions.create',{serverId:'one',operationId:'cleanup'}),error=>error.code==='CLEANUP_REQUIRED'&&error.details.cleanupCode==='IO_ERROR');
});

test('two actual versions have different persisted reset behavior',async()=>{
  const old=await fixture('0.2.0'),newer=await fixture('0.2.1');
  for(const f of [old,newer]) {await f.worker.invoke('settings.set',{fontSize:19});assert.equal(f.settings.get('fontSize'),19);}
  await assert.rejects(old.worker.invoke('settings.reset',{}),error=>error.code==='METHOD_NOT_FOUND');
  assert.equal((await newer.worker.invoke('settings.reset',{})).fontSize,14);assert.equal(newer.settings.get('fontSize'),14);
  await assert.rejects(newer.worker.invoke('settings.set',{fontSize:100}),error=>error.code==='INVALID_PARAMS');
});

test('reconnect preserves logical identity and rejects concurrent close',async()=>{
  let release;let block=false;
  const f=await fixture('0.2.0',async method=>{if(block&&method==='pty.close')await new Promise(resolve=>{release=resolve;});});
  const first=await f.worker.invoke('sessions.create',{serverId:'one'});block=true;
  const reconnect=f.worker.invoke('sessions.reconnect',{sessionId:first.sessionId});
  await new Promise(resolve=>setTimeout(resolve,0));
  await assert.rejects(f.worker.invoke('sessions.close',{sessionId:first.sessionId}),error=>error.code==='BUSY');
  await assert.rejects(f.worker.invoke('sessions.rename',{sessionId:first.sessionId,title:'new'}),error=>error.code==='BUSY');
  block=false;release();const second=await reconnect;
  assert.equal(first.sessionId,second.sessionId);assert.notEqual(first.ptyId,second.ptyId);assert.equal(f.ptys.size,1);assert.equal(f.leases.size,1);
});

test('App restart starts empty and does not reopen remote shells',async()=>{
  const first=await fixture();await first.worker.invoke('sessions.create',{serverId:'one'});
  const restarted=await fixture();assert.equal((await restarted.worker.invoke('sessions.list',{})).length,0);
  assert.equal(restarted.records.filter(r=>r.method==='connections.acquire'||r.method==='pty.open').length,0);
});

test('rename trims a display title and publishes generic metadata without changing resource identity',async()=>{
  const f=await fixture();
  const original=await f.worker.invoke('sessions.create',{serverId:'one'});
  await f.worker.invoke('sessions.bind',{sessionId:original.sessionId,viewId:'view-one'});
  const calls=f.records.length;
  const renamed=await f.worker.invoke('sessions.rename',{sessionId:original.sessionId,title:'  数据库日志 🧪  '});
  assert.equal(renamed.title,'数据库日志 🧪');
  for(const key of ['sessionId','serverId','workspaceId','leaseId','connectionId','ptyId','generation','streamId'])assert.equal(renamed[key],original[key]);
  assert.equal(renamed.viewId,'view-one');assert.equal(f.records.length,calls+1);assert.equal(f.records.at(-1).method,'views.update');assert.equal(f.records.at(-1).params.title,'数据库日志 🧪');
  for(const title of ['', '   ', 'a'.repeat(81), 'line\nbreak', 'tab\tname', 'bad\u0085name', null, 7])await assert.rejects(f.worker.invoke('sessions.rename',{sessionId:original.sessionId,title}),error=>error.code==='INVALID_PARAMS');
  assert.equal((await f.worker.invoke('sessions.get',{sessionId:original.sessionId})).title,'数据库日志 🧪');
  assert.equal((await f.worker.invoke('sessions.rename',{sessionId:original.sessionId,title:'🧪'.repeat(80)})).title,'🧪'.repeat(80));
  await assert.rejects(f.worker.invoke('sessions.rename',{sessionId:'missing',title:'new'}),error=>error.code==='SESSION_ENDED');
  await f.worker.stop();
});

 test('restored placeholder renames and reconnects into its original view without resurrecting handles',async()=>{
 const f=await fixture('0.3.0');const metadata=await f.worker.invoke('lifecycle.restore',{viewId:'saved-view',instanceKey:'saved-session',viewType:'terminal.session',workspaceId:'one',serverId:'one',title:'saved title'});
 assert.deepEqual([...metadata.actions],['terminal.rename','terminal.reconnect']);assert.equal(f.ptys.size,0);
 await f.worker.invoke('command.execute',{commandId:'terminal.rename',context:{viewId:'saved-view'},args:{title:'new title'}});
 const opened=await f.worker.invoke('command.execute',{commandId:'terminal.reconnect',context:{viewId:'saved-view',operationId:'restore-reconnect'}});
 assert.equal(opened.viewId,'saved-view');assert.equal(opened.sessionId,'saved-session');assert.equal(opened.title,'new title');assert.equal(f.ptys.size,1);assert.equal(f.records.filter(r=>r.method==='views.open').length,0);await f.worker.stop();
 });
