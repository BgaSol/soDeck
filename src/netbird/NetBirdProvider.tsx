import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { desktop } from "../api";
import { NetBirdStore } from "./netbirdStore";
const Context = createContext<NetBirdStore | null>(null);
export function NetBirdProvider({
  contextId,
  children,
  suspended = false,
}: {
  contextId: string | null;
  children: ReactNode;
  suspended?: boolean;
}) {
  const saved = useRef<NetBirdStore | null>(null);
  if (!saved.current || (contextId && contextId !== saved.current.contextId))
    saved.current = new NetBirdStore(contextId ?? "");
  const store = saved.current;
  useEffect(() => {
    if (desktop && contextId && !suspended) void store.start();
    return () => store.stop();
  }, [contextId, store, suspended]);
  return <Context.Provider value={store}>{children}</Context.Provider>;
}
export function useNetBird() {
  const store = useContext(Context);
  if (!store) throw new Error("NetBird 必须位于工作区内");
  const state = useSyncExternalStore(store.subscribe, store.getState);
  return { store, state };
}
