import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ResourceDetailsData } from '../plugins/docker/ui/resource-details-data.js';

const resource = { kind:'container', id:'a'.repeat(64) };
const snapshot = { scopeEpoch:'1' };
const observation = (revision='1') => ({ resource, scopeEpoch:'1', observationRevision:revision, relation:{phase:'ready',count:0}, info:{counts:{ports:30,mounts:30,networks:1}} });
function fixture(handler) {
  const calls=[];
  const data=new ResourceDetailsData(async input => {
    calls.push(input);
    if(handler) { const result=await handler(input); if(result !== undefined) return result; }
    return input.action === 'browser.object' ? observation() : {resource,scopeEpoch:'1',observationRevision:input.observationRevision,offset:input.offset,rows:[input.offset],total:30,nextOffset:input.offset+2<30?input.offset+2:null};
  },()=>{});
  data.mount({resource},snapshot);data.enabled(true);
  return {data,calls};
}
test('overview reads no collections; selected pages reuse one cache and never load remote objects',async()=>{
  const {data,calls}=fixture();assert.equal(data.page('ports'),null);await data.refresh();assert.deepEqual(calls.map(c=>c.action),['browser.object']);
  data.select('ports');await data.refresh();data.select('mounts');await data.refresh();data.select('ports');await data.refresh();
  assert.deepEqual(calls.filter(c=>c.collection).map(c=>c.collection),['ports','mounts']);assert.equal(data.pages.size,2);
  assert(calls.every(c=>['browser.object','browser.collection'].includes(c.action)));
});
test('budget-short pages return to actual start and navigation restores it',async()=>{
  const {data}=fixture();data.select('ports');await data.refresh();data.navigate('ports','next');await data.refresh();assert.equal(data.offset('ports'),2);
  const navigation=data.navigation();const resumed=fixture().data;resumed.mount({resource},snapshot,navigation);resumed.enabled(true);await resumed.refresh();
  assert.equal(resumed.previous('ports'),0);resumed.navigate('ports','previous');await resumed.refresh();assert.equal(resumed.offset('ports'),0);
  assert.equal(resumed.page('ports').rows[0],0);
});
test('rapid tab and page changes coalesce behind one pending read',async()=>{
  let release;const pending=new Promise(resolve=>release=resolve);let once=true;
  const {data,calls}=fixture(async input=>{if(input.collection==='ports'&&once){once=false;await pending;}});
  data.select('ports');const first=data.refresh();await new Promise(resolve=>setTimeout(resolve,0));
  data.select('mounts');const second=data.refresh();assert.equal(first,second);release();await second;
  assert.equal(data.tab,'mounts');assert(data.page('mounts'));assert.deepEqual(calls.filter(c=>c.collection).map(c=>c.collection),['ports','mounts']);
});
test('new observation invalidates pages but keeps same-object pagination',async()=>{
  let revision='1';const {data,calls}=fixture(input=>input.action==='browser.object'?observation(revision):undefined);
  data.select('ports');await data.refresh();data.navigate('ports','next');await data.refresh();revision='2';await data.refresh();
  assert.equal(data.offset('ports'),2);assert.equal(data.page('ports').observationRevision,'2');assert.equal(calls.at(-1).offset,2);
});
test('late scope/resource/page replies and disposed reads cannot update the view',async()=>{
  let release;const pending=new Promise(resolve=>release=resolve);const {data}=fixture(async input=>{if(input.collection) {await pending;return {resource:{kind:'container',id:'other'},scopeEpoch:'1',observationRevision:'1',offset:0,rows:['wrong']};}});
  data.select('ports');const running=data.refresh();await new Promise(resolve=>setTimeout(resolve,0));release();await running;assert.equal(data.page('ports'),null);
  const scope=fixture();scope.data.update({resource},{scopeEpoch:'2'});await scope.data.refresh();assert.equal(scope.data.observation,null);
  data.dispose();await data.refresh();assert.equal(data.node,null);
});
test('hidden/candidate reads stop and collection errors survive ordinary render updates',async()=>{
  const {data,calls}=fixture(input=>{if(input.collection)throw Error('collection failed');});
  data.enabled(false);await data.refresh();assert.equal(calls.length,0);data.enabled(true);data.select('ports');await data.refresh();
  assert.equal(data.errors.get('ports'),'collection failed');data.update({resource},snapshot);assert.equal(data.errors.get('ports'),'collection failed');
});

test('shrinking and empty collections clamp restored pagination to valid offsets',async()=>{
  let count=30,revision='1';
  const {data}=fixture(input=>{
    if(input.action==='browser.object')return {...observation(revision),info:{counts:{ports:count}}};
    return {resource,scopeEpoch:'1',observationRevision:revision,offset:input.offset,rows:count?[input.offset]:[],total:count,nextOffset:null};
  });
  data.mount({resource},snapshot,{tab:'ports',offsets:{ports:20},previousOffsets:{ports:[0,5,10,15]}});data.enabled(true);await data.refresh();
  count=3;revision='2';await data.refresh();assert.equal(data.offset('ports'),2);assert.equal(data.page('ports').rows[0],2);assert.deepEqual(data.previousOffsets.ports,[0]);
  count=0;revision='3';await data.refresh();assert.equal(data.offset('ports'),0);assert.deepEqual(data.page('ports').rows,[]);assert.deepEqual(data.previousOffsets.ports,[]);
});

test('remount rejects a pending old-object reply while a new object reads independently',async()=>{
  let release;const pending=new Promise(resolve=>release=resolve),other={kind:'container',id:'b'.repeat(64)};
  const {data}=fixture(async input=>{
    if(input.resource.id===resource.id){await pending;return observation();}
    return {...observation('2'),resource:other};
  });
  const old=data.refresh();await new Promise(resolve=>setTimeout(resolve,0));
  data.mount({resource:other},snapshot);data.enabled(true);await data.refresh();
  release();await old;assert.equal(data.observation.resource.id,other.id);assert.equal(data.observation.observationRevision,'2');assert.equal(data.pages.size,0);
});

test('an older observation cannot replace newer cached pages',async()=>{
  let revision='2';const {data}=fixture(input=>input.action==='browser.object'?observation(revision):undefined);
  data.select('ports');await data.refresh();revision='1';await data.refresh();
  assert.equal(data.observation.observationRevision,'2');assert.equal(data.page('ports').observationRevision,'2');
});
