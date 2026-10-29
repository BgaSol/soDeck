import {it,expect,vi,afterEach} from 'vitest';
import {Connection} from './connection';
import type {ExecBridge} from '@sodeck/sdk/bounded-exec';
afterEach(()=>vi.useRealTimers());
it('explicit failure preserves the attempted connection reference',async()=>{
 const reference={bootId:'b',attemptId:'explicit',attemptGeneration:8};
 const failure={code:'PROXY_DISCONNECTED',details:{diagnosticRef:reference}};
 const bridge={call:async(method:string)=>{if(method==='servers.get')return {revision:1};if(method==='capabilities.list')return {capabilities:['connections.route.v2']};if(method==='connections.route.get')return {saved:{attemptTimeoutMs:120000}};if(method==='resources.list')return {leases:[],connections:[]};if(method==='connections.acquire')throw failure;return {};}} as ExecBridge;
 const c=new Connection(bridge,{workspaceId:'explicit-w',workspaceGeneration:'g'},'s','v',{monotonicMs:()=>0},'monitor');await expect(c.acquire(true,'explicit')).rejects.toEqual(failure);expect(c.diagnosticRef).toEqual(reference);expect(c.lease).toBeUndefined();await c.retry();
});
it('passive failure explanation reads only the matching session history and never dials',async()=>{
 const calls:{method:string;params:unknown}[]=[],reference={bootId:'b',attemptId:'failed',attemptGeneration:7};
 const bridge={call:async(method:string,params:unknown)=>{calls.push({method,params});if(method==='servers.get')return {revision:1};if(method==='connections.observe')return {state:'failed'};if(method==='connections.diagnostics.list')return {attempts:[{state:'failed',reference,error:{code:'PROXY_DISCONNECTED'},purpose:'session'}],total:1,nextCursor:null};throw Error('Unexpected '+method);}} as ExecBridge;
 const c=new Connection(bridge,{workspaceId:'w',workspaceGeneration:'g'},'s','v',{monotonicMs:()=>0});await c.acquire(false,'observe');
 expect(c.diagnosticRef).toEqual(reference);expect(c.lease).toBeUndefined();expect(calls.map(c=>c.method)).toEqual(['servers.get','connections.observe','connections.diagnostics.list']);
 expect(calls.at(-1)?.params).toEqual({workspaceId:'w',workspaceGeneration:'g',serverId:'s',purpose:'session'});
});
it('retains late acquire and failed release until the original lease is reclaimed',async()=>{
 vi.useFakeTimers();let complete!:(v:unknown)=>void,failRelease=true;
 const calls:string[]=[],lease={leaseId:'owned',connectionId:'c',generation:1};
 const bridge={call:async(method:string)=>{calls.push(method);if(method==='servers.get')return {revision:1,name:'s'};if(method==='connections.observe')return {state:'active'};if(method==='connections.acquireExisting')return new Promise<unknown>(r=>complete=r);if(method==='connections.release'&&failRelease)throw Error('temporarily unavailable');return {};}} as ExecBridge;
 const c=new Connection(bridge,{workspaceId:'w',workspaceGeneration:'g'},'s','v',{monotonicMs:()=>Date.now()});
 const pending=c.acquire(false,'one').catch(e=>e.code);await vi.advanceTimersByTimeAsync(6001);expect(await pending).toBe('CLEANUP_PENDING');expect(c.cleanupPending).toBe(true);
 await c.acquire(false,'two');expect(calls.filter(x=>x==='connections.acquireExisting')).toHaveLength(1);
 complete(lease);await vi.advanceTimersByTimeAsync(1);await c.retry();expect(c.cleanupPending).toBe(true);expect(c.lease).toBeUndefined();failRelease=false;await c.retry();expect(c.cleanupPending).toBe(false);
});
it('rejects a different connection generation before sampling',async()=>{
 const released:string[]=[];
 const bridge={call:async(method:string,p:{leaseId?:string})=>{if(method==='servers.get')return {revision:1};if(method==='connections.observe')return {state:'active'};if(method==='connections.acquireExisting')return {leaseId:'owned',connectionId:'c',generation:1};if(method==='connections.route.get')return {running:[{connectionId:'c',generation:2,state:'active'}]};if(method==='connections.release')released.push(p.leaseId!);return {};}} as ExecBridge;
 const c=new Connection(bridge,{workspaceId:'w',workspaceGeneration:'g'},'s','v',{monotonicMs:()=>0});await expect(c.acquire(false,'one')).rejects.toMatchObject({code:'CONNECTION_NOT_READY'});expect(released).toEqual(['owned']);expect(c.lease).toBeUndefined();await c.retry();
});
it('reconciles unknown explicit acquisition without releasing other owners or reopening the workspace early',async()=>{
 let attempted=false,queryFails=false;const released:string[]=[];
 const connections=[{id:'c',serverId:'s',workspaceId:'w',generation:1},{id:'elsewhere',serverId:'s',workspaceId:'other',generation:2}];
 const before=[{id:'old',connectionId:'c',instanceId:'monitor'}];
 const bridge={call:async(method:string,p:{leaseId?:string})=>{
  if(method==='servers.get')return {revision:1};if(method==='capabilities.list')return {capabilities:['connections.route.v2']};
  if(method==='connections.route.get')return {saved:{attemptTimeoutMs:120000}};
  if(method==='resources.list'){if(queryFails)throw Error('temporarily unavailable');return {connections,leases:attempted?[...before,{id:'new',connectionId:'c',instanceId:'monitor'},{id:'terminal',connectionId:'c',instanceId:'terminal'},{id:'other-workspace',connectionId:'elsewhere',instanceId:'monitor'}]:before};}
  if(method==='connections.acquire'){attempted=true;throw Object.assign(Error('audit result missing'),{code:'RESULT_UNKNOWN'});}
  if(method==='connections.release')released.push(p.leaseId!);return {};
 }} as ExecBridge;
 const c=new Connection(bridge,{workspaceId:'w',workspaceGeneration:'g'},'s','v',{monotonicMs:()=>0},'monitor');
 await expect(c.acquire(true,'one')).rejects.toMatchObject({code:'RESULT_UNKNOWN'});expect(c.cleanupPending).toBe(true);
 queryFails=true;await c.retry();expect(c.cleanupPending).toBe(true);
 const replacement=new Connection(bridge,{workspaceId:'w',workspaceGeneration:'next'},'s','v2',{monotonicMs:()=>0},'monitor');
 await expect(replacement.acquire(false,'two')).rejects.toMatchObject({code:'CLEANUP_PENDING'});
 queryFails=false;await c.retry();expect(released).toEqual(['new']);expect(c.cleanupPending).toBe(false);
});
