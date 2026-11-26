import type { Snapshot, Update, UpdateOverview, Manifest, OfficialPlugin, OfficialPreparation, UpdateNetwork, MirrorDiagnostic } from "./core";
import { updatesPage, pendingUpdates } from "./updates";
import { pluginsPage } from "./plugin-settings";


export { status, health, byteSize } from "./status";

export interface ViewState {
  officialPlugins: OfficialPlugin[];
  officialFetchedAt: number;
  officialLoading: boolean;
  officialError: string;
  officialPreparation?: OfficialPreparation | null;
  plugin: string;
  error: string;
  busy: Set<string>;
  updates: Record<string, Update>;
  updateOverview: UpdateOverview;
  excludedUpdates: Set<string>;
  pluginTab: "installed" | "official" | null;
  sourceDrafts: Record<string, NonNullable<Manifest["updateSource"]>>;
  networkDraft?: UpdateNetwork;
  networkMirrorKeys?: { revision: number; keys: string[] };
  networkError?: string;
  networkNotice?: string;
  networkTesting?: string;
  networkResults?: { endpoint: string; results: MirrorDiagnostic[] };
  networkGeneration?: number;
}
export function maintenance(s:Snapshot, v:ViewState, page:"plugins"|"updates") {
  return `<div class="maintenance-main"><section class="maintenance-page" data-scroll="${page}">${page === "plugins" ? pluginsPage(s,v) : updatesPage(s,v)}</section>${pendingUpdates(s,v)}</div>`;
}
