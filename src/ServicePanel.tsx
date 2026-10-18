import { useId, type ReactNode } from "react";
import { Activity, Boxes, Network } from "lucide-react";
import type { ResourcePreferences } from "./resourcePresentation";

export function ServicePanel({
  preferences,
  onChange,
  monitor,
  docker,
  netbird,
}: {
  preferences: ResourcePreferences;
  onChange: (next: ResourcePreferences) => void;
  monitor: ReactNode;
  docker: ReactNode;
  netbird: ReactNode;
}) {
  const id = useId();
  const tabs = ["monitor", "docker", "netbird"] as const;
  return (
    <aside className="resource-service-panel" aria-label="服务器监控与服务">
      <div className="inspector-navigation">
        <div className="resource-tabs" role="tablist" aria-label="服务器侧栏">
          {tabs.map((tab, index) => (
            <button
              key={tab}
              type="button"
              role="tab"
              id={`${id}-${tab}`}
              aria-controls={`${id}-${tab}-panel`}
              aria-selected={preferences.tab === tab}
              tabIndex={preferences.tab === tab ? 0 : -1}
              onClick={() => onChange({ ...preferences, tab })}
              onKeyDown={(e) => {
                if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key))
                  return;
                e.preventDefault();
                const target =
                  e.key === "Home"
                    ? 0
                    : e.key === "End"
                      ? tabs.length - 1
                      : (index +
                          (e.key === "ArrowRight" ? 1 : -1) +
                          tabs.length) %
                        tabs.length;
                onChange({ ...preferences, tab: tabs[target] });
                document.getElementById(`${id}-${tabs[target]}`)?.focus();
              }}
            >
              {tab === "monitor" ? (
                <Activity size={14} />
              ) : tab === "docker" ? (
                <Boxes size={14} />
              ) : (
                <Network size={14} />
              )}{" "}
              {tab === "monitor"
                ? "监控"
                : tab === "docker"
                  ? "Docker"
                  : "NetBird"}
            </button>
          ))}
        </div>
      </div>
      <div
        role="tabpanel"
        id={`${id}-monitor-panel`}
        aria-labelledby={`${id}-monitor`}
        hidden={preferences.tab !== "monitor"}
      >
        {monitor}
      </div>
      <div
        role="tabpanel"
        id={`${id}-docker-panel`}
        aria-labelledby={`${id}-docker`}
        hidden={preferences.tab !== "docker"}
      >
        {docker}
      </div>
      <div
        role="tabpanel"
        id={`${id}-netbird-panel`}
        aria-labelledby={`${id}-netbird`}
        hidden={preferences.tab !== "netbird"}
      >
        {netbird}
      </div>
    </aside>
  );
}
