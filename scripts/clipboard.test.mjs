import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import { ClipboardBridge } from '../src/clipboard.ts';
const context={window:{}};vm.runInNewContext(fs.readFileSync('plugins/shared/terminal/clipboard.js','utf8'),context);
const TerminalClipboard=context.window.TerminalClipboard;
const deferred=()=>{let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};};

test('clipboard transport keeps escaped Unicode text within bounded frames and commits once',async()=>{
  const text='中文🙂\u0000\n'.repeat(5000);let written='',writes=0;
  const host=new ClipboardBridge(async()=>text,async value=>{written=value;writes++;});
  const read=await host.request('A',{op:'read'},()=>true);let result='';
  while(result.length<read.length){const chunk=await host.request('A',{op:'chunk',id:read.id,offset:result.length},()=>true);assert(Buffer.byteLength(JSON.stringify(chunk))<24*1024);result+=chunk.text;}
  assert.equal(result,text);
  const write=await host.request('A',{op:'beginWrite',length:text.length},()=>true);
  for(let offset=0;offset<text.length;offset+=2048)await host.request('A',{op:'writeChunk',id:write.id,offset,text:text.slice(offset,offset+2048)},()=>true);
  await host.request('A',{op:'commitWrite',id:write.id},()=>true);
  assert.equal(written,text);assert.equal(writes,1);
  await assert.rejects(host.request('B',{op:'commitWrite',id:write.id},()=>true));
});
test('clipboard rejects oversized text, stale focus, wrong order and partial writes',async()=>{
  const wait=deferred();let valid=true;
  const host=new ClipboardBridge(()=>wait.promise,async()=>assert.fail('must not write'));
  const read=host.request('A',{op:'read'},()=>valid);valid=false;wait.resolve('private');await assert.rejects(read,/切换/);
  const large=new ClipboardBridge(async()=>'中'.repeat(100000),async()=>{});await assert.rejects(large.request('A',{op:'read'},()=>true),/256 KiB/);
  await assert.rejects(host.request('A',{op:'read',extra:'中'.repeat(9000)},()=>true),/控制帧/);
  const write=await host.request('A',{op:'beginWrite',length:10},()=>true);
  await assert.rejects(host.request('A',{op:'writeChunk',id:write.id,offset:1,text:'bad'},()=>true),/顺序/);
  await assert.rejects(host.request('A',{op:'commitWrite',id:write.id},()=>true),/不完整/);
});
test('paste is single-flight and a tab/context change discards asynchronous clipboard results',async()=>{
  let state={key:'A',active:true,canPaste:true,canCopy:true,selection:'selected',selectionVersion:1},reads=0,pasted=[];
  const waiting=deferred();
  const c=new TerminalClipboard(()=>state,()=>{reads++;return waiting.promise;},async()=>true,text=>pasted.push(text),()=>{},e=>{throw e;});
  const first=c.pasteText(),duplicate=c.pasteText();state={...state,key:'B'};waiting.resolve('中文🙂');await Promise.all([first,duplicate]);assert.equal(reads,1);assert.deepEqual(pasted,[]);
  const fresh=new TerminalClipboard(()=>state,async()=>'once',async()=>true,text=>pasted.push(text),()=>{},e=>{throw e;});await fresh.pasteText();assert.deepEqual(pasted,['once']);
  const afterBlur=deferred();const old=new TerminalClipboard(()=>state,()=>afterBlur.promise,async()=>true,text=>pasted.push(text),()=>{},e=>{throw e;});const pending=old.pasteText();old.invalidate();afterBlur.resolve('late');await pending;assert.deepEqual(pasted,['once']);
});
test('copy preserves newer selection and clipboard errors preserve original selection',async()=>{
  let state={key:'A',active:true,canCopy:true,selection:'first',selectionVersion:1},cleared=0,errors=[];
  const waiting=deferred();const c=new TerminalClipboard(()=>state,async()=>'',()=>waiting.promise,()=>{},()=>cleared++,e=>errors.push(String(e)));
  const pending=c.copy();state={...state,selection:'second',selectionVersion:2};waiting.resolve(true);await pending;assert.equal(cleared,0);
  const failing=new TerminalClipboard(()=>state,async()=>'',async()=>{throw new Error('clipboard busy');},()=>{},()=>cleared++,e=>errors.push(String(e)));await failing.copy();assert.equal(cleared,0);assert.match(errors[0],/clipboard busy/);
});
