import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { desktop } from "./api";
import { MOUNT_REFRESH_MS, RemoteMounts } from "./remoteMounts";
import type {
  MountSnapshot,
  MountTable,
  RemoteMountApi,
  RemoteMountUsage,
} from "./remoteMounts";

const api: RemoteMountApi = {
  list: (sessionId) => invoke<MountTable>("list_remote_mounts", { sessionId }),
  usage: (sessionId, mountTableVersion, mountKeys) =>
    invoke<RemoteMountUsage[]>("get_remote_mount_usage", {
      sessionId,
      mountTableVersion,
      mountKeys,
    }),
};
export function useRemoteMounts(
  sessionId: string | null,
  active: boolean,
  paths: string[],
  initial?: MountSnapshot,
) {
  const saved = useRef(initial);
  const model = useMemo(
    () => new RemoteMounts(sessionId ?? "", api, saved.current),
    [sessionId],
  );
  const state = useSyncExternalStore(model.subscribe, model.getState);
  const [visible, setVisible] = useState(!document.hidden);
  useEffect(() => {
    const update = () => setVisible(!document.hidden);
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  useEffect(() => {
    model.setVisible(paths);
  }, [model, paths]);
  useEffect(() => {
    const enabled = desktop && !!sessionId && active && visible;
    model.setActive(enabled);
    if (!enabled) return;
    let stopped = false;
    let timer: number | undefined;
    const poll = async () => {
      await model.refresh();
      if (!stopped)
        timer = window.setTimeout(() => void poll(), MOUNT_REFRESH_MS);
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
      model.setActive(false);
    };
  }, [model, sessionId, active, visible]);
  return { model, state };
}
