import { useState } from "react";
import { workspaceApi, type WorkspaceCloseImpact } from "./workspaceApi";
import type { WorkspaceTab } from "./workspaces";
import { WorkspaceCloseDialog } from "./WorkspaceCloseDialog";
import { errorOf, type AppError } from "./types";

export function useWorkspaceClose(
  onError: (e: AppError) => void,
  afterClose?: () => void,
) {
  const [request, setRequest] = useState<{
    workspace: WorkspaceTab;
    impact: WorkspaceCloseImpact;
  } | null>(null);
  async function close(workspace: WorkspaceTab) {
    try {
      if (document.querySelector("dialog[open]"))
        throw new Error("请先完成或取消当前操作，再关闭服务器工作区。");
      const impact = await workspaceApi.prepareClose(workspace);
      if (!impact.terminals && !impact.verifying && !impact.blockers.length) {
        await workspaceApi.close(workspace, impact.revision);
        afterClose?.();
      } else setRequest({ workspace, impact });
    } catch (e) {
      onError(errorOf(e));
    }
  }
  return {
    close,
    dialog: request && (
      <WorkspaceCloseDialog
        title={`关闭服务器：${request.workspace.server.name}`}
        details={`将关闭 ${request.impact.terminals} 个终端（${request.impact.connected} 个已有连接），并释放服务器工具。独立编辑器、容器窗口和已启动的后台任务继续保留。${request.impact.verifying ? "正在进行的工具验证也会取消。" : ""}`}
        blockers={request.impact.blockers}
        onClose={() => setRequest(null)}
        onConfirm={async () => {
          try {
            await workspaceApi.close(
              request.workspace,
              request.impact.revision,
            );
            afterClose?.();
          } catch (e) {
            const impact = await workspaceApi.prepareClose(request.workspace);
            setRequest({ ...request, impact });
            throw e;
          }
        }}
      />
    ),
  };
}
