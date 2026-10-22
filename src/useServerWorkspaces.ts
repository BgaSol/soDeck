import {
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { workspaceApi, type WorkspaceList } from "./workspaceApi";
import {
  projectWorkspace,
  type Workspaces,
  type WorkspaceTab,
} from "./workspaces";
import { errorOf, type AppError } from "./types";

export function useServerWorkspaces(
  setWorkspaces: Dispatch<SetStateAction<Workspaces>>,
  onError: (e: AppError) => void,
) {
  const [all, setAll] = useState<WorkspaceTab[]>([]);
  const revision = useRef(-1);
  useEffect(() => {
    if (!desktop) return;
    let live = true;
    const off: (() => void)[] = [];
    const window = getCurrentWindow();
    const apply = (snapshot: WorkspaceList) => {
      if (!live || snapshot.revision <= revision.current) return;
      revision.current = snapshot.revision;
      const global = snapshot.workspaces.map(projectWorkspace);
      setAll(global);
      setWorkspaces((current) => {
        // Incoming components are installed from the transfer snapshot before
        // ownership commits. Keep both sides mounted for the entire transaction.
        const tabs = current.tabs.flatMap((previous) => {
          const latest = global.find((w) => w.id === previous.id);
          if (!latest) return [];
          if (latest.lifecycle === "moving")
            return [latest.owner.label === window.label ? latest : previous];
          return latest.owner.label === window.label ? [latest] : [];
        });
        for (const tab of global)
          if (
            tab.owner.label === window.label &&
            tab.lifecycle === "active" &&
            !tabs.some((t) => t.id === tab.id)
          )
            tabs.push(tab);
        const previousIndex = current.tabs.findIndex(
          (t) => t.id === current.activeId,
        );
        return {
          tabs,
          activeId: tabs.some((t) => t.id === current.activeId)
            ? current.activeId
            : current.activeId && previousIndex >= 0
              ? (tabs[Math.max(0, previousIndex - 1)]?.id ?? "")
              : "",
        };
      });
    };
    void (async () => {
      const changed = await window.listen<WorkspaceList>(
        "server-workspaces-changed",
        (e) => apply(e.payload),
      );
      if (!live) {
        changed();
        return;
      }
      off.push(changed);
      const focused = await window.listen<string>(
        "server-workspace-focus",
        (e) => {
          setWorkspaces((current) => ({ ...current, activeId: e.payload }));
          void workspaceApi
            .list()
            .then(apply)
            .catch((e) => onError(errorOf(e)));
        },
      );
      if (!live) {
        focused();
        return;
      }
      off.push(focused);
      apply(await workspaceApi.list());
    })().catch((e) => onError(errorOf(e)));
    const recover = () => {
      if (!document.hidden)
        void workspaceApi
          .list()
          .then(apply)
          .catch((e) => onError(errorOf(e)));
    };
    document.addEventListener("visibilitychange", recover);
    const timer = setInterval(recover, 5000);
    return () => {
      live = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", recover);
      off.forEach((fn) => fn());
      revision.current = -1;
    };
  }, [setWorkspaces, onError]);
  return all;
}
