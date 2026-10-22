import { useEffect, useRef, useSyncExternalStore } from "react";
import { api } from "./api";
import { RemoteFileTree } from "./remoteFileTree";
import type {
  RemoteFileTreeSnapshot,
  TerminalDirectory,
} from "./remoteFileTree";

export function useRemoteFileTree(
  contextId: string | null,
  active: boolean,
  initial?: RemoteFileTreeSnapshot,
  terminalCwd?: TerminalDirectory,
  contextIdentity?: string,
) {
  const previous = useRef<{
    model: RemoteFileTree;
    contextId: string | null;
    identity?: string;
  } | null>(null);
  if (!previous.current || previous.current.contextId !== contextId) {
    const sameRoute =
      !!contextIdentity && previous.current?.identity === contextIdentity;
    const saved = sameRoute
      ? { ...previous.current!.model.snapshot(), contextId }
      : initial;
    if (sameRoute && saved)
      for (const entry of Object.values(saved.directories)) entry.stale = true;
    previous.current = {
      model: new RemoteFileTree(contextId, api, saved),
      contextId,
      identity: contextIdentity,
    };
  }
  const model = previous.current.model;
  const state = useSyncExternalStore(model.subscribe, model.getState);
  useEffect(() => {
    model.acceptTerminalDirectory(terminalCwd ?? null, active);
    if (active) model.activate();
  }, [model, active, terminalCwd]);
  return { model, state };
}
