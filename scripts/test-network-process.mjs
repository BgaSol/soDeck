import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import {pluginPeer,until} from './docker-process.mjs';
const executable=process.env.SODECK_TEST_NETWORK_EXECUTABLE;
if(!executable)throw new Error('Set SODECK_TEST_NETWORK_EXECUTABLE to the native network plugin executable');
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-network-process-'));
const context={workspaceId:'network-test',serverId:'server',connectionId:'connection',generation:1};
let listener,createCount=0,failQuery=false,allowCleanup=false,stopped=false,holdQuery,enteredQuery,failRelease=true;
const observed=()=>({state:'observed',listenerId:'listener',cleanupComplete:stopped,snapshot:{listenerId:'listener',state:stopped?'closed':'running',cleanupComplete:stopped,address:listener?.address()&&{host:'127.0.0.1',port:listener.address().port}}});
const peer=pluginPeer(executable,async(method,p)=>{
  if(method==='logging.write'||method==='messages.publish')return {};
  if(method==='net.poll')return {items:[]};
  if(method==='net.listener.create'){
    createCount++;assert.equal(createCount,1,'Lost create response must never cause a second listener');listener=net.createServer();await new Promise(resolve=>listener.listen(0,'127.0.0.1',resolve));throw Object.assign(new Error('Create accepted; reply lost'),{code:-32003});
  }
  if(method==='net.listener.lookup'){
    if(holdQuery){enteredQuery?.();await holdQuery;}
    if(failQuery)throw Object.assign(new Error('Query reply lost'),{code:-32003});
    return observed();
  }
  if(method==='net.listener.cancel'){
    if(allowCleanup&&!stopped){await new Promise(resolve=>listener.close(resolve));stopped=true;}
    return observed();
  }
  if(method==='net.listener.release'){if(failRelease)throw Object.assign(new Error('Release reply lost'),{code:-32003});return {released:true};}
  throw new Error('Unexpected call: '+method);
});
const request=(method,viewId,extra={})=>peer.request(method,{viewId,context,...extra});
const event=(data)=>request('ui.event','manager',{data});
const row=()=>request('ui.getView','manager').then(r=>r.data.catalog.rules[0]);
try{
  await peer.request('initialize',{dataDirectory:directory,capabilities:['net.tcp.v1','net.listenerLifecycle.v1','ui.dialog.v1']});
  await request('ui.createView','manager',{viewType:'manager'});
  await request('ui.createView','editor',{viewType:'rule-editor',params:{rule:{id:'rule',serverId:'server',name:'test',revision:0,type:'socks5',bind:{host:'127.0.0.1',port:0}}}});
  const rule={id:'rule',serverId:'server',name:'test',revision:0,type:'socks5',bind:{host:'127.0.0.1',port:0}};
  await request('ui.event','editor',{data:{action:'draft',rule}});
  await request('ui.event','editor',{data:{action:'save'}});
  assert.equal(createCount,0,'Saving a rule must not start a listener');
  await event({action:'start',id:'rule'});
  await until(row,r=>r.run?.snapshot.state==='running');
  const port=listener.address().port;
  failQuery=true;await until(row,r=>Boolean(r.run?.queryError));
  await assert.rejects(event({action:'start',id:'rule'}),/清理|确认/);
  await assert.rejects(event({action:'delete',id:'rule',revision:1}),/清理|确认/);
  assert.equal(createCount,1);assert(listener.listening);
  failQuery=false;
  let releaseQuery;holdQuery=new Promise(resolve=>releaseQuery=resolve);
  const entered=new Promise(resolve=>enteredQuery=resolve);await entered;
  await event({action:'stop',id:'rule'}); // Must return even while the background RPC is blocked.
  releaseQuery();holdQuery=null;
  await until(row,r=>r.run?.stopRequested&&!r.run.queryError);
  await assert.rejects(event({action:'delete',id:'rule',revision:1}),/清理|确认/);
  assert(listener.listening,'Stop ACK must not be treated as complete cleanup');
  allowCleanup=true;await until(row,r=>r.run?.snapshot.cleanupComplete&&r.run.queryError);
  await assert.rejects(event({action:'start',id:'rule'}),/清理|确认/);
  await assert.rejects(event({action:'delete',id:'rule',revision:1}),/清理|确认/);
  failRelease=false;await until(row,r=>r.run?.canStart===true);
  const replacement=net.createServer();await new Promise(resolve=>replacement.listen(port,'127.0.0.1',resolve));await new Promise(resolve=>replacement.close(resolve));
  const blockedWrite=path.join(directory,'network-rules.json.tmp');fs.mkdirSync(blockedWrite);
  await assert.rejects(event({action:'delete',id:'rule',revision:1}),/保存失败/);
  assert.equal((await row()).run.canStart,true,'Failed deletion must preserve the stopped run');assert.equal(createCount,1);
  fs.rmdirSync(blockedWrite);
  await event({action:'delete',id:'rule',revision:1});assert.equal(createCount,1);
  console.log('Network process: query failure, unlocked stop, explicit save/start/delete, and actual port cleanup passed.');
  await peer.close();
}finally{
  if(holdQuery)holdQuery=null;
  if(listener?.listening)await new Promise(resolve=>listener.close(resolve));
  if(peer.child.exitCode===null){peer.child.kill();await new Promise(resolve=>peer.child.once('exit',resolve));}
  assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-network-process-'));fs.rmSync(directory,{recursive:true,force:true});
}
