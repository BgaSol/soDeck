import { invokeDiagnosed as invoke } from "./diagnostics";
import type { ServerWorkspaceView, TerminalTab } from "./workspaces";

export interface WorkspaceList {
  revision: number;
  workspaces: ServerWorkspaceView[];
}
export interface WorkspaceCloseImpact {
  revision: number;
  terminals: number;
  connected: number;
  verifying: boolean;
  blockers: string[];
  independentTasks: number;
}
export type TerminalUpdate =
  | { type: "select"; id: string }
  | { type: "rename"; id: string; title: string | null }
  | { type: "reorder"; ids: string[] }
  | {
      type: "report";
      id: string;
      attempt: number;
      state: TerminalTab["state"];
    };
export const workspaceArgs = (workspace: ServerWorkspaceView) => ({
  workspaceId: workspace.id,
  ownerEpoch: workspace.owner.ownerEpoch,
});
export const workspaceApi = {
  list: () => invoke<WorkspaceList>("list_server_workspaces"),
  open: (
    serverId: string,
    newTerminal = false,
    requestId: string = crypto.randomUUID(),
  ) =>
    invoke<ServerWorkspaceView>("open_server_workspace", {
      serverId,
      newTerminal,
      requestId,
    }),
  createTerminal: (workspace: ServerWorkspaceView) =>
    invoke<ServerWorkspaceView>("create_workspace_terminal", {
      ...workspaceArgs(workspace),
      requestId: crypto.randomUUID(),
    }),
  update: (workspace: ServerWorkspaceView, update: TerminalUpdate) =>
    invoke<ServerWorkspaceView>("update_workspace_terminal", {
      ...workspaceArgs(workspace),
      update,
    }),
  closeTerminal: (workspace: ServerWorkspaceView, terminalId: string) =>
    invoke<void>("close_workspace_terminal", {
      ...workspaceArgs(workspace),
      terminalId,
    }),
  prepareClose: (workspace: ServerWorkspaceView) =>
    invoke<WorkspaceCloseImpact>(
      "prepare_workspace_close",
      workspaceArgs(workspace),
    ),
  close: (workspace: ServerWorkspaceView, revision: number) =>
    invoke<void>("close_server_workspace", {
      ...workspaceArgs(workspace),
      revision,
    }),
};
