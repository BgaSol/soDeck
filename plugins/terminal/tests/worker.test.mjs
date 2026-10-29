import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { build } from 'esbuild';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

const source = await readFile(new URL('../src/worker.ts',import.meta.url),'utf8');
async function fixture(version='0.4.0', override) {
  const records=[], leases=new Set(), ptys=new Map(), settings=new Map(), views=new Map(); let sequence=0;
  const call = async (method,params) => {
    records.push({method,params});
    const customized=await override?.(method,params); if(customized!==undefined)return customized;
    switch(method) {
      case 'storage.preferences.read':return {pageId:'plugin:sodeck.terminal:general',providerEpoch:'epoch',storageGeneration:'generation',schemaVersion:1,settingsRevision:String(sequence),values:[{key:'fontSize',value:settings.get('fontSize')??14},{key:'shellIntegration',value:settings.get('shellIntegration')??true}],syncState:'current',warnings:[]};
      case 'storage.preferences.commit':for(const e of params.changes)settings.set(e.key,e.value);sequence++;return {operationId:params.commitTicket,persistedRevision:String(sequence),persistence:'saved',synchronization:'pending'};
      case 'settings.provider.applied':return {accepted:true};
      case 'storage.get': return settings.get(params.key) ?? null;
      case 'storage.set': settings.set(params.key,params.value);return {saved:true};
      case 'connections.acquire': {const leaseId=`lease-${++sequence}`;leases.add(leaseId);return {leaseId,connectionId:'shared-connection',generation:1};}
      case 'connections.release': leases.delete(params.leaseId);return {released:true};
      case 'exec.start': return {execId:'probe-'+params.operationId};
      case 'exec.read': {const bytes=Buffer.from('SODECK_SHELL:5.2.37(1)-release\nLinux\n');return {frames:[{sequence:1,stream:'stdout',bytesBase64:bytes.toString('base64'),length:bytes.length}],lastSequence:1,acknowledged:0,state:'completed',exitCode:0,reason:null,truncated:false};}
      case 'exec.ack': return {acknowledged:params.sequence};
      case 'exec.close': return {closed:true};
      case 'exec.cancel': return {canceled:true};
      case 'remote.operations.forget': return {forgotten:true};
      case 'pty.openCommand':
      case 'pty.open': {const ptyId=`pty-${++sequence}`;ptys.set(ptyId,{ptyId,state:'running'});return {ptyId,generation:1,streamId:ptyId,connectionId:'shared-connection'};}
      case 'directories.register':return {sourceId:'source-'+params.viewId,sourceGeneration:'generation'};
      case 'directories.report':return {accepted:true,sourceRevision:'1'};
      case 'directories.unregister':return {accepted:true};
      case 'pty.close': ptys.delete(params.ptyId);return {closed:true};
      case 'operations.cancel': return {canceled:true};
      case 'resources.list': return {ptys:[...ptys.values()]};
      case 'views.open': {const viewId=`view-${++sequence}`;views.set(viewId,{...params,viewId});return {viewId};}
      case 'views.presentation':return {scope:{workspaceId:'one',workspaceGeneration:'generation'}};
      case 'views.bindResources': return {bound:true};
      case 'views.update': {const view={...views.get(params.viewId),...params};views.set(params.viewId,view);return view;}
      case 'views.close': views.delete(params.viewId);return {closed:true};
      default:throw new Error(`Unexpected method ${method}`);
    }
  };
  const context={sodeck:{call,onNotification:()=>()=>{},clock:{monotonicMs:()=>performance.now()}},setTimeout,clearTimeout};
  const bundle=await build({entryPoints:[fileURLToPath(new URL('../src/worker.ts',import.meta.url))],bundle:true,write:false,format:'iife',platform:'neutral',target:'es2022',define:{__TERMINAL_VERSION__:JSON.stringify(version)}});
  const code=bundle.outputFiles[0].text;
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
  const f=await fixture('0.2.0',method=>{if(method==='pty.openCommand')throw {code:'PTY_REFUSED',message:'fixture'};});
  await assert.rejects(f.worker.invoke('sessions.create',{serverId:'one'}),error=>error.code==='PTY_REFUSED');
  assert.equal(f.leases.size,0);assert.equal(f.ptys.size,0);
});

