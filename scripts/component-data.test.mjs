import assert from 'node:assert/strict';
import fs from 'node:fs';
import {test} from 'node:test';
import {transformWithOxc} from 'vite';
import {Channel} from '@tauri-apps/api/core';

const transformed=await transformWithOxc(fs.readFileSync('src/component-data.ts','utf8'),'component-data.ts');
const code=transformed.code.replace('"@tauri-apps/api/core"',JSON.stringify(new URL('../node_modules/@tauri-apps/api/core.js',import.meta.url).href));
const {ComponentData}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
const tick=()=>new Promise(resolve=>setImmediate(resolve));

function fixture() {
  const callbacks=new Map(),calls=[],streams=new Map();let serial=0;
  globalThis.window={__TAURI_INTERNALS__:{transformCallback:fn=>{callbacks.set(++serial,fn);return serial;},unregisterCallback:id=>callbacks.delete(id)}};
  const key=a=>a.surfaceId+':'+a.request.id;
  const send=(a,index,message)=>callbacks.get(a.channel.id)?.({index,message});
  const end=(a,index)=>callbacks.get(a.channel.id)?.({index,end:true});
  const client=new ComponentData(async(command,args)=>{
    assert.equal(command,'component_data');calls.push(args);
    const {request}=args;
    if(request.op==='attach'||request.action==='attach') {
      const previous=streams.get(key(args));if(previous)end(previous,previous.index);
      args.index=0;streams.set(key(args),args);send(args,args.index++,request.id+':reset');
    } else {
      if(request.op==='detach'||request.action==='detach') {const stream=streams.get(key(args));if(stream)end(stream,stream.index);streams.delete(key(args));}
      end(args,0);
    }
    return {};
  });
  return {client,calls,callbacks,send,end,emit:(token,id,text)=>{const a=streams.get(token+':'+id);send(a,a.index++,text);}};
}

test('native sequences restart per invocation; a reused Channel loses messages and can close an active stream',()=>{
  const f=fixture(),received=[],channel=new Channel(value=>received.push(value)),args={channel};
  f.send(args,0,'first stream');f.send(args,0,'second stream');f.end(args,0);f.send(args,1,'first continues');
  assert.deepEqual(received,['first stream','first continues']);
  f.end(args,2);assert.equal(f.callbacks.size,0);
});

test('one surface receives two streams, ACKs, replacement subscriptions and terminal data independently',async()=>{
  const f=fixture(),a=[],b=[],terminal=[];
  await f.client.request('page',{op:'sessionStream',action:'attach',id:'history'},v=>a.push(v));
  await f.client.request('page',{op:'sessionStream',action:'attach',id:'live'},v=>b.push(v));
  await f.client.request('page',{op:'sessionStream',action:'ack',id:'live'},()=>assert.fail('ACK has no frames'));
  f.emit('page','history','historical body');f.emit('page','live','live body');
  const replacement=[];
  await f.client.request('page',{op:'sessionStream',action:'attach',id:'history'},v=>replacement.push(v));
  f.emit('page','history','replayed history');f.emit('page','live','live continues');
  await f.client.request('page',{op:'attach',id:'pty'},v=>terminal.push(v));f.emit('page','pty','prompt');
  assert.deepEqual(a,['history:reset','historical body']);assert.deepEqual(b,['live:reset','live body','live continues']);
  assert.deepEqual(replacement,['history:reset','replayed history']);assert.deepEqual(terminal,['pty:reset','prompt']);
  assert.equal(new Set(f.calls.map(a=>a.channel.id)).size,f.calls.length);
  f.client.detach('page');await tick();assert.equal(f.callbacks.size,0);
  assert.deepEqual(f.calls.slice(-3).map(a=>a.request),[{op:'sessionStream',action:'detach',id:'history'},{op:'sessionStream',action:'detach',id:'live'},{op:'detach',id:'pty'}]);
});

test('surface cleanup owns pending and successful subscriptions, but excludes failed and explicitly detached resources',async()=>{
  const f=fixture();await f.client.request('other',{op:'attach',id:'pty'},()=>{});
  await f.client.request('page',{op:'sessionStream',action:'attach',id:'live'},()=>{});
  await f.client.request('page',{op:'sessionStream',action:'detach',id:'live'},()=>{});
  f.client.detach('page');await tick();assert.equal(f.callbacks.size,1,'other surface remains live');
  let reject;const calls=[];
  const client=new ComponentData(async(_,args)=>{calls.push(args);if(calls.length===1)return new Promise((_,r)=>reject=r);return {};});
  const old=client.request('page',{op:'attach',id:'same'},()=>{});
  await client.request('page',{op:'attach',id:'same'},()=>{});reject(new Error('old attach failed'));await assert.rejects(old,/old attach failed/);
  client.detach('page');await tick();assert.equal(calls.at(-1).request.op,'detach','old failure must not erase newer subscription');
  const failed=new ComponentData(async()=>{throw new Error('attach rejected');});
  await assert.rejects(failed.request('page',{op:'attach',id:'failed'},()=>{}));assert.equal(failed.subscriptions.get('page').size,0);
  f.client.detach('other');await tick();
});
