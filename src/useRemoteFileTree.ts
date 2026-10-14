import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import { api } from "./api";
import { RemoteFileTree } from "./remoteFileTree";
import type {
  RemoteFileTreeSnapshot,
  TerminalDirectory,
} from "./remoteFileTree";

export function useRemoteFileTree(
  sessionId: string | null,
  active: boolean,
  initial?: RemoteFileTreeSnapshot,
  terminalCwd?: TerminalDirectory,
) {
  const initialSnapshot = useRef(initial);
  const model = useMemo(
    () => new RemoteFileTree(sessionId, api, initialSnapshot.current),
    [sessionId],
  );
  const state = useSyncExternalStore(model.subscribe, model.getState);
  useEffect(() => {
    if (terminalCwd) model.acceptTerminalDirectory(terminalCwd, active);
    if (active) model.activate();
  }, [model, active, terminalCwd]);
  return { model, state };
}
