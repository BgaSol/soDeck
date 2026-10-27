import { context, core, onCoreNotification } from './index';
import {observeState,type StateSubscription} from './state';
import {workbenchSchema} from './workbench.generated';
import {validate,type Shape} from './schema';
import type { ThemeResponse, LiveTheme, MenuItem, MenuResult } from './workbench.generated';
export type ThemeResult=ThemeResponse;
export type { LiveTheme, MenuItem, ThemePreference, ThemeCatalog, ThemeCatalogEntry } from './workbench.generated';
let current:ThemeResult|undefined,initializing:Promise<ThemeResult>|undefined,feed:StateSubscription<LiveTheme>|undefined,connecting:Promise<void>|undefined;
let disposed=false,themeError:string|undefined;
const themeListeners=new Set<(theme:ThemeResult)=>void>(),errorListeners=new Set<(error:string|undefined)=>void>();
export function currentTheme(){return current;}
export function subscribeTheme(listener:(theme:ThemeResult)=>void){themeListeners.add(listener);if(current)listener(current);return()=>{themeListeners.delete(listener);};}
export function onThemeError(listener:(error:string|undefined)=>void){errorListeners.add(listener);listener(themeError);return()=>{errorListeners.delete(listener);};}
function report(error?:unknown){themeError=error?`外观同步未完成：${error instanceof Error?error.message:String(error)}`:undefined;for(const listener of errorListeners)listener(themeError);}
function liveTheme(value:unknown):LiveTheme{
  const schema=workbenchSchema as unknown as {$defs:Record<string,Shape>};
  const error=validate(schema,schema.$defs.LiveTheme,value,'theme');if(error)throw new Error(error);return value as LiveTheme;
}
function install(theme:ThemeResult){
  if(current?.schemaVersion===2&&theme.schemaVersion===2&&BigInt(theme.revision)<=BigInt(current.revision)){report();return;}
  const freeze=(v:unknown):void=>{if(v&&typeof v==='object'){Object.freeze(v);Object.values(v).forEach(freeze);}};
  freeze(theme);current=theme;applyTheme(theme);report();for(const listener of themeListeners){try{listener(theme);}catch(e){console.error(e);}}
}
export async function retryTheme():Promise<void>{
  if(disposed||current?.schemaVersion!==2)return;
  if(connecting)return connecting;
  connecting=(async()=>{
    await feed?.dispose();feed=undefined;
    const next=await observeState({call:(m,p)=>core.call(m,p),onNotification:onCoreNotification},{contract:'app.theme.v1',target:{app:'current'}},liveTheme,install,{onError:report});
    if(disposed){await next.dispose();return;}feed=next;if(next.current)install(next.current);
  })().catch(report).finally(()=>{connecting=undefined;});return connecting;
}

export function applyTheme(theme: ThemeResult): void {
  const root = document.documentElement;
  root.style.colorScheme = theme.mode;
  root.style.background = context().dialog ? theme.colors.transparent : theme.colors.canvas;
  for (const [name, color] of Object.entries(theme.colors)) root.style.setProperty(`--${name}`, color);
  root.style.setProperty('--terminal-background', theme.terminal.background);
  root.style.setProperty('--terminal-foreground', theme.terminal.foreground);
  root.style.setProperty('--terminal-cursor', theme.terminal.cursor);
  root.dataset.theme = theme.id;
  root.dataset.themeRevision=String(theme.revision);
  root.dataset.highContrast=String(theme.schemaVersion===2&&theme.highContrast);
}
export async function initializeTheme(): Promise<ThemeResult> {
  if(initializing)return initializing;
  initializing=(async()=>{
    const seed=context().theme;
    const result=seed?.schemaVersion===1?seed:await core.call<ThemeResult>('ui.theme.get');
    const theme=result.schemaVersion===2?liveTheme(result):result;
    install(theme);await retryTheme();
    window.addEventListener('focus',()=>{if(themeError)void retryTheme();});
    window.addEventListener('pagehide',()=>{disposed=true;void feed?.dispose().catch(()=>{});themeListeners.clear();errorListeners.clear();},{once:true});
    return current!;
  })();return initializing;
}

/** Subscribe before opening: a native menu can finish before the open receipt arrives. */
export async function showMenu(items: MenuItem[], anchor: { x: number; y: number }): Promise<string | null> {
  const menuId = crypto.randomUUID();
  let resolve!: (result: string | null) => void;
  const result = new Promise<string | null>(r => { resolve = r; });
  let finished = false;
  const complete = (v: MenuResult) => { if (v.menuId === menuId && v.status === 'closed' && !finished) { finished = true; resolve(v.selectedId); } };
  const off = onCoreNotification(n => { if (n.type === 'ui.menu.closed') complete(n.payload as MenuResult); });
  let polling = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  const cancel = () => { void core.call('ui.menu.close', { menuId }).catch(() => {}); if (!finished) { finished = true; resolve(null); } };
  window.addEventListener('pagehide', cancel);
  try {
    await core.call('ui.menu.open', { menuId, anchor, items });
    timer = setInterval(() => {
      if (polling || finished) return; polling = true;
      void core.call<MenuResult>('ui.menu.result', { menuId }).then(complete).catch(cancel).finally(() => { polling = false; });
    }, 750);
    return await result;
  } finally { off(); if (timer) clearInterval(timer); window.removeEventListener('pagehide', cancel); }
}
