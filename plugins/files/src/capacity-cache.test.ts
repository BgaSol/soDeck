import {expect,it,vi} from 'vitest';
import {CapacityCache} from './capacity-cache';
it('bounds concurrency across cancellation and discards late results across generations',async()=>{
  const jobs=new Map<string,{resolve(value:string):void;cancel:ReturnType<typeof vi.fn>}>();
  const cache=new CapacityCache<string>(key=>{let resolve!:(v:string)=>void;const promise=new Promise<string>(r=>resolve=r),cancel=vi.fn();jobs.set(key,{resolve,cancel});return {promise,cancel};},()=>{});
  cache.visible(['a','b','c']);expect(cache.diagnostics().active).toBe(2);expect(jobs.has('c')).toBe(false);
  cache.visible(['c','d']);expect(jobs.get('a')!.cancel).toHaveBeenCalledOnce();expect(cache.diagnostics().active).toBe(2);
  jobs.get('a')!.resolve('late');await vi.waitFor(()=>expect(jobs.has('c')).toBe(true));expect(cache.get('a')).toBeUndefined();
  cache.clear();jobs.get('b')!.resolve('old');jobs.get('c')!.resolve('old');await vi.waitFor(()=>expect(cache.diagnostics().active).toBe(0));expect(cache.diagnostics().entries).toBe(0);
});
it('caches failures until refresh and evicts settled entries with a strict LRU bound',async()=>{
  const load=vi.fn((key:string)=>({promise:key==='bad'?Promise.reject(Error('unavailable')):Promise.resolve(key),cancel(){}}));
  const cache=new CapacityCache(load,()=>{},3);
  cache.visible(['bad']);await vi.waitFor(()=>expect(cache.get('bad')?.state).toBe('unavailable'));
  cache.visible([]);cache.visible(['bad']);expect(load).toHaveBeenCalledTimes(1);
  for(const id of ['1','2','3','4']){cache.visible([id]);await vi.waitFor(()=>expect(cache.get(id)?.state).toBe('ready'));expect(cache.diagnostics().entries).toBeLessThanOrEqual(3);}
  cache.clear();cache.visible(['bad']);await vi.waitFor(()=>expect(cache.get('bad')?.state).toBe('unavailable'));expect(load).toHaveBeenCalledTimes(6);
});