for(const sample of ['','SODECK_SHELL:\nLinux\n','SODECK_SHELL:5.2\nMINGW64_NT-10.0\n','SODECK_SHELL:5.2\nDarwin\n','noise\nSODECK_SHELL:5.2\nLinux\n'])test(`unsupported probe ${JSON.stringify(sample)} opens only a normal shell`,async()=>{
  const f=await fixture('0.13.1',method=>{if(method==='exec.read'){const bytes=Buffer.from(sample);return {frames:bytes.length?[{sequence:1,stream:'stdout',bytesBase64:bytes.toString('base64'),length:bytes.length}]:[],lastSequence:bytes.length?1:0,acknowledged:0,state:'completed',exitCode:sample?0:1,reason:null,truncated:false};}});
  const session=await f.worker.invoke('sessions.create',{serverId:'one'});
  assert.equal(session.integrationAvailable,false);assert.equal(session.status,'running');
  assert.equal(f.records.filter(r=>r.method==='pty.open').length,1);assert.ok(!f.records.some(r=>r.method==='pty.openCommand'));
  assert.equal((await f.worker.invoke('settings.get')).shellIntegration,true);
  assert.ok(f.records.some(r=>r.method==='exec.close'));assert.ok(f.records.some(r=>r.method==='remote.operations.forget'));
  await f.worker.stop();assert.equal(f.leases.size,0);
});

test('probe refusal does not fail the normal terminal',async()=>{
  const f=await fixture('0.13.1',method=>{if(method==='exec.start')throw {code:'EXEC_REFUSED'};if(method==='remote.operations.get')return {state:'failed',result:null,error:{code:'EXEC_REFUSED',message:'fixture'}};});
  const s=await f.worker.invoke('sessions.create',{serverId:'one'});assert.equal(s.integrationAvailable,false);assert.equal(s.status,'running');assert.ok(!f.records.some(r=>r.method==='pty.openCommand'));await f.worker.stop();
});

test('cancel during probe closes late exec and never creates a PTY',async()=>{
  let enter,release;const began=new Promise(r=>enter=r),blocked=new Promise(r=>release=r);
  const f=await fixture('0.13.1',async method=>{if(method==='exec.start'){enter();await blocked;return {execId:'late-probe'};}});
  const opening=f.worker.invoke('sessions.create',{serverId:'one',operationId:'probe-cancel'});
  const rejected=assert.rejects(opening,e=>e.code==='CANCELED');await began;
  await f.worker.invoke('sessions.cancelOpen',{operationId:'probe-cancel'});release();await rejected;
  assert.ok(!f.records.some(r=>r.method==='pty.open'||r.method==='pty.openCommand'));assert.ok(f.records.some(r=>r.method==='exec.close'&&r.params.execId==='late-probe'));
  assert.equal(f.leases.size,0);await f.worker.stop();
});

test('connection failure publishes its exact diagnostic and clears it after a new connection',async()=>{
  let fail=true;const ref={bootId:'boot',attemptId:'attempt',attemptGeneration:3};
  const f=await fixture('0.13.0',method=>{if(fail&&method==='connections.acquire')throw {code:'PROXY_DISCONNECTED',message:'private raw cause',details:{diagnosticRef:ref,stage:'proxyNegotiate',stepIndex:2}};});
  await assert.rejects(f.worker.invoke('command.execute',{commandId:'terminal.new',context:{workspaceId:'one',serverId:'one',operationId:'failure'}}),e=>e.code==='PROXY_DISCONNECTED');
  const session=(await f.worker.invoke('sessions.list',{}))[0];assert.equal(JSON.stringify(session.diagnosticRef),JSON.stringify(ref));assert.match(session.error,/第 3 站/);assert.doesNotMatch(session.error,/private raw/);
  fail=false;const next=await f.worker.invoke('sessions.create',{serverId:'one',workspaceId:'one'});assert.equal(next.diagnosticRef,undefined);await f.worker.stop();
});

for(const failure of ['views.open','views.bindResources','views.update'])test(`failed ${failure} releases resources and retains an allocated error page`,async()=>{
  let fail=false;
  const f=await fixture('0.2.1',method=>{if(fail&&method===failure)throw {code:'FIXTURE_FAILURE'};});
  const retained=await f.worker.invoke('command.execute',{commandId:'terminal.new',context:{workspaceId:'one',serverId:'one',operationId:'retained'}});
  fail=true;
  await assert.rejects(f.worker.invoke('command.execute',{commandId:'terminal.new',context:{workspaceId:'one',serverId:'one',operationId:'failed'}}),e=>e.code==='FIXTURE_FAILURE');
  assert.equal(f.ptys.size,1);assert.equal(f.leases.size,1);assert.equal(f.views.size,failure==='views.open'?1:2);assert.ok(f.views.has(retained.viewId));
  const list=await f.worker.invoke('sessions.list',{});
  assert.equal(list.length,failure==='views.open'?1:2);
  if(failure!=='views.open')assert.equal(list.find(s=>s.viewId!==retained.viewId).status,'failed');
  await f.worker.stop();
});

