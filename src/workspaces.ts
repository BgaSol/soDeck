import type { Server } from "./types";
import type { ConnectionView } from "./types";
import type { ToolContextRef } from "./toolContext";

export interface TerminalTab {
  id: string;
  number: number;
  title: string | null;
  attempt: number;
  connectionId: string | null;
  state:
    | "queued"
    | "connecting"
    | "authenticating"
    | "connected"
    | "waiting"
    | "disconnected"
    | "failed";
}
export interface ServerWorkspaceView {
  id: string;
  server: Server;
  owner: { label: string; generation: number; ownerEpoch: number };
  revision: number;
  lifecycle: "active" | "moving" | "closing";
  terminals: TerminalTab[];
  activeTerminalId: string | null;
  nextTerminalNumber: number;
  tools: {
    context: ToolContextRef | null;
    state: "unavailable" | "verifying" | "available" | "error";
    connection: ConnectionView | null;
    configurationChanged: boolean;
  };
}
export const terminalStateLabel = (state: TerminalTab["state"]) =>
  ({
    queued: "等待连接",
    connecting: "连接中",
    authenticating: "等待验证",
    connected: "已连接",
    waiting: "等待重连",
    disconnected: "已断开",
    failed: "连接失败",
  })[state];
export const terminalTitle = (tab: TerminalTab) =>
  tab.title || `终端 ${tab.number}`;
export function projectWorkspace(view: ServerWorkspaceView): WorkspaceTab {
  const state = view.terminals.some((t) => t.state === "connected")
    ? "已连接"
    : view.terminals.some((t) =>
          ["queued", "connecting", "authenticating"].includes(t.state),
        )
      ? "连接中"
      : view.terminals.some((t) => t.state === "failed")
        ? "连接失败"
        : "已断开";
  return { ...view, number: 1, state };
}

export interface WorkspaceTab extends ServerWorkspaceView {
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
  return tab.server.name;
}
export function openWorkspace(
  current: Workspaces,
  server: Server,
  _forceNew = false,
  newId?: string,
  afterId?: string,
): Workspaces {
  const existing = current.tabs.find((tab) => tab.server.id === server.id);
  if (existing) return { ...current, activeId: existing.id };
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
    owner: { label: "main", generation: 0, ownerEpoch: 0 },
    revision: 0,
    lifecycle: "active",
    terminals: [],
    activeTerminalId: null,
    nextTerminalNumber: 1,
    tools: {
      context: null,
      state: "unavailable",
      connection: null,
      configurationChanged: false,
    },
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
