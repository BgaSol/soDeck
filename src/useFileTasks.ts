import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import {
  fileTaskApi,
  type FileTask,
  type TaskFilter,
  type TaskSummary,
} from "./fileTaskApi";
import { errorOf } from "./types";

const emptySummary: TaskSummary = {
  total: 0,
  active: 0,
  attention: 0,
  completed: 0,
};
export function useFileTasks() {
  const [tasks, setTasks] = useState<FileTask[]>([]);
  const [summary, setSummary] = useState(emptySummary);
  const [rates, setRates] = useState<Record<string, number>>({});
  const [error, setError] = useState("");
  const [completion, setCompletion] = useState("");
  const [filter, setFilter] = useState<TaskFilter>("all");
  const [limit, setLimit] = useState(50);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const refreshRef = useRef<() => Promise<void>>(async () => {});
  const query = useRef({ filter, limit });
  query.current = { filter, limit };
  const refresh = useCallback(() => refreshRef.current(), []);

  useEffect(() => {
    if (!desktop) return;
    let live = true;
    let queued = false;
    let flight: Promise<void> | null = null;
    let previousActive = 0;
    let completionTimer: ReturnType<typeof setTimeout> | undefined;
    const samples = new Map<
      string,
      { at: number; bytes: number; run: string; rate: number }
    >();
    function announce(message: string) {
      clearTimeout(completionTimer);
      setCompletion(message);
      completionTimer = setTimeout(() => {
        if (live) setCompletion("");
      }, 3000);
    }
    const request = (): Promise<void> => {
      queued = true;
      if (flight) return flight;
      setLoading(true);
      flight = (async () => {
        do {
          queued = false;
          const current = query.current;
          try {
            const next: FileTask[] = [];
            const seen = new Set<string>();
            const cursors = new Set<string>();
            let cursor: string | null = null;
            let page;
            do {
              page = await fileTaskApi.list(
                current.filter,
                cursor,
                Math.min(1000, current.limit - next.length),
              );
              for (const item of page.tasks) {
                if (!seen.has(item.task.id)) {
                  next.push(item);
                  seen.add(item.task.id);
                }
              }
              cursor = page.nextCursor;
              if (cursor && cursors.has(cursor)) break;
              if (cursor) cursors.add(cursor);
            } while (
              live &&
              cursor &&
              next.length < current.limit &&
              query.current.filter === current.filter &&
              query.current.limit === current.limit
            );
            if (!live) return;
            if (
              query.current.filter !== current.filter ||
              query.current.limit !== current.limit
            ) {
              queued = true;
              continue;
            }
            const now = performance.now();
            const nextRates: Record<string, number> = {};
            for (const item of next) {
              if (item.category !== "transfer") continue;
              const task = item.task;
              const before = samples.get(task.id);
              let rate = 0;
              if (
                task.status === "running" &&
                before?.run === task.runId &&
                task.bytesDone >= before.bytes
              ) {
                if (now - before.at < 250) {
                  nextRates[task.id] = before.rate;
                  continue;
                }
                rate =
                  ((task.bytesDone - before.bytes) * 1000) / (now - before.at);
              }
              nextRates[task.id] = rate;
              samples.set(task.id, {
                at: now,
                bytes: task.bytesDone,
                run: task.runId,
                rate,
              });
            }
            const ids = new Set(next.map((t) => t.task.id));
            for (const id of samples.keys())
              if (!ids.has(id)) samples.delete(id);
            if (previousActive && !page.summary.active)
              announce("文件任务已结束");
            previousActive = page.summary.active;
            setSummary(page.summary);
            setTasks(next);
            setRates(nextRates);
            setHasMore(!!cursor);
            setError("");
          } catch (e) {
            if (live) setError(errorOf(e).message);
          }
        } while (live && queued);
      })().finally(() => {
        flight = null;
        if (live) setLoading(false);
      });
      return flight;
    };
    refreshRef.current = request;
    const started = () => {
      void request();
    };
    const deletionStarted = () => {
      announce("已开始删除 · 可在此查看");
      void request();
    };
    const timer = setInterval(() => {
      if (!flight) void request();
    }, 900);
    window.addEventListener("workspace-transfer-started", started);
    window.addEventListener("workspace-download-started", started);
    window.addEventListener("workspace-file-task-started", deletionStarted);
    const off: (() => void)[] = [];
    for (const event of ["file-task-updated", "transfer-updated"]) {
      void getCurrentWindow()
        .listen(event, started)
        .then((cleanup) => {
          if (live) {
            off.push(cleanup);
            void request();
          } else cleanup();
        })
        .catch(() => {
          /* Polling is the fallback. */
        });
    }
    void request();
    return () => {
      live = false;
      refreshRef.current = async () => {};
      clearInterval(timer);
      clearTimeout(completionTimer);
      off.forEach((cleanup) => cleanup());
      window.removeEventListener("workspace-transfer-started", started);
      window.removeEventListener("workspace-download-started", started);
      window.removeEventListener(
        "workspace-file-task-started",
        deletionStarted,
      );
    };
  }, []);

  useEffect(() => {
    void refresh();
  }, [filter, limit, refresh]);
  return {
    tasks,
    summary,
    rates,
    error,
    completion,
    refresh,
    filter,
    setFilter: (value: TaskFilter) => {
      setFilter(value);
      setLimit(50);
    },
    hasMore,
    loading,
    loadMore: () => setLimit((n) => n + 50),
  };
}
