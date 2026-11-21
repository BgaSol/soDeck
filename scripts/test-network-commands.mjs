import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import {pluginPeer} from './plugin-peer.mjs';
import {until} from './docker-process.mjs';

export async function verifyNetworkCommands(executable){
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-network-commands-'));
  const context={workspaceId:'commands',serverId:'server',connectionId:'connected',generation:1};
  const records=new Map(),listeners=new Map(),probes=new Set();let creates=0,lostPut=false,loseCreate=true,failLookup=false;
  const key=p=>JSON.stringify([p.viewId,p.expectedContext,p.topic,p.key]);
  const snapshot=entry=>({state:'observed',listenerId:entry.id,cleanupComplete:entry.closed,snapshot:{state:entry.closed?'closed':'running',cleanupComplete:entry.closed,address:entry.address,clients:[...probes].map(probeId=>({id:'probe-client',probeId,target:{state:'relaying'}}))}});
  const peer=pluginPeer(executable,async(method,p)=>{
    if(['logging.write','messages.publish','net.close'].includes(method))return {};
    if(method==='state.put'){
      const old=records.get(key(p));if((old?.revision||'0')!==p.expectedRevision)throw new Error('revisionConflict');
      const value={revision:String(BigInt(old?.revision||'0')+1n),value:p.value,present:true};records.set(key(p),value);
      if(!lostPut){lostPut=true;throw new Error('put committed but reply lost');}return {revision:value.revision};
    }
    if(method==='state.get')return {record:records.get(key(p))||{revision:'0',present:false}};
    if(method==='state.remove'){const old=records.get(key(p));if(old?.revision!==p.expectedRevision)throw new Error('revisionConflict');records.delete(key(p));return {};}
    if(method==='net.poll')return {items:[]};
    if(method==='net.listener.create'){
      creates++;const socket=net.createServer();await new Promise(resolve=>socket.listen(p.bind.port,p.bind.host,resolve));
      const entry={id:'listener-'+creates,operation:p.operationId,socket,closed:false,address:{host:p.bind.host,port:socket.address().port}};listeners.set(p.operationId,entry);
      if(loseCreate){loseCreate=false;throw new Error('create committed but reply lost');}return snapshot(entry);
    }
    if(method==='net.listener.lookup'){if(failLookup)throw new Error('lookup lost');return snapshot(listeners.get(p.operationId));}
    if(method==='net.listener.cancel'){const entry=listeners.get(p.operationId);if(entry&&!entry.closed){await new Promise(resolve=>entry.socket.close(resolve));entry.closed=true;}return snapshot(entry);}
    if(method==='net.listener.release')return {released:true};
    if(method==='net.listener.probe'){probes.add(p.operationId);return {};}
    if(method==='net.listener.snapshot')return snapshot([...listeners.values()].find(e=>e.id===p.listenerId)).snapshot;
    if(method==='ui.openView')return {viewId:'manager'};
    throw new Error('Unexpected network command RPC: '+method);
  });
  const request=(method,viewId,extra={})=>peer.request(method,{viewId,context,...extra});
  const event=data=>request('ui.event','manager',{data});
  let command=0;
  const ensure=(target={host:'127.0.0.1',port:8080},selectedRule=null)=>peer.request('commands.handle',{commandId:'ensure-'+(++command),name:'network.tcpForward.ensure.v1',context,payload:{version:1,target,suggestedName:'Docker · example',selectedRule}});
  const catalog=()=>request('ui.getView','manager').then(r=>r.data.catalog);
  const row=async id=>(await catalog()).rules.find(row=>row.rule.id===id);
  const save=async rule=>{await request('ui.createView','editor',{viewType:'rule-editor',params:{rule}});await request('ui.event','editor',{data:{action:'draft',rule}});await request('ui.event','editor',{data:{action:'save'}});await request('ui.disposeView','editor');};
  try{
    const initialized=await peer.request('initialize',{dataDirectory:directory,capabilities:['net.tcp.v1','net.listenerLifecycle.v1','ui.dialog.v1','commands.v1','sharedState.v1','ui.background.v1','ui.openView.v1']});
    assert.deepEqual(initialized.commandHandlers,['network.tcpForward.ensure.v1','network.rule.reveal.v1']);
    await assert.rejects(ensure(),/managerNotReady/);
    await request('ui.createView','manager',{viewType:'manager'});
    const first=await ensure();assert.equal(first.outcome,'accepted');assert.equal(first.disposition,'created');assert.equal(first.viewId,undefined);
    const attempts=await Promise.allSettled([ensure(),ensure()]);assert(attempts.every(r=>r.status==='fulfilled'||/changeBusy/.test(r.reason.message)));
    await until(()=>row(first.ruleId),r=>r?.run?.snapshot.state==='running'&&!r.run.queryError);
    assert.equal(creates,1);assert.equal((await catalog()).total,1);
    const reused=await ensure();assert.equal(reused.disposition,'reused');assert.equal(reused.ruleId,first.ruleId);
    await until(async()=>[...records.values()],values=>values.some(r=>r.value.rule?.id===first.ruleId&&r.value.run?.snapshot.state==='running')&&values.some(r=>r.value.publishState==='synced'));
    assert(lostPut,'Publisher must recover a committed put whose reply was lost');
    await until(()=>row(first.ruleId),r=>r?.run&&[...records.values()].some(v=>v.value.rule?.id===first.ruleId&&v.value.check?.ok));
    failLookup=true;await until(()=>row(first.ruleId),r=>Boolean(r.run?.queryError));await assert.rejects(ensure(),/tunnelStateUnknown/);assert.equal(creates,1);failLookup=false;
    await until(()=>row(first.ruleId),r=>!r.run?.queryError);
    const original=(await row(first.ruleId)).rule;
    await save({...original,name:'Renamed'});assert.equal((await ensure()).disposition,'reused','Name changes do not alter the active endpoint');
    const renamed=(await row(first.ruleId)).rule;
    await save({...renamed,target:{host:'127.0.0.1',port:9090}});await assert.rejects(ensure(),/ruleConflict/);
    await event({action:'stop',id:first.ruleId});await until(()=>row(first.ruleId),r=>r.run?.canStart);
    const changed=await ensure({host:'127.0.0.1',port:9090});assert.equal(changed.disposition,'started');await until(()=>row(first.ruleId),r=>r.run?.snapshot.state==='running');
    await request('ui.beforeClose','manager');await event({action:'start',id:'missing'}).catch(()=>{});await ensure({host:'127.0.0.1',port:9090});
    await request('ui.disposeView','manager');
    await until(async()=>[...listeners.values()].every(e=>e.closed),Boolean);
    await request('ui.createView','manager',{viewType:'manager'});
    await until(catalog,c=>c.rules.every(r=>!r.run||r.run.canStart));
    const second={...renamed,id:'manual-second',revision:0,target:{host:'127.0.0.1',port:9090}};await save(second);
    const selection=await ensure({host:'127.0.0.1',port:9090});assert.equal(selection.outcome,'selectionRequired');assert.equal(selection.candidateCount,2);
    const selected=await ensure({host:'127.0.0.1',port:9090},{id:'manual-second',revision:1});assert.equal(selected.ruleId,'manual-second');
    await assert.rejects(ensure({host:'127.0.0.1',port:9090},{id:'manual-second',revision:0}),/ruleConflict/);
    const revealed=await peer.request('commands.handle',{name:'network.rule.reveal.v1',commandId:'reveal',context,payload:{version:1,ruleId:'manual-second'}});assert.equal(revealed.viewId,'manager');assert.equal((await request('ui.getView','manager')).data.navigation.selected,'manual-second');
    await assert.rejects(ensure({host:'localhost',port:80}),/invalidParams/);
    const count=(await catalog()).total;
    const blocked=path.join(directory,'network-rules.json.tmp');fs.mkdirSync(blocked);await assert.rejects(ensure({host:'127.0.0.1',port:7070}),/storageUnavailable/);fs.rmdirSync(blocked);assert.equal((await catalog()).total,count);
    await request('ui.disposeView','manager');await until(async()=>[...listeners.values()].every(e=>e.closed),Boolean);
    await peer.close();assert.equal(peer.child.exitCode,0);
    console.log('Network commands: background ownership, target reuse/concurrency, lost create/put, configuration revisions, selection, reveal, checks, storage failure and actual listener cleanup passed.');
  }finally{
    if(peer.child.exitCode===null)await peer.kill();
    for(const entry of listeners.values())if(entry.socket.listening)await new Promise(resolve=>entry.socket.close(resolve));
    assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-network-commands-'));fs.rmSync(directory,{recursive:true,force:true});
  }
}