test('late cancellation retains the page and releases all resources',async()=>{
  let entered,release;const started=new Promise(r=>entered=r),blocked=new Promise(r=>release=r);
  const f=await fixture('0.2.1',async method=>{if(method==='views.bindResources'){entered();await blocked;}});
  const result=f.worker.invoke('command.execute',{commandId:'terminal.new',context:{workspaceId:'one',serverId:'one',operationId:'late'}});
  const rejected=assert.rejects(result,e=>e.code==='CANCELED');
  await started;await f.worker.invoke('lifecycle.cancel',{operationId:'late'});release();await rejected;
  assert.equal(f.views.size,1);assert.equal(f.ptys.size,0);assert.equal(f.leases.size,0);
  assert.equal((await f.worker.invoke("sessions.list",{}))[0].status,"canceled");
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
  assert.equal(f.records.filter(r=>r.method==='pty.openCommand'&&r.params.operationId==='cancel-one').length,0);
  const list=await f.worker.invoke('sessions.list',{});assert.equal(list.length,2);assert.equal(list.filter(s=>s.status==='canceled').length,1);
  await f.worker.stop();
});

test('canceling while PTY is pending closes the late PTY and its lease',async()=>{
  let entered, release;
  const started=new Promise(resolve=>{entered=resolve;});
  const blocked=new Promise(resolve=>{release=resolve;});
  const f=await fixture('0.2.0',async(method,params)=>{if(method==='pty.openCommand'&&params.operationId==='cancel-pty'){entered();await blocked;}});
  const opening=f.worker.invoke('sessions.create',{serverId:'one',operationId:'cancel-pty'});
  const rejected=assert.rejects(opening,error=>error.code==='CANCELED');
  await started;await f.worker.invoke('sessions.cancelOpen',{operationId:'cancel-pty'});release();await rejected;
  assert.equal(f.ptys.size,0);assert.equal(f.leases.size,0);
  assert.equal((await f.worker.invoke('sessions.list',{}))[0].status,'canceled');
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
  const broken=await fixture('0.2.0',method=>{if(method==='pty.openCommand')throw {code:'CANCELED'};if(method==='connections.release')throw {code:'IO_ERROR'};});
  await assert.rejects(broken.worker.invoke('sessions.create',{serverId:'one',operationId:'cleanup'}),error=>error.code==='CLEANUP_REQUIRED'&&error.details.cleanupCode==='IO_ERROR');
});

test('provider validates and commits a group; legacy direct writes are unavailable',async()=>{
  const f=await fixture('0.11.0');
  const invalid=await f.worker.invoke('settings.provider.validate',{groupId:'general',changes:[{key:'fontSize',value:100}]});assert.equal(invalid.errors.length,1);
  const result=await f.worker.invoke('settings.provider.commit',{groupId:'general',ticket:'ticket',changes:[{key:'fontSize',value:19},{key:'shellIntegration',value:false}]});
  assert.equal(f.settings.get('fontSize'),19);assert.equal(f.settings.get('shellIntegration'),false);assert.equal(result.values[0].value,19);
  assert.equal(f.records.filter(r=>r.method==='storage.preferences.commit').length,1);assert.equal(f.records.filter(r=>r.method==='storage.set').length,0);
  await assert.rejects(f.worker.invoke('settings.set',{fontSize:18}),e=>e.code==='METHOD_NOT_FOUND');await f.worker.stop();
});
test('an opening session retains its captured integration preference',async()=>{
  let enter,release;const started=new Promise(r=>enter=r),blocked=new Promise(r=>release=r);let pause=true;
  const f=await fixture('0.11.0',async method=>{if(method==='connections.acquire'&&pause){enter();await blocked;}});
  const opening=f.worker.invoke('sessions.create',{serverId:'one'});await started;
  await f.worker.invoke('settings.provider.commit',{groupId:'general',ticket:'ticket',changes:[{key:'shellIntegration',value:false}]});pause=false;release();await opening;
  await f.worker.invoke('sessions.create',{serverId:'two'});assert.equal(f.records.filter(r=>r.method==='pty.openCommand').length,1);assert.equal(f.records.filter(r=>r.method==='pty.open').length,1);await f.worker.stop();
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
  assert.equal(restarted.records.filter(r=>r.method==='connections.acquire'||r.method==='pty.openCommand').length,0);
});

