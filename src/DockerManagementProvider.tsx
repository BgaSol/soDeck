import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { desktop } from "./api";
import { DockerManagementStore } from "./dockerManagement";
const DockerContext = createContext<DockerManagementStore | null>(null);
export function DockerManagementProvider({
  contextId,
  children,
}: {
  contextId: string | null;
  children: ReactNode;
}) {
  const saved = useRef<DockerManagementStore | null>(null);
  if (!saved.current || (contextId && saved.current.contextId !== contextId))
    saved.current = new DockerManagementStore(contextId ?? "");
  const store = saved.current;
  useEffect(() => {
    if (desktop && contextId) void store.start();
    return () => store.stop();
  }, [store, contextId]);
  return (
    <DockerContext.Provider value={store}>{children}</DockerContext.Provider>
  );
}
export function useDockerManagement() {
  const store = useContext(DockerContext);
  if (!store) throw new Error("Docker 管理必须位于工作区内");
  const state = useSyncExternalStore(store.subscribe, store.getState);
  return { store, state };
}
export function useDockerDetail(containerId: string | null, active: boolean) {
  const { store } = useDockerManagement();
  useEffect(() => {
    const update = () =>
      store.setTarget(active && !document.hidden ? containerId : null);
    update();
    document.addEventListener("visibilitychange", update);
    return () => {
      document.removeEventListener("visibilitychange", update);
      store.setTarget(null);
    };
  }, [store, containerId, active]);
}
