export interface Server { id: string; name: string; host: string; port: number; username: string; authType: "password" | "key"; revision: number; hasSecret: boolean }
export interface View { viewId: string; viewType: string; workspaceId: string; serverId: string; title: string; status: string; placement: "left" | "right" | "center" | "bottom" | "detached"; group: number; active?: boolean; mountGeneration: number; stateVersion?: number; failureReason?: string }
export interface PendingOpen { operationId: string; serverId: string; cancelling?: boolean }
export interface Workbench { views: View[]; pendingOpens: PendingOpen[]; activeWorkspaceId: string | null; split: "none" | "horizontal" | "vertical"; sessionsSide: "left" | "right"; sessionsVisible: boolean; activeViewIds?: (string | null)[] }
export interface PluginStatus { pluginId: string; activeVersion?: string; pendingVersion?: string; version?: string; error?: string }
export const emptyWorkbench: Workbench = { views: [], pendingOpens: [], activeWorkspaceId: null, split: "none", sessionsSide: "left", sessionsVisible: true };
