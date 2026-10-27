import {it,expect} from 'vitest';
import {networkBridge,targetOperation} from './bridge';
it('bounds all rule calls while reserving state acknowledgements and cleanup capacity',async()=>{
 const calls:Array<{method:string;release:()=>void}>=[];
 const bridge=networkBridge({onNotification:()=>()=>{},call:<T>(method:string)=>new Promise<T>(r=>calls.push({method,release:()=>r({} as T)}))});
 const normal=Array.from({length:268},()=>bridge.call('net.tcp.open',{}));expect(calls.length).toBe(12);
 await expect(bridge.call('net.tcp.open',{})).rejects.toMatchObject({code:'CAPACITY'});
 const ack=bridge.call('state.ack',{}),stop=bridge.call('net.groups.close',{});
 expect(calls.slice(-2).map(c=>c.method)).toEqual(['state.ack','net.groups.close']);
 let index=0;while(index<calls.length){calls[index++].release();await Promise.resolve();await Promise.resolve();await Promise.resolve();}
 await Promise.all([...normal,ack,stop]);expect(calls.length).toBe(270);
});
it('holds target slots through pending receipts and releases them after success or failure',async()=>{
 const bridge={onNotification:()=>()=>{},call:async<T>()=>({state:'pending'} as T)};
 const releases:Array<()=>void>=[];let active=0,peak=0;
 const tasks=Array.from({length:64},(_,i)=>targetOperation(bridge,async()=>{
   active++;peak=Math.max(peak,active);await bridge.call();
   await new Promise<void>(r=>releases.push(r));active--;
   if(i===10)throw Error('target refused');return i;
 }).catch(()=>-1));
 await Promise.resolve();await Promise.resolve();expect(releases).toHaveLength(12);
 for(let i=0;i<64;i++){while(!releases[i])await Promise.resolve();releases[i]();}
 const results=await Promise.all(tasks);expect(peak).toBe(12);expect(active).toBe(0);expect(results[10]).toBe(-1);expect(results[63]).toBe(63);
});
