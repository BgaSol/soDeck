import { useContext, useMemo, useSyncExternalStore } from "react";
import { MonitorContext, useMonitorPaths } from "./MonitorProvider";
import type { MountSnapshot, MountState } from "./remoteMounts";

const emptyState: MountState = {
  sessionId: "",
  usage: {},
  fetchedAt: 0,
  loading: false,
  error: "",
};
const emptySubscribe = () => () => {};
const getEmptyState = () => emptyState;

// The empty workspace has no provider; connected workspaces use the shared backend cache.
export function useRemoteMounts(
  sessionId: string | null,
  active: boolean,
  paths: string[],
  _initial?: MountSnapshot,
) {
  const store = useContext(MonitorContext);
  useMonitorPaths(store, paths, active && !!sessionId);
  const state = useSyncExternalStore(
    store?.subscribeMounts ?? emptySubscribe,
    store?.getMounts ?? getEmptyState,
  );
  const model = useMemo(
    () =>
      store
        ? {
            refresh: (_force = false) => store.refresh("mounts"),
            snapshot: () => store.getMounts(),
          }
        : { refresh: async (_force = false) => {}, snapshot: getEmptyState },
    [store],
  );
  return { model, state };
}
