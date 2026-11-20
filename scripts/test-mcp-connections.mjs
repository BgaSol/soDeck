import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';
import {pluginPeer} from './plugin-peer.mjs';
import {testPluginDirectory} from './test-plugin-directory.mjs';

const root=testPluginDirectory('mcp'),manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json')));
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-mcp-open-'));
const socket=net.createServer();await new Promise(r=>socket.listen(0,'127.0.0.1',r));const port=socket.address().port;await new Promise(r=>socket.close(r));
const servers=[{id:'a',name:'187',host:'10.0.0.187'},{id:'b',name:'backup',host:'10.0.1.187'},{id:'c',name:'third',host:'node.example.com'},{id:'late',name:'late',host:'late.example.com'}];
let revision='1',sequence=0,created=0,cancelCalls=0,lateRelease;
const requests=new Map(),sessions=new Map(),backgrounds=new Map();
const descriptor=s=>({name:s.name,protocol:'ssh',host:s.host,port:22,endpoint:s.host+':22',username:'ops'});
const context=(s,generation=1)=>({workspaceId:'workspace-'+s.id,serverId:'server-'+s.id,connectionId:'connection-'+s.id+'-'+generation,generation});
function reply(record){
  if(record.cancelled)return {receipt:{state:'cancelled'}};
  const s=servers.find(s=>'server-'+s.id===record.input.serverId),session=sessions.get(s.id);
  if(!record.context&&session?.accepted)record.context={...session.context};
  if(record.context){
    const same=session&&session.context.connectionId===record.context.connectionId;
    return {receipt:{state:'accepted',context:record.context},current:same?{context:record.context,state:session.disconnected?'disconnected':'connected',closing:false,capabilities:session.disconnected?[]:['exec','files'],target:descriptor(s),targetSource:session.disconnected?'lastConnected':'current'}:{error:{code:'staleContext',message:'old context'}},background:backgrounds.get(s.id)??{state:'waiting',viewId:null}};
  }
  return {receipt:{state:'attached',attemptId:session.id},view:{stage:session.question?'credentials':'tcp',question:session.question?{kind:'credentials',name:s.name,endpoint:s.host+':22',nodeIndex:0,message:'answer in SoDeck'}:null,windowId:'main'}};
}
const peer=pluginPeer(path.join(root,manifest.entry),async(method,p)=>{
  if(method==='settings.get')return {values:{port}};
  if(method==='servers.list'){
    const offset=p.cursor?Number(p.cursor.split(':')[1]):0;if(p.cursor&&!p.cursor.startsWith(revision+':'))throw Object.assign(new Error('catalog changed'),{data:{code:'catalogChanged'}});
    const selected=p.serverId?servers.filter(s=>'server-'+s.id===p.serverId):servers,items=selected.slice(offset,offset+2);
    return {catalogRevision:revision,servers:items.map(s=>{const session=sessions.get(s.id);return {serverId:'server-'+s.id,profileToken:'token-'+s.id,...descriptor(s),connection:session?{state:session.accepted?'connected':'connecting',matchesSavedTarget:true}:null,context:session?.accepted?session.context:null,current:session?.accepted?reply({input:{serverId:'server-'+s.id},context:session.context}).current:null,background:backgrounds.get(s.id)??null};}),hasMore:offset+items.length<selected.length,nextCursor:offset+items.length<selected.length?revision+':'+(offset+items.length):null};
  }
  if(method==='connections.open'){
    if(p.serverId==='server-late'&&!requests.has(p.requestId))await new Promise(r=>lateRelease=r);
    if(requests.has(p.requestId))return reply(requests.get(p.requestId));
    const s=servers.find(s=>'server-'+s.id===p.serverId);assert(s);assert.equal(p.expectedProfileToken,'token-'+s.id);
    if(!sessions.has(s.id)){created++;sessions.set(s.id,{id:'attempt-'+s.id,accepted:false,question:false,context:context(s)});}
    const record={input:p};requests.set(p.requestId,record);return reply(record);
  }
  if(method==='connections.get'){
    const record=requests.get(p.requestId);if(!record)throw Object.assign(new Error('not yet registered'),{data:{code:'connectionRequestNotFound'}});return reply(record);
  }
  if(method==='connections.cancel'){
    cancelCalls++;let record=requests.get(p.requestId);if(record?.context)return reply(record);
    if(!record){record={cancelled:true};requests.set(p.requestId,record);}else record.cancelled=true;return {receipt:{state:'cancelled'}};
  }
  throw new Error('Unexpected RPC '+method);
});
let token;
async function call(method,params){const r=await fetch('http://127.0.0.1:'+port+'/mcp',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json, text/event-stream'},body:JSON.stringify({jsonrpc:'2.0',id:++sequence,method,params})});assert.equal(r.status,200,await r.clone().text());return (await r.json()).result;}
async function tool(name,args={}){const r=await call('tools/call',{name,arguments:args});assert.equal(r.content.length,1);assert(!('structuredContent' in r));assert(Buffer.byteLength(JSON.stringify(r))<=256*1024);return {value:JSON.parse(r.content[0].text),error:r.isError};}
async function until(read,check){const end=Date.now()+12000;let result;do{result=await read();if(check(result))return result;await delay(20);}while(Date.now()<end);throw new Error('Did not settle: '+JSON.stringify(result));}
const open=(id,requestId,waitMs=0)=>tool('sodeck_connection_open',{serverId:'server-'+id,expectedProfileToken:'token-'+id,requestId,waitMs});
try{
  await peer.request('initialize',{capabilities:['session.target.v2','session.process.v1','files.pluginData.v1','files.operations.v2','files.copy.v1','ui.background.v1','settings.v1','servers.read.v1','connections.v1'],dataDirectory:directory});
  token=fs.readFileSync(path.join(directory,'access-token'),'utf8');
  await until(()=>call('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'opening-regression',version:'1'}}).catch(()=>null),Boolean);
  const first=(await tool('sodeck_servers_list',{query:'187',limit:1})).value;
  assert.equal(first.resolution,'ambiguous');assert.equal(first.matchCount,2);assert.equal(first.servers.length,1);assert(first.hasMore);assert.equal(created,0,'discovery never connects');
  const second=(await tool('sodeck_servers_list',{query:'187',limit:1,cursor:first.nextCursor})).value;assert.equal(second.matchCount,2);assert.equal(second.servers[0].serverId,'server-b');assert(!second.hasMore);
  revision='2';assert.equal((await tool('sodeck_servers_list',{query:'187',limit:1,cursor:first.nextCursor})).value.error.code,'catalogChanged');
  const a=(await open('a','open-a')).value,b=(await open('a','open-b')).value;
  assert.equal(a.connection.state,'pending');await until(async()=>created,n=>n===1);
  await tool('sodeck_connection_cancel',{connectionRequestId:b.connection.connectionRequestId});await until(()=>tool('sodeck_connection_get',{connectionRequestId:b.connection.connectionRequestId}),r=>r.value.connection.state==='cancelled');
  sessions.get('a').question=true;await peer.request('connections.changed',{serverId:'server-a'});
  const needs=await until(()=>tool('sodeck_connection_get',{connectionRequestId:a.connection.connectionRequestId}),r=>r.value.connection.state==='needsUser');assert.equal(needs.value.connection.interaction.kind,'credentials');
  sessions.get('a').question=false;sessions.get('a').accepted=true;backgrounds.set('a',{state:'running',viewId:'view-a'});await peer.request('connections.changed',{serverId:'server-a'});
  const preparing=await until(()=>tool('sodeck_connection_get',{connectionRequestId:a.connection.connectionRequestId}),r=>r.value.connection.stage==='preparingTarget');assert.equal(preparing.value.target,null);
  await peer.request('ui.createView',{viewId:'view-a',viewType:'status',context:context(servers[0])});
  assert.equal((await tool('sodeck_connection_get',{connectionRequestId:a.connection.connectionRequestId})).value.connection.state,'pending','registration alone is not a successful core create reply');
  backgrounds.set('a',{state:'created',viewId:'view-a'});await peer.request('connections.changed',{serverId:'server-a'});
  const ready=await until(()=>tool('sodeck_connection_get',{connectionRequestId:a.connection.connectionRequestId,waitMs:1000}),r=>r.value.connection.state==='ready');assert(ready.value.target.targetId);
  assert.equal((await open('a','open-a',1000)).value.connection.connectionRequestId,a.connection.connectionRequestId);assert.equal(created,1);
  const oldTarget=ready.value.target.targetId;const oldRevision=ready.value.connection.revision;
  const finishedCancel=(await tool('sodeck_connection_cancel',{connectionRequestId:a.connection.connectionRequestId})).value;
  assert.equal(finishedCancel.cancelResult,'alreadyFinished');assert.equal(finishedCancel.connection.state,'ready');assert.equal(finishedCancel.connection.revision,oldRevision);
  for(const sinceRevision of ['+1','1e1','18446744073709551616'])assert.equal((await tool('sodeck_connection_get',{connectionRequestId:a.connection.connectionRequestId,sinceRevision})).value.error.code,'invalidParams');
  sessions.get('a').disconnected=true;await peer.request('connections.changed',{serverId:'server-a'});
  const original=(await tool('sodeck_connection_get',{connectionRequestId:a.connection.connectionRequestId})).value;assert.equal(original.connection.revision,oldRevision);assert.equal(original.target.targetId,oldTarget);
  const offline=(await tool('sodeck_servers_list',{serverId:'server-a'})).value;assert.equal(offline.servers[0].target.availability,'disconnected');
  sessions.get('a').context=context(servers[0],2);sessions.get('a').disconnected=false;
  await peer.request('ui.createView',{viewId:'view-a',viewType:'status',context:context(servers[0],2)});
  const rebound=await until(()=>open('a','new-connection',1000),r=>r.value.connection.state==='ready');assert.notEqual(rebound.value.target.targetId,oldTarget);
  await peer.request('ui.disposeView',{viewId:'view-a',context:context(servers[0],1)});assert.equal((await tool('sodeck_servers_list',{serverId:'server-a'})).value.servers[0].target.targetId,rebound.value.target.targetId,'old disposal cannot remove the new generation');
  const failed=(await open('c','failure')).value;await until(async()=>sessions.has('c'),Boolean);sessions.get('c').accepted=true;backgrounds.set('c',{state:'failed',viewId:'view-c',error:'create failed'});await peer.request('connections.changed',{serverId:'server-c'});
  const failedReceipt=await until(()=>tool('sodeck_connection_get',{connectionRequestId:failed.connection.connectionRequestId,detail:true}),r=>r.value.connection.state==='failed');assert.deepEqual(failedReceipt.value.connection.error.context,context(servers[2]));assert(sessions.get('c').accepted,'target failure retains the accepted core session');
  const before=created,cancels=cancelCalls;const late=(await open('late','late-open')).value;await until(async()=>lateRelease,Boolean);
  const cancelling=tool('sodeck_connection_cancel',{connectionRequestId:late.connection.connectionRequestId});
  await until(async()=>cancelCalls,n=>n>cancels);
  lateRelease();await cancelling;
  await until(()=>tool('sodeck_connection_get',{connectionRequestId:late.connection.connectionRequestId}),r=>r.value.connection.state==='cancelled');
  assert.equal(created,before,'cancel before delayed core admission prevents a new attempt');
  assert.equal((await tool('sodeck_connection_get',{connectionRequestId:'old-run'})).value.error.code,'connectionRequestNotFound');
  await peer.close();
  console.log('MCP opening: zero targets, complete catalog paging, shared requests, interaction, creation ACK, stable receipt, reconnect, stale dispose and delayed-admission cancellation passed');
}finally{
  if(lateRelease)lateRelease();if(peer.child.exitCode===null)peer.child.kill();
  if(path.dirname(path.resolve(directory))!==path.resolve(os.tmpdir())||!path.basename(directory).startsWith('sodeck-mcp-open-'))throw new Error('Unexpected cleanup path');
  fs.rmSync(directory,{recursive:true,force:true});
}
