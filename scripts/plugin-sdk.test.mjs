import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

function surface() {
  const events=new EventTarget(), sent=[], timers=new Map();let next=0;
  const parent={postMessage:frame=>sent.push(frame)};
  const runtime={parent,document:{readyState:'complete'},Map,Error,Promise,CustomEvent,
    addEventListener:events.addEventListener.bind(events),dispatchEvent:events.dispatchEvent.bind(events),
    setTimeout:fn=>{timers.set(++next,fn);return next;},clearTimeout:id=>timers.delete(id)};
  runtime.window=runtime;
  vm.runInNewContext(fs.readFileSync('packages/plugin-sdk/src/index.js','utf8'),runtime);
  const send=(message,{source=parent,origin='http://core'}={})=>{
    const event=new Event('message');Object.assign(event,{source,origin,data:{channel:'sodeck-ui-v2',surfaceId:'one',...message}});events.dispatchEvent(event);
  };
  return {client:runtime.sodeck,sent,timers,send};
}

test('page SDK fences replies by parent and Surface, retaining structured errors',async()=>{
  const s=surface();assert.equal(s.sent[0].type,'ready');
  await assert.rejects(s.client.request('event',{}),e=>e.code==='surfaceClosed'&&e.data.dispatch==='notSent');
  s.send({type:'mount'});const pending=s.client.request('event',{}),frame=s.sent.at(-1);
  s.send({type:'response',id:frame.id,data:'foreign'},{source:{}});
  s.send({type:'response',id:frame.id,data:'foreign',surfaceId:'old'});
  s.send({type:'response',id:frame.id,data:'foreign'},{origin:'http://other'});
  assert.equal(s.timers.size,1);
  s.send({type:'response',id:frame.id,error:{code:-32000,message:'conflict',data:{code:'revisionConflict',effect:'unknown'}}});
  await assert.rejects(pending,e=>e.code===-32000&&e.data.code==='revisionConflict'&&e.data.effect==='unknown');
  assert.equal(s.timers.size,0);
});

test('page SDK bounds pending work and clears every timer on unmount and replacement',async()=>{
  const s=surface();s.send({type:'mount'});
  const pending=Array.from({length:64},()=>s.client.request('event',{}));
  const checked=pending.map(p=>assert.rejects(p,e=>e.code==='surfaceClosed'&&e.data.dispatch==='unknown'));
  await assert.rejects(s.client.request('event',{}),e=>e.code==='resourceBusy');
  s.send({type:'unmount'});await Promise.all(checked);assert.equal(s.timers.size,0);
  await assert.rejects(s.client.request('event',{}),e=>e.code==='surfaceClosed');
  s.send({type:'mount',surfaceId:'two'});const old=s.client.request('event',{});
  const rejected=assert.rejects(old,e=>e.code==='surfaceClosed');
  s.send({type:'mount',surfaceId:'three'});await rejected;assert.equal(s.timers.size,0);
  const current=s.client.request('data',{}),id=s.sent.at(-1).id;
  s.send({type:'response',surfaceId:'three',id,data:{ok:true}});assert.equal((await current).ok,true);
});

test('page SDK reports sent timeouts as unknown and preserves explicit flush participation',async()=>{
  const s=surface();s.send({type:'mount'});s.client.prepared(true);
  assert.equal(s.sent.at(-1).flush,true);
  const pending=s.client.request('event',{}),rejected=assert.rejects(pending,e=>e.code==='requestTimeout'&&e.data.dispatch==='unknown');
  s.timers.values().next().value();await rejected;
});
