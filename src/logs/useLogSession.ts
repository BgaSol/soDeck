import { useCallback, useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import {
  logApi,
  type LogInfo,
  type LogEvent,
  type CollectionState,
  type LogTimeline,
} from "../logApi";
import { errorOf } from "../types";
const emptyTimeline: LogTimeline = {
  events: [],
  boots: [],
  nextCursor: 0,
  hasMore: false,
};
export function useLogSession() {
  const [info, setInfo] = useState<LogInfo | null>(null),
    [collection, setCollection] = useState<CollectionState>({
      phase: "connecting",
    }),
    [error, setError] = useState("");
  const [revision, setRevision] = useState(0),
    [changedFrom, setChangedFrom] = useState<number | null>(null),
    [timeline, setTimeline] = useState(emptyTimeline);
  const mounted = useRef(false),
    epoch = useRef(0),
    collector = useRef(""),
    dirty = useRef(false),
    changed = useRef<number | null>(null),
    busy = useRef(false),
    starting = useRef(false),
    timelineBusy = useRef(false),
    timelineDirty = useRef(true),
    cursor = useRef(0),
    canCollect = useRef(false);
  const refreshTimeline = useCallback(async () => {
    if (timelineBusy.current) return;
    timelineBusy.current = true;
    timelineDirty.current = false;
    try {
      do {
        const page = await logApi.timeline(cursor.current);
        if (!mounted.current) return;
        cursor.current = page.nextCursor;
        setTimeline((old) => ({
          ...page,
          events: [
            ...new Map(
              [...old.events, ...page.events].map((e) => [e.id, e]),
            ).values(),
          ].slice(-2000),
        }));
        if (!page.hasMore) break;
      } while (mounted.current);
    } catch (e) {
      if (mounted.current) setError("运行事件暂不可用：" + errorOf(e).message);
    } finally {
      timelineBusy.current = false;
    }
  }, []);
  const refresh = useCallback(async () => {
    const value = await logApi.info();
    if (mounted.current) {
      setInfo(value);
      canCollect.current = value.canCollect;
    }
    return value;
  }, []);
  const start = useCallback(async () => {
    if (starting.current) return;
    starting.current = true;
    const token = ++epoch.current;
    setError("");
    setCollection({ phase: "connecting" });
    const output = new Channel<LogEvent>();
    output.onmessage = (event) => {
      if (!mounted.current || token !== epoch.current) return;
      if (event.type === "state")
        setCollection((old) =>
          old.collectorId === event.collectorId &&
          (old.eventRevision ?? -1) >= (event.eventRevision ?? 0)
            ? old
            : event,
        );
      else if (event.type === "saved") {
        dirty.current = true;
        if (event.changedFrom !== null)
          changed.current =
            changed.current === null
              ? event.changedFrom
              : Math.min(changed.current, event.changedFrom);
      } else if (event.type === "lifecycle") timelineDirty.current = true;
      else if (event.type === "notice") setError(event.error);
      else {
        dirty.current = true;
        setCollection((old) =>
          old.phase === "failed"
            ? old
            : {
                ...old,
                phase: event.error ? "failed" : "offline",
                error: event.error,
              },
        );
      }
    };
    try {
      const id = await logApi.start(output);
      if (mounted.current && token === epoch.current) collector.current = id;
      else await logApi.stop(id);
    } catch (e) {
      if (mounted.current && token === epoch.current)
        setCollection({ phase: "failed", error: errorOf(e).message });
    } finally {
      starting.current = false;
    }
  }, []);
  const retry = useCallback(async () => {
    try {
      if (!(await logApi.retry())) await start();
    } catch (e) {
      setError(errorOf(e).message);
    }
  }, [start]);
  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    void refresh()
      .then((value) => {
        if (disposed) return;
        if (value.canCollect) void start();
        else setCollection({ phase: "offline" });
        void refreshTimeline();
      })
      .catch((e) => {
        if (!disposed) setError(errorOf(e).message);
      });
    const timer = window.setInterval(() => {
      if (timelineDirty.current) void refreshTimeline();
      if (!dirty.current || busy.current) return;
      dirty.current = false;
      busy.current = true;
      const change = changed.current;
      changed.current = null;
      void refresh()
        .then(() => {
          if (!disposed) {
            setChangedFrom(change);
            setRevision((r) => r + 1);
          }
        })
        .catch((e) => {
          if (!disposed) setError(errorOf(e).message);
        })
        .finally(() => {
          busy.current = false;
        });
    }, 300);
    const focus = () => {
      const before = canCollect.current;
      void refresh()
        .then((value) => {
          if (!disposed && value.canCollect && !before) void start();
        })
        .catch((e) => {
          if (!disposed) setError(errorOf(e).message);
        });
    };
    window.addEventListener("focus", focus);
    return () => {
      disposed = true;
      mounted.current = false;
      epoch.current++;
      clearInterval(timer);
      window.removeEventListener("focus", focus);
      if (collector.current)
        void logApi.stop(collector.current).catch(() => {});
    };
  }, [refresh, refreshTimeline, start]);
  return {
    info,
    collection,
    error,
    setError,
    revision,
    changedFrom,
    timeline,
    retry,
    refresh,
  };
}
