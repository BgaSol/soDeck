import {convertFileSrc} from '@tauri-apps/api/core';
import type {Manifest, OfficialPlugin} from './core';
import type {Icon} from './ui';
export function packageIcon(manifest: Manifest, name = manifest.icon || 'plug'): Icon {
  return manifest.icons?.[name] ? resourceIcon(`${manifest.id}/icons/${manifest.version}/${name}`, name) : 'plug';
}
export function resourceIcon(url: string | null | undefined, fallback = 'plug'): Icon {
  return url ? {url:new URL(url,convertFileSrc('', 'sodeck-plugin')).href} : fallback;
}
export function catalogIcon(plugin: OfficialPlugin): Icon {
  if (plugin.iconData) return {url:plugin.iconData};
  return 'plug';
}
