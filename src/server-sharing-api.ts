import {invoke} from '@tauri-apps/api/core';
import type {Catalog} from './server-api';

export interface ShareRow {name: string; originalName: string; endpoint: string; target: boolean; route: string[]; needsCredentials: boolean}
export interface ShareSummary {catalogRevision: number; targetCount: number; serverCount: number; proxyCount: number; keyCount: number; rows: ShareRow[]}
export const sharingApi = {
  export: (revision: number, serverId?: string) => invoke<{code: string; summary: ShareSummary}>('export_server_share', {revision, serverId: serverId ?? null}),
  preview: (code: string) => invoke<ShareSummary>('preview_server_share', {code}),
  import: (revision: number, code: string) => invoke<{catalog: Catalog; targetIds: string[]; addedIds: string[]; summary: ShareSummary}>('import_server_share', {revision, code}),
  copy: (text: string) => invoke('clipboard_text', {operation: 'write', text, purpose: 'serverShare'}),
};
