import {it,expect} from 'vitest';
import {Scheduler} from './scheduler';
it('limits targets and lanes independently and coalesces duplicate intentions',async()=>{
  const s=new Scheduler(),release:(()=>void)[]=[];
  for(let i=0;i<9;i++)s.want(String(i),()=>{});
  for(let i=0;i<8;i++)for(const lane of ['fast','slow'] as const)s.enqueue(String(i),lane,()=>new Promise<void>(resolve=>release.push(resolve)));
  expect(s.counts()).toMatchObject({targets:8,waiting:1,fast:1,slow:1,queued:14});expect(s.counts().transientBytes).toBeLessThanOrEqual(4194304);
  expect(s.enqueue('0','fast',async()=>{})).toBe(false);
  s.drop('0');expect(s.counts().waiting).toBe(0);expect(s.counts().targets).toBe(8);
  await Promise.resolve();for(const resolve of release)resolve();
  await new Promise(resolve=>setTimeout(resolve,0));expect(s.counts().fast).toBeLessThanOrEqual(3);expect(s.counts().slow).toBe(1);
});
