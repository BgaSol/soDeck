import {beforeEach,afterEach,describe,it,expect,vi} from 'vitest';
import dark from '../../../themes/soft-dark.json';
import light from '../../../themes/soft-light.json';
const bridge=vi.hoisted(()=>({call:vi.fn(),onCoreNotification:vi.fn(),context:vi.fn()}));
vi.mock('./index',()=>({core:{call:bridge.call},onCoreNotification:bridge.onCoreNotification,context:bridge.context}));
const tick=()=>new Promise(r=>setTimeout(r,0));
describe('live theme singleton',()=>{
 let listeners:Set<(n:any)=>void>,events:EventTarget,css:Map<string,string>,revision:number;
 const theme=(n:number,mode='dark')=>({...structuredClone(mode==='dark'?dark:light),revision:String(n),highContrast:false,testOverride:false});
 const snapshot=(n:number,mode='dark')=>({scope:{appGeneration:'app'},objectId:'theme',revision:String(n),value:theme(n,mode)});
 beforeEach(()=>{
  vi.resetModules();vi.clearAllMocks();listeners=new Set();events=new EventTarget();css=new Map();revision=1;
  vi.stubGlobal('window',events);vi.stubGlobal('document',{documentElement:{style:{setProperty:(k:string,v:string)=>css.set(k,v)},dataset:{}}});
  bridge.context.mockReturnValue({});bridge.onCoreNotification.mockImplementation(fn=>{listeners.add(fn);return()=>listeners.delete(fn);});
  bridge.call.mockImplementation(async(m:string)=>m==='ui.theme.get'?theme(1):m==='state.open'?{subscriptionId:'s',feedEpoch:'e',sequence:'0',snapshot:snapshot(revision,revision===1?'dark':'light')}:{accepted:true});
 });
 afterEach(()=>vi.unstubAllGlobals());
 it('initializes once, replaces whole palettes without recreating consumers, ignores duplicates and stale revisions',async()=>{
  const sdk=await import('./presentation');await Promise.all([sdk.initializeTheme(),sdk.initializeTheme()]);await tick();
  expect(bridge.call.mock.calls.filter(c=>c[0]==='state.open')).toHaveLength(1);
  const values:string[]=[];const off=sdk.subscribeTheme(t=>values.push(String(t.revision)));
  for(const listener of listeners)listener({type:'state.update',payload:{subscriptionId:'s',feedEpoch:'e',sequence:'1',snapshot:snapshot(2,'light')}});
  await tick();expect(values).toEqual(['1','2']);expect(css.get('--canvas')).toBe(light.colors.canvas);expect(css.get('--terminal-background')).toBe(light.terminal.background);
  for(const listener of listeners)listener({type:'state.update',payload:{subscriptionId:'s',feedEpoch:'e',sequence:'1',snapshot:snapshot(2,'light')}});
  await tick();expect(values).toEqual(['1','2']);expect(Object.isFrozen(sdk.currentTheme()!.colors)).toBe(true);
  off();events.dispatchEvent(new Event('pagehide'));await tick();expect(listeners.size).toBe(0);
 });
 it('keeps last complete palette on failure and recovers from focus with current snapshot',async()=>{
  const sdk=await import('./presentation');await sdk.initializeTheme();let error:string|undefined;const off=sdk.onThemeError(e=>error=e);
  for(const listener of [...listeners])listener({type:'state.closed',payload:{subscriptionId:'s',feedEpoch:'e',reason:'transport-gap'}});
  await tick();expect(error).toContain('外观同步');expect(css.get('--canvas')).toBe(dark.colors.canvas);
  revision=3;events.dispatchEvent(new Event('focus'));await tick();expect(sdk.currentTheme()!.revision).toBe('3');expect(error).toBeUndefined();off();events.dispatchEvent(new Event('pagehide'));
 });
});
