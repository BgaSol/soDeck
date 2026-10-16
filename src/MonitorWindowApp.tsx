import { useEffect, useState } from "react";
import { Activity } from "lucide-react";
import { MonitorContext } from "./MonitorProvider";
import { MonitorStore } from "./resourceMonitoring";
import { ResourcePanel } from "./ResourcePanel";
import { normalizeResourcePreferences } from "./resourcePresentation";
import { monitorWindowApi, windowMonitorApi } from "./monitorWindowApi";
import { WindowTitlebar } from "./WindowTitlebar";
import { errorOf } from "./types";

export function MonitorWindowApp() {
  const [store] = useState(
    () => new MonitorStore("monitor-window", windowMonitorApi),
  );
  const [serverName, setServerName] = useState("服务器监控");
  const [preferences, setPreferences] = useState(() =>
    normalizeResourcePreferences(),
  );
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false;
    void monitorWindowApi
      .info()
      .then((info) => {
        if (disposed) return;
        setServerName(info.serverName);
        setPreferences(normalizeResourcePreferences(info.preferences));
        void store.start();
      })
      .catch((e) => {
        if (!disposed) setError(errorOf(e).message);
      });
    const visible = () => store.setVisible(!document.hidden);
    visible();
    document.addEventListener("visibilitychange", visible);
    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", visible);
      store.stop();
    };
  }, [store]);
  return (
    <div className="monitor-window-app">
      <WindowTitlebar
        leading={<Activity size={18} />}
        title={`${serverName} · 监控`}
        onError={(e) => setError(errorOf(e).message)}
      />
      {error && (
        <p className="connection-error" role="alert">
          {error}
        </p>
      )}
      <MonitorContext.Provider value={store}>
        <ResourcePanel
          serverName={serverName}
          active
          preferences={preferences}
          onChange={setPreferences}
          standalone
          onOpenMetric={(metric) =>
            setPreferences((p) => ({ ...p, selectedMetric: metric }))
          }
        />
      </MonitorContext.Provider>
    </div>
  );
}
