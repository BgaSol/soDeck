import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { desktop } from "./api";
import {
  MonitorStore,
  type MonitorApi,
  type MonitorEvent,
} from "./resourceMonitoring";
import type { DockerInterest } from "./serverMonitoring";

export const monitorApi: MonitorApi = {
  subscribe: (sessionId, interest, receive) =>
    invoke("subscribe_monitor", {
      sessionId,
      interest,
      events: new Channel<MonitorEvent>(receive),
    }),
  unsubscribe: (subscriptionId) =>
    invoke("unsubscribe_monitor", { subscriptionId }),
  update: (subscriptionId, interest) =>
    invoke("update_monitor_interest", { subscriptionId, interest }),
  history: (subscriptionId, minutes) =>
    invoke("get_monitor_history", { subscriptionId, minutes }),
  refresh: (subscriptionId, scope) =>
    invoke("refresh_monitor", { subscriptionId, scope }),
};
export const MonitorContext = createContext<MonitorStore | null>(null);

export function MonitorProvider({
  sessionId,
  active,
  children,
}: {
  sessionId: string | null;
  active: boolean;
  children: ReactNode;
}) {
  const saved = useRef<MonitorStore | null>(null);
  if (!saved.current || (sessionId && saved.current.sessionId !== sessionId))
    saved.current = new MonitorStore(sessionId ?? "", monitorApi);
  const store = saved.current;
  useEffect(() => {
    const visible = () => store.setVisible(active && !document.hidden);
    visible();
    document.addEventListener("visibilitychange", visible);
    return () => document.removeEventListener("visibilitychange", visible);
  }, [store, active]);
  useEffect(() => {
    if (desktop && sessionId) void store.start();
    return () => store.stop();
  }, [store, sessionId]);
  return (
    <MonitorContext.Provider value={store}>{children}</MonitorContext.Provider>
  );
}
export function useMonitor() {
  const store = useContext(MonitorContext);
  if (!store) throw new Error("资源监控必须位于工作区内");
  const state = useSyncExternalStore(store.subscribe, store.getState);
  return { store, state };
}
export function useMonitorPaths(
  store: MonitorStore | null,
  paths: string[],
  active: boolean,
) {
  const id = useId();
  useEffect(() => {
    if (!store) return;
    store.setPaths(id, active ? paths : []);
  }, [store, id, paths, active]);
  useEffect(() => () => store?.removePaths(id), [store, id]);
}

export function useDockerInterest(interest: DockerInterest) {
  const { store } = useMonitor();
  const id = useId();
  useEffect(() => {
    store.setDockerInterest(id, interest);
  }, [store, id, interest.summary, interest.containers, interest.images]);
  useEffect(() => () => store.removeDockerInterest(id), [store, id]);
}
