import { context, core, onCoreNotification } from './index';
import type { ThemeResult, MenuItem, MenuResult } from './workbench.generated';
export type { ThemeResult, MenuItem } from './workbench.generated';

export function applyTheme(theme: ThemeResult): void {
  const root = document.documentElement;
  root.style.colorScheme = theme.mode;
  for (const [name, color] of Object.entries(theme.colors)) root.style.setProperty(`--${name}`, color);
  root.style.setProperty('--terminal-background', theme.terminal.background);
  root.style.setProperty('--terminal-foreground', theme.terminal.foreground);
  root.style.setProperty('--terminal-cursor', theme.terminal.cursor);
  root.dataset.theme = theme.id;
}
export async function initializeTheme(): Promise<ThemeResult> {
  const theme = context().theme ?? await core.call<ThemeResult>('ui.theme.get');
  const freeze=(value:unknown):void=>{if(value&&typeof value==='object'){Object.freeze(value);for(const item of Object.values(value))freeze(item);}};
  freeze(theme);applyTheme(theme); return theme;
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
