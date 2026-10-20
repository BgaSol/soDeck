import type { Server } from "./types";

export interface WorkspaceTab {
  id: string;
  server: Server;
  number: number;
  state: string;
}
export function reorderWorkspace(
  current: Workspaces,
  id: string,
  beforeId?: string | null,
): Workspaces {
  const moving = current.tabs.find((tab) => tab.id === id);
  if (!moving || id === beforeId) return current;
  const tabs = current.tabs.filter((tab) => tab.id !== id);
  const index = beforeId
    ? tabs.findIndex((tab) => tab.id === beforeId)
    : tabs.length;
  tabs.splice(index < 0 ? tabs.length : index, 0, moving);
  return { ...current, tabs };
}
export interface Workspaces {
  tabs: WorkspaceTab[];
  activeId: string;
}
export const emptyWorkspaces = (): Workspaces => ({ tabs: [], activeId: "" });
export function tabTitle(tab: WorkspaceTab) {
  return tab.number === 1
    ? tab.server.name
    : `${tab.server.name} · ${tab.number}`;
}
export function openWorkspace(
  current: Workspaces,
  server: Server,
  forceNew = false,
  newId?: string,
  afterId?: string,
): Workspaces {
  const existing = current.tabs.find((tab) => tab.server.id === server.id);
  if (existing && !forceNew) return { ...current, activeId: existing.id };
  const tab: WorkspaceTab = {
    id: newId ?? crypto.randomUUID(),
    server: structuredClone(server),
    number:
      Math.max(
        0,
        ...current.tabs
          .filter((tab) => tab.server.id === server.id)
          .map((tab) => tab.number),
      ) + 1,
    state: "连接中",
  };
  const tabs = [...current.tabs];
  const index = afterId ? tabs.findIndex((item) => item.id === afterId) : -1;
  tabs.splice(index < 0 ? tabs.length : index + 1, 0, tab);
  return { tabs, activeId: tab.id };
}
export function closeWorkspace(current: Workspaces, id: string): Workspaces {
  const index = current.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return current;
  const tabs = current.tabs.filter((tab) => tab.id !== id);
  return {
    tabs,
    activeId:
      current.activeId === id
        ? tabs[Math.max(0, index - 1)]?.id || ""
        : current.activeId,
  };
}
