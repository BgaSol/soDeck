import { Activity, Boxes, Folder, Network } from "lucide-react";
import type { ServiceTool } from "./workspaceTools";
export const serviceToolNames = {
  docker: "Docker",
  monitor: "服务器状态",
  netbird: "NetBird",
};
export function WorkspaceToolRail({
  side,
  id,
  filesOpen,
  servicesOpen,
  selected,
  onFiles,
  onService,
}: {
  side: "left" | "right";
  id: string;
  filesOpen: boolean;
  servicesOpen: boolean;
  selected: ServiceTool;
  onFiles: () => void;
  onService: (tool: ServiceTool) => void;
}) {
  const tools =
    side === "left"
      ? [{ key: "files" as const, name: "远程文件", Icon: Folder }]
      : [
          {
            key: "docker" as const,
            name: serviceToolNames.docker,
            Icon: Boxes,
          },
          {
            key: "monitor" as const,
            name: serviceToolNames.monitor,
            Icon: Activity,
          },
          {
            key: "netbird" as const,
            name: serviceToolNames.netbird,
            Icon: Network,
          },
        ];
  return (
    <nav
      className={`workspace-tool-rail rail-${side}`}
      aria-label={side === "left" ? "文件工具" : "服务工具"}
    >
      {tools.map(({ key, name, Icon }) => {
        const expanded =
          key === "files" ? filesOpen : servicesOpen && selected === key;
        return (
          <div className="workspace-tool-entry" key={key}>
            <button
              type="button"
              className="workspace-tool-button"
              data-tool={key}
              aria-label={name}
              aria-expanded={expanded}
              aria-controls={`${id}-${key === "files" ? "files" : "services"}`}
              onClick={() => (key === "files" ? onFiles() : onService(key))}
            >
              <Icon size={20} strokeWidth={1.7} aria-hidden="true" />
            </button>
            <span className="workspace-tool-tooltip" aria-hidden="true">
              {name}
              <small>{expanded ? "点击收起" : "点击打开"}</small>
            </span>
          </div>
        );
      })}
    </nav>
  );
}
