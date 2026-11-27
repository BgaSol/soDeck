import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import {test} from 'node:test';

class Element {
  constructor(tag){this.tag=tag;this.children=[];this.dataset={};this.style={};this.attributes={};this.scrollTop=0;}
  append(...items){for(const item of items)this.insertBefore(item,null);}
  insertBefore(item,next){item.remove();const index=next?this.children.indexOf(next):this.children.length;this.children.splice(index,0,item);item.parent=this;}
  remove(){if(this.parent){const list=this.parent.children;list.splice(list.indexOf(this),1);this.parent=null;}}
  replaceChildren(...items){for(const child of [...this.children])child.remove();this.append(...items);}
  setAttribute(key,value){this.attributes[key]=value;}
  get lastElementChild(){return this.children.at(-1);}
  querySelectorAll(selector){const result=[];for(const child of this.children){if(selector==='[data-batch]'&&child.dataset.batch||selector==='[data-control]'&&child.dataset.control)result.push(child);result.push(...child.querySelectorAll(selector));}return result;}
  querySelector(selector){return selector==='.transfer-progress span'?this._transfer?.fill:null;}
}
function fixture(){
  let now=0;
  const scope={document:{createElement:tag=>new Element(tag),createTextNode:()=>new Element('text'),activeElement:null},performance:{now:()=>now},addEventListener(){},removeEventListener(){},setTimeout(){return 1;},clearTimeout(){},SoDeckMotion:{observeMotion(){},captureMotion(){return new Map();},cancelMotion(){},settleMotion(){}}};
  scope.window=scope;vm.createContext(scope);vm.runInContext(fs.readFileSync('plugins/files/ui/transfer.js','utf8'),scope);
  const task={id:'batch',kind:'download',state:'running',current:'file.bin',completed:'0',failed:'0',skipped:'0',currentOperation:{id:'copy',received:'0',total:'10000',transfer:{phase:'transferring',commitState:'pending'}}};
  const ui=new scope.FileTransfers({host:new Element('host'),event:async()=>({tasks:[structuredClone(task)]}),message(){},refresh:async()=>{}});
  return {ui,task,Rate:scope.FileTransferRate,tick:async(time,bytes)=>{now=time;if(bytes!=null)task.currentOperation.received=String(bytes);await ui.poll();},row:()=>ui.groups.active.list.children[0]||ui.groups.attention.list.children[0]};
}
test('speed survives intermediate refreshes, converges to zero and resets per operation',()=>{
  const {Rate}=fixture(),rate=new Rate();
  assert.equal(rate.read({id:'a',received:'0'},0),null);
  assert.equal(rate.read({id:'a',received:'350'},350),null);
  assert.equal(rate.read({id:'a',received:'700'},700),1000);
  assert.equal(rate.read({id:'a',received:'1050'},1050),1000);
  for(let at=1400;at<=3500;at+=700)rate.read({id:'a',received:'700'},at);
  assert.equal(rate.rate,0);
  assert.equal(rate.read({id:'b',received:'1'},4000),null);
});
test('task nodes stay mounted and full bytes remain finalizing until a confirmed result',async()=>{
  const f=fixture();await f.tick(0,0);const row=f.row(),detail=row._transfer.detail,fill=row._transfer.fill,button=row._transfer.actions.children[0];
  await f.tick(700,700);assert.match(detail.textContent,/KiB\/s/);
  await f.tick(1050,1050);assert.match(detail.textContent,/KiB\/s/);assert.equal(f.row(),row);assert.equal(row._transfer.detail,detail);assert.equal(row._transfer.fill,fill);assert.equal(row._transfer.actions.children[0],button);
  f.task.currentOperation.transfer.phase='finalizing';await f.tick(1400,10000);
  assert.equal(row.dataset.progress,'1');assert.match(row._transfer.title.textContent,/正在校验与保存/);assert.doesNotMatch(detail.textContent,/秒|KiB\/s/);
  f.task.state='partial';f.task.lastError='磁盘写入失败';f.task.lastFailure={error:{stage:'flush'}};await f.tick(1750);
  assert.match(f.row()._transfer.error.textContent,/刷盘失败：磁盘写入失败/);assert(f.row()._transfer.meter.hidden);
  assert.equal(f.ui.rates.size,0);
});
test('context changes fence pending polls and clearing tasks releases row caches',async()=>{
  const f=fixture();await f.tick(0);let release;f.ui.event=()=>new Promise(resolve=>release=resolve);const pending=f.ui.poll();f.ui.invalidate();release({tasks:[{...f.task,current:'stale'}]});await pending;
  assert.doesNotMatch(f.row()._transfer.title.textContent,/stale/);
  f.ui.event=async()=>({tasks:[]});await f.ui.poll();assert.equal(f.ui.last.size,0);assert.equal(f.ui.groups.active.list.children.length,0);
});
