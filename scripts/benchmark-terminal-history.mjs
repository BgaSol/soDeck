// Repeatable computation/memory benchmark. UI debounce, xterm and native WebViews are measured separately.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import {HistoryIndex} from '../plugins/terminal/ui-src/history-worker.js';
assert.equal(typeof global.gc,'function','Run with node --expose-gc');
global.gc();const initial=process.memoryUsage();
let rows=Array.from({length:100000},(_,id)=>({id,text:`${String(id).padStart(6,'0')} INFO timeout 中文🙂 /api/worker-${id%25} 0123456789 abcdefghijklmnopqrstuvwxyz ${'a'.repeat(30)}`,wrapped:false,colors:new Uint32Array(32)}));
for(const row of rows)row.colors.fill(row.id%7?0x3000000:0x1dd3344,0,28);
const bytes=rows.reduce((n,r)=>n+Buffer.byteLength(r.text),0),index=new HistoryIndex();index.reset(1);
let now=performance.now();index.patch({epoch:1,head:0,length:rows.length,rows});const indexMs=performance.now()-now;
rows=undefined;
let revision=0;await index.queryFor({query:'warmup',caseSensitive:false,queryRevision:++revision,anchorRow:50000});
const queries=[...Array.from({length:25},(_,n)=>({kind:'rare',query:`worker-${n} `})),...Array.from({length:25},(_,n)=>({kind:'missing',query:`missing-${n}`})),...Array.from({length:25},(_,n)=>({kind:'dense',query:'a'.repeat(n+3)})),...'abcdefghijklmnopqrstuvwxyz'.slice(0,25).split('').map(query=>({kind:'short',query}))],samples=[];
for(const sample of queries){now=performance.now();await index.queryFor({query:sample.query,caseSensitive:false,queryRevision:++revision,anchorRow:50000});const ms=performance.now()-now;assert(Number.isSafeInteger(index.total));samples.push({...sample,ms,total:index.total});}
const stats=values=>{const sorted=values.slice().sort((a,b)=>a-b);return {p50:sorted[Math.floor(sorted.length*.5)],p95:sorted[Math.ceil(sorted.length*.95)-1],max:sorted.at(-1)};};
global.gc();const memory=process.memoryUsage(),groups=Object.fromEntries(['rare','missing','dense','short'].map(kind=>[kind,stats(samples.filter(s=>s.kind===kind).map(s=>s.ms))]));
const report={platform:process.platform,arch:process.arch,node:process.version,cpu:os.cpus()[0]?.model,rows:100000,columns:120,textBytes:bytes,indexMs,queries:samples.length,groups,heapDelta:memory.heapUsed-initial.heapUsed,arrayBufferDelta:memory.arrayBuffers-initial.arrayBuffers,retainedHeap:memory.heapUsed,samples};
fs.mkdirSync('artifacts/terminal-history',{recursive:true});fs.writeFileSync('artifacts/terminal-history/search.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({...report,samples:undefined},null,2));
