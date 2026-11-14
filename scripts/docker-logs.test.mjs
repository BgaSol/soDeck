import test from 'node:test';
import assert from 'node:assert/strict';
import {LogModel,timestamp,level} from '../plugins/docker/ui/logs-worker.js';
import {LogLayout} from '../plugins/docker/ui/logs-layout.js';
const encode = new TextEncoder();
const frame = (bytes, source='stdout') => encode.encode(JSON.stringify({v:1,sequence:'1',source,receivedAt:100000,dataBase64:Buffer.from(bytes).toString('base64')})+'\n');
function feed(model,id,bytes,offset=0,source='stdout') { const data=frame(bytes,source); model.append(id,data,offset+data.length); return offset+data.length; }
const register=(m,id='live',end=null)=>m.register({id,start:0,end});
test('nanosecond source timestamps, explicit levels and literal Unicode search',()=>{
  assert.equal(timestamp('2026-11-15T08:00:00.123456789+08:00'),timestamp('2026-11-15T00:00:00.123456789Z'));
  assert.equal(level('ordinary error mentioned in sentence'),''); assert.equal(level('[ERROR] refused'),'ERROR');
  const m=new LogModel();register(m);feed(m,'live','INFO 服务 [A+B] Ä ä\n');m.query({query:'ä'});
  assert.equal(m.view().matches,2);assert.deepEqual(m.view().rows[0].matches,[[14,15],[16,17]]);
  m.query({query:'[A+B]'});assert.equal(m.view().matches,1);
});
test('split envelopes, UTF8, interleaved streams and replay have stable identities',()=>{
  const m=new LogModel();register(m);const utf=encode.encode('INFO 中文\n');let at=feed(m,'live',utf.subarray(0,6));at=feed(m,'live','ERROR other\n',at,'stderr');at=feed(m,'live',utf.subarray(6),at);
  assert.equal(m.view().total,2);assert(m.view().rows.some(r=>r.text==='INFO 中文'));const range=m.pinRanges()[0];assert.equal(range.start,0);assert.equal(range.end,at);
  const replay=new LogModel();register(replay);let next=feed(replay,'live',utf.subarray(0,6));next=feed(replay,'live','ERROR other\n',next,'stderr');feed(replay,'live',utf.subarray(6),next);
  assert.deepEqual(m.layout().ids,replay.layout().ids);
});
test('paused reading survives recent rotation while fixed history still arrives',()=>{
  const m=new LogModel({recentRows:2});register(m);register(m,'history',200);let at=feed(m,'live','one\ntwo\n');m.anchor();at=feed(m,'live','three\nfour\n',at);feed(m,'history','earlier\n');
  assert.equal(m.view().total,3);assert.equal(m.view().newCount,2);assert(m.view().rows.some(r=>r.text==='one'));
  m.latest();assert.equal(m.view().total,2);assert(!m.view().rows.some(r=>r.text==='one'));
});
test('time ranges meet at a half-open boundary; identical lines are retained',()=>{
  const m=new LogModel();register(m,'history',100);m.register({id:'next',start:100,end:null});
  const text='1970-01-01T00:01:39.999999999Z repeated\n1970-01-01T00:01:40Z repeated\n1970-01-01T00:01:40Z repeated\n';feed(m,'history',text);feed(m,'next',text);
  assert.equal(m.view().total,3);
});
test('tail previews keep at most 2000 records and 2MiB of original text',()=>{
  const m=new LogModel();m.register({id:'tail',start:0,end:2000000000,tail:true});feed(m,'tail',Array.from({length:2001},(_,i)=>'INFO '+i+'\n').join(''));m.end('tail');assert.equal(m.view().total,2000);assert.equal(m.view().rows[0].text,'INFO 1');
  const large=new LogModel();large.register({id:'tail',start:0,end:2000000000,tail:true});let at=0;for(let i=0;i<4;i++)at=feed(large,'tail','INFO '+i+' '+'.'.repeat(700000)+'\n',at);large.end('tail');assert(large.history.reduce((n,r)=>n+r.bytes.length,0)<=2*1024*1024);assert.equal(large.history.length,2);
});
test('gap discards unfinished fragments and starts at an explicit block offset',()=>{
  const m=new LogModel();register(m);feed(m,'live','partial');m.resetSource('live',1000,true);feed(m,'live','tail\nvalid\n',1000);assert.deepEqual(m.view().rows.map(r=>r.text),['valid']);assert(m.view().gap);
});
test('reattaching the same capture never duplicates retained records or grows the identity index',()=>{
  const m=new LogModel({recentRows:2});register(m);const first=frame('one\ntwo\n');m.append('live',first,first.length);m.anchor();
  m.resetSource('live',0);m.append('live',first,first.length);assert.equal(m.view().total,2);assert.equal(new Set(m.layout().ids).size,2);
  feed(m,'live','three\nfour\n',first.length);assert.equal(m.ids.recent.size,2);assert.equal(m.fixedIds.size,2);m.latest();assert.equal(m.fixedIds.size,0);
  for(let i=0;i<30;i++)feed(m,'live','line '+i+'\n',m.sources.get('live').offset);assert.equal(m.ids.recent.size,2);
});
test('pinned replay at a nonzero offset restores a complete fragmented timestamp and drops a gap fragment',()=>{
  const m=new LogModel();register(m);const ignored=frame('earlier\n'),one=frame('2026-11-15T00:00:'),two=frame('00.123456789Z INFO anchor\n');
  m.append('live',ignored,ignored.length);m.append('live',one,ignored.length+one.length);m.append('live',two,ignored.length+one.length+two.length);
  const anchor=m.view().rows.find(r=>r.text==='INFO anchor'),range={id:'live',start:anchor?m.detail(anchor.id).start:0,end:ignored.length+one.length+two.length};
  const restored=new LogModel();register(restored);restored.resetSource('live',range.start,false,true);restored.append('live',one,range.start+one.length);restored.append('live',two,range.end);restored.restore([range]);
  assert.equal(restored.view().total,1);assert.equal(restored.layout().ids[0],anchor.id);assert.equal(restored.view().rows[0].text,'INFO anchor');
  const gap=new LogModel();register(gap);gap.resetSource('live',1000,true);feed(gap,'live','00.123456789Z leftover\n2026-11-15T00:00:01Z complete\n',1000);assert.deepEqual(gap.view().rows.map(r=>r.text),['complete']);
});
test('export is bounded, snapshot-based and preserves invalid UTF8 original bytes',()=>{
  const m=new LogModel();register(m);let at=feed(m,'live',new Uint8Array([73,78,70,79,32,255,10]));m.anchor();const length=m.exportBegin(null,'header').length;feed(m,'live','later\n',at);
  const chunks=[];let next;do{next=m.exportChunk();assert(next.bytes.length<=16384);chunks.push(next.bytes)}while(!next.done);
  const output=Buffer.concat(chunks);assert.equal(output.length,length);assert(output.includes(255));assert(!output.includes(Buffer.from('later')));m.exportEnd();
});
test('search-only context expands actual adjacent records and minimap targets real matches',()=>{
  const m=new LogModel();register(m);feed(m,'live','before\nERROR target target\nafter\n');m.query({query:'target',only:true});assert.equal(m.view().total,1);assert.equal(m.view().matches,2);
  const id=m.view().rows[0].id;assert.equal(m.summary(1).bins[0].target,id);m.context(id);assert.equal(m.view().total,3);assert(m.view().rows[0].context);
});
test('filtered result boundaries belong to its records and an empty result has no invented time',()=>{
  const m=new LogModel();register(m);feed(m,'live','1970-01-01T00:00:01Z old\nERROR target\n');m.query({query:'target',only:true});const view=m.view();assert.equal(view.total,1);assert.equal(view.windowTotal,2);assert.equal(view.first,null);assert.equal(view.firstReceivedAt,100000);
  m.query({query:'absent',only:true});assert.equal(m.view().total,0);assert.equal(m.view().firstReceivedAt,undefined);assert.equal(m.view().first,undefined);
});
test('wrapped rows keep record and pixel anchors after history is prepended',()=>{
  const layout=new LogLayout();layout.reset(['a','b','c']);layout.measure(0,100);layout.measure(1,60);assert.equal(layout.index(110),1);const anchor=layout.anchor(110);
  assert.equal(layout.restore(layout.anchor(-38)),-38,'space before the first record keeps its pixel position');
  layout.reset(['earlier','a','b','c']);assert.equal(layout.restore(anchor),133);assert.equal(layout.offset(4),206);
});
test('200000-row history and dense matches keep the display and index bounded',()=>{
  const m=new LogModel();register(m,'history',200);
  for(let i=0;i<400;i++)feed(m,'history',Array.from({length:500},(_,n)=>'INFO record '+(i*500+n)+'\n').join(''),m.sources.get('history').offset);
  assert.equal(m.view(199900,100).total,200000);assert.equal(m.view(199900,100).rows.length,100);m.query({query:'record'});assert.equal(m.view(0,1).matches,200000);
  const dense=new LogModel();register(dense);feed(dense,'live','a'.repeat(100000)+'\n');dense.query({query:'a'});assert.equal(dense.view(0,1).matches,100000);assert.equal(dense.projection().rows[0].count,100000);assert(!('matches' in dense.projection().rows[0]),'index stores counts, not one object per match');
});