test('rename trims a display title and publishes generic metadata without changing resource identity',async()=>{
  const f=await fixture();
  const original=await f.worker.invoke('sessions.create',{serverId:'one'});
  await f.worker.invoke('sessions.bind',{sessionId:original.sessionId,viewId:'view-one'});
  const business=()=>f.records.filter(r=>!['events.','traces.','observability.','metrics.','diagnostics.'].some(prefix=>r.method.startsWith(prefix)));
  const calls=business().length;
  const renamed=await f.worker.invoke('sessions.rename',{sessionId:original.sessionId,title:'  数据库日志 🧪  '});
  assert.equal(renamed.title,'数据库日志 🧪');
  for(const key of ['sessionId','serverId','workspaceId','leaseId','connectionId','ptyId','generation','streamId'])assert.equal(renamed[key],original[key]);
  assert.equal(renamed.viewId,'view-one');assert.equal(business().length,calls+1);assert.equal(business().at(-1).method,'views.update');assert.equal(business().at(-1).params.title,'数据库日志 🧪');
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


test('initial terminal page precedes acquisition; authentication retry reuses its identity',async()=>{
  let auth=true;const f=await fixture('0.4.0',method=>{if(method==='connections.acquire'&&auth)throw {code:'AUTH_REQUIRED',message:'需要密码'};});
  await assert.rejects(f.worker.invoke('command.execute',{commandId:'terminal.new',context:{serverId:'one',workspaceId:'one',operationId:'initial'}}),e=>e.code==='AUTH_REQUIRED');
  const [pending]=await f.worker.invoke('sessions.list',{});assert.equal(pending.status,'waitingAuth');assert.equal(f.views.size,1);
  assert.ok(f.records.findIndex(r=>r.method==='views.open')<f.records.findIndex(r=>r.method==='connections.acquire'));
  auth=false;
  const ready=await f.worker.invoke('command.execute',{commandId:'terminal.new',context:{serverId:'one',workspaceId:'one',operationId:'retry',viewId:pending.viewId}});
  assert.equal(ready.sessionId,pending.sessionId);assert.equal(ready.viewId,pending.viewId);assert.equal(f.views.size,1);assert.equal(f.ptys.size,1);
  await f.worker.stop();
});

test('closing during PTY acquisition cannot resurrect a logical terminal or leak late resources',async()=>{
  let release,entered;const blocked=new Promise(r=>release=r),started=new Promise(r=>entered=r);
  const f=await fixture('0.4.0',async method=>{if(method==='pty.openCommand'){entered();await blocked;}});
  const opening=f.worker.invoke('sessions.create',{serverId:'one',operationId:'close'});
  const rejected=assert.rejects(opening,e=>e.code==='CANCELED');await started;
  const [pending]=await f.worker.invoke('sessions.list',{});
  await f.worker.invoke('lifecycle.close',{viewId:pending.viewId});f.views.delete(pending.viewId);release();await rejected;
  assert.equal((await f.worker.invoke('sessions.list',{})).length,0);assert.equal(f.ptys.size,0);assert.equal(f.leases.size,0);assert.equal(f.views.size,0);
});

test('disabled integration reports unsupported while keeping the ordinary PTY usable',async()=>{
  const f=await fixture();
  try{
    await f.worker.invoke('settings.provider.commit',{groupId:'general',ticket:'disabled-ticket',changes:[{key:'shellIntegration',value:false}]});
    const session=await f.worker.invoke('sessions.create',{serverId:'one'});
    for(let i=0;i<50&&!f.records.some(r=>r.method==='directories.report'&&r.params.status==='unsupported');i++)await new Promise(r=>setTimeout(r,2));
    assert.equal(session.status,'running');assert.equal(f.ptys.size,1);
    assert.ok(f.records.some(r=>r.method==='directories.report'&&r.params.status==='unsupported'));
    assert.ok(f.records.some(r=>r.method==='pty.open'));
    assert.ok(!f.records.some(r=>r.method==='pty.openCommand'||r.method==='pty.observe.read'));
  }finally{await f.worker.stop();}
});

test('retained failed pages count toward capacity and retry reuses the reserved slot',async()=>{
  let fail=true;const f=await fixture('0.4.0',method=>{if(method==='connections.acquire'&&fail)throw {code:'CONNECTION_FAILED',message:'fixture'};});
  for(let i=0;i<8;i++)await assert.rejects(f.worker.invoke('sessions.create',{serverId:'one',operationId:'open-'+i}),e=>e.code==='CONNECTION_FAILED');
  assert.equal(f.views.size,8);await assert.rejects(f.worker.invoke('sessions.create',{serverId:'one'}),e=>e.code==='CAPACITY');
  const [first]=await f.worker.invoke('sessions.list',{});fail=false;
  const ready=await f.worker.invoke('command.execute',{commandId:'terminal.reconnect',context:{viewId:first.viewId,operationId:'retry'}});
  assert.equal(ready.viewId,first.viewId);assert.equal(f.views.size,8);assert.equal(f.ptys.size,1);await f.worker.stop();
});
