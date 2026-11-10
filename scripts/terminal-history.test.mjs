import assert from 'node:assert/strict';
import {test} from 'node:test';
import {HistoryIndex,fold} from '../plugins/terminal/ui-src/history-worker.js';
const row=(id,text,wrapped=false)=>({id,text,wrapped,colors:new Uint32Array(32)});
async function setup(rows,query,caseSensitive=false){const messages=[],index=new HistoryIndex(m=>messages.push(m));index.reset(1);index.patch({epoch:1,head:0,length:rows.length,rows});await index.queryFor({query,caseSensitive,queryRevision:1,anchorRow:0});return {index,messages};}
test('literal matches span soft wraps, use non-overlap, and map Unicode expansions',async()=>{
  const {index,messages}=await setup([row(0,'aa'),row(1,'aa',true),row(2,'İ 中文🙂')],'aa');assert.equal(index.total,2);
  await index.locate({ordinal:1,queryRevision:1});assert.deepEqual(messages.at(-1).match,{start:{row:1,offset:0},end:{row:1,offset:2}});
  await index.queryFor({query:'i',caseSensitive:false,queryRevision:2,anchorRow:0});assert.equal(index.total,1);await index.locate({ordinal:0,queryRevision:2});assert.deepEqual(messages.at(-1).match,{start:{row:2,offset:0},end:{row:2,offset:1}});
  await index.queryFor({query:'中文🙂',caseSensitive:false,queryRevision:3,anchorRow:0});assert.equal(index.total,1);
  await index.queryFor({query:'aaİ',caseSensitive:false,queryRevision:4,anchorRow:0});assert.equal(index.total,0);
});
test('incremental checkpoints retain crossing matches and remove trimmed/overwritten rows',async()=>{
  const rows=Array.from({length:800},(_,id)=>row(id,id%11===0?'aaaa timeout':'prefix',id%3!==0));
  const {index}=await setup(rows,'prefixaa');
  rows[755]=row(755,'timeout prefix',true);rows.push(row(800,'prefixaaaa',true));index.patch({epoch:1,head:0,length:801,rows:[rows[755],rows[800]]});await index.search();
  const baseline=await setup(rows,'prefixaa');assert.equal(index.total,baseline.index.total);assert.deepEqual([...index.rows.values()].map(r=>r.count),[...baseline.index.rows.values()].map(r=>r.count));
  index.patch({epoch:1,head:30,length:771,rows:[]});await index.search();const trimmed=await setup(rows.slice(30).map((r,id)=>({...r,id})),'prefixaa');assert.equal(index.total,trimmed.index.total);
});
test('million dense matches keep counts rather than a result object per match',async()=>{
  const {index,messages}=await setup(Array.from({length:10000},(_,id)=>row(id,'a'.repeat(120))),'a');assert.equal(index.total,1200000);assert.equal(messages.length,1);await index.locate({ordinal:1199999,queryRevision:1});assert.deepEqual(messages.at(-1).match,{start:{row:9999,offset:119},end:{row:9999,offset:120}});
});
test('superseded queries and a reset cannot deliver an old final result',async()=>{
  const {index,messages}=await setup(Array.from({length:5000},(_,id)=>row(id,'timeout 中文')),'timeout');messages.length=0;
  const old=index.queryFor({query:'timeout',caseSensitive:false,queryRevision:2,anchorRow:0});messages.length=0;const latest=index.queryFor({query:'missing',caseSensitive:false,queryRevision:3,anchorRow:0});await Promise.all([old,latest]);assert(messages.every(m=>m.queryRevision===3));assert.equal(index.total,0);
  const pending=index.queryFor({query:'timeout',caseSensitive:false,queryRevision:4,anchorRow:0});index.reset(2);messages.length=0;await pending;assert(messages.every(m=>m.queryRevision!==4));
  assert.equal(fold('Σ'),'σ');
  assert.equal(fold('ΟΣ'),fold('ος'));
});
test('compact color runs preserve rare colors through overwrite, trim and minimap projection',async()=>{
  const samples=Array.from({length:5000},(_,id)=>row(id,'plain'));for(const r of samples)r.colors.fill(0x3000000,0,24);samples[1200].colors.fill(0x1ff0022,0,16);
  const {index,messages}=await setup(samples,'plain');assert(index.rows.get(0).colors.byteLength<32*4);index.map(500);assert(messages.at(-1).colors.includes(0x1ff0022));
  index.patch({epoch:1,head:1260,length:3740,rows:[]});await index.search();index.map(500);assert(!messages.at(-1).colors.includes(0x1ff0022));assert(messages.at(-1).colors.includes(0x3000000));
});
