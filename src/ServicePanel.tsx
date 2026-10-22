import type { ReactNode } from "react";
import { PanelRightClose } from "lucide-react";
import { serviceToolNames } from "./WorkspaceToolRail";
import type { ResourcePreferences } from "./resourcePresentation";
export function ServicePanel({
  preferences,
  onClose,
  monitor,
  docker,
  netbird,
}: {
  preferences: ResourcePreferences;
  onClose: () => void;
  monitor: ReactNode;
  docker: ReactNode;
  netbird: ReactNode;
}) {
  return (
    <aside className="resource-service-panel" aria-label="服务器监控与服务">
      <header className="workspace-tool-heading">
        <h2>{serviceToolNames[preferences.tab]}</h2>
        <button
          className="icon-button"
          aria-label="收起服务面板"
          title="收起服务面板"
          onClick={onClose}
        >
          <PanelRightClose size={16} />
        </button>
      </header>
      <div
        hidden={preferences.tab !== "monitor"}
        inert={preferences.tab !== "monitor"}
      >
        {monitor}
      </div>
      <div
        hidden={preferences.tab !== "docker"}
        inert={preferences.tab !== "docker"}
      >
        {docker}
      </div>
      <div
        hidden={preferences.tab !== "netbird"}
        inert={preferences.tab !== "netbird"}
      >
        {netbird}
      </div>
    </aside>
  );
}
