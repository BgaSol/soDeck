import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { transferActive, transferApi, type TransferTask } from "./transferApi";
import { sortTransfers } from "./transferPresentation";
import { errorOf } from "./types";

export function useTransferTasks() {
  const [tasks, setTasks] = useState<TransferTask[]>([]);
  const [rates, setRates] = useState<Record<string, number>>({});
  const [error, setError] = useState("");
  const [completion, setCompletion] = useState("");
  const refreshRef = useRef<() => Promise<void>>(async () => {});
  const refresh = useCallback(() => refreshRef.current(), []);

  useEffect(() => {
    if (!desktop) return;
    let live = true;
    let queued = false;
    let flight: Promise<void> | null = null;
    let previous: TransferTask[] = [];
    let completionTimer: ReturnType<typeof setTimeout> | undefined;
    const samples = new Map<
      string,
      { at: number; bytes: number; run: string; rate: number }
    >();
    const request = (): Promise<void> => {
      queued = true;
      if (flight) return flight;
      flight = (async () => {
        do {
          queued = false;
          try {
            const next = sortTransfers(await transferApi.list());
            if (!live) return;
            const now = performance.now();
            const nextRates: Record<string, number> = {};
            for (const task of next) {
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
            const ids = new Set(next.map((task) => task.id));
            for (const id of samples.keys())
              if (!ids.has(id)) samples.delete(id);
            const hadActive = previous.filter(transferActive);
            if (next.some(transferActive)) {
              clearTimeout(completionTimer);
              setCompletion("");
            } else if (hadActive.length) {
              const finished = hadActive.map((old) =>
                next.find(
                  (task) => task.id === old.id && task.runId === old.runId,
                ),
              );
              if (finished.every(Boolean)) {
                setCompletion(
                  finished.every((task) => task?.status === "completed")
                    ? "传输已完成"
                    : "传输已结束",
                );
                clearTimeout(completionTimer);
                completionTimer = setTimeout(() => {
                  if (live) setCompletion("");
                }, 2500);
              }
            }
            previous = next;
            setTasks(next);
            setRates(nextRates);
            setError("");
          } catch (e) {
            if (live) setError(errorOf(e).message);
          }
        } while (live && queued);
      })().finally(() => {
        flight = null;
      });
      return flight;
    };
    refreshRef.current = request;
    const started = () => {
      void request();
    };
    // A slow request must not keep itself alive forever through the polling timer.
    // Explicit events/actions still queue a follow-up snapshot while it is running.
    const timer = setInterval(() => {
      if (!flight) void request();
    }, 900);
    window.addEventListener("workspace-transfer-started", started);
    window.addEventListener("workspace-download-started", started);
    let off: (() => void) | undefined;
    void getCurrentWindow()
      .listen("transfer-updated", started)
      .then((cleanup) => {
        if (live) off = cleanup;
        else cleanup();
      })
      .catch(() => {
        /* Polling remains available if event registration fails. */
      });
    void request();
    return () => {
      live = false;
      refreshRef.current = async () => {};
      clearInterval(timer);
      clearTimeout(completionTimer);
      off?.();
      window.removeEventListener("workspace-transfer-started", started);
      window.removeEventListener("workspace-download-started", started);
    };
  }, []);

  return { tasks, rates, error, completion, refresh };
}
