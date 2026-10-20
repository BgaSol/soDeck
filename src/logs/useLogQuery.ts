import { useCallback, useEffect, useRef, useState } from "react";
import {
  logApi,
  type LogInfo,
  type LogPage,
  type LogQuery,
  type LogRow,
  type LogSnapshot,
} from "../logApi";
import { errorOf } from "../types";
import { bounded, mergeRows, PAGE_SIZE } from "./model";
export type QueryMode = "live" | "snapshot";
interface PageState {
  rows: LogRow[];
  older: boolean;
  newer: boolean;
  before: number;
  scannedThrough: number;
}
const empty = (): PageState => ({
  rows: [],
  older: false,
  newer: false,
  before: 0,
  scannedThrough: 0,
});
export function useLogQuery(
  info: LogInfo | null,
  filters: LogQuery,
  mode: QueryMode,
  follow: boolean,
  revision: number,
  changedFrom: number | null,
) {
  const [page, setPage] = useState<PageState>(empty),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [scanned, setScanned] = useState(0),
    [snapshot, setSnapshot] = useState<LogSnapshot | null>(null),
    [context, setContext] = useState<number | null>(null),
    [appliedKey, setAppliedKey] = useState<string | null>(null);
  const s = useRef({
    alive: false,
    version: 0,
    busy: false,
    page: empty(),
    snapshot: null as LogSnapshot | null,
    job: "",
    context: null as number | null,
    returnPage: null as PageState | null,
    mode,
    filters,
    info,
    follow,
  });
  Object.assign(s.current, { mode, filters, info, follow });
  const publish = useCallback((next: PageState) => {
    s.current.page = next;
    setPage(next);
  }, []);
  const load = useCallback(
    async (
      direction: "latest" | "older" | "newer" | "context",
      anchor?: number,
    ) => {
      const state = s.current;
      if (!state.alive || state.busy || !state.info) return;
      state.busy = true;
      setBusy(true);
      setError("");
      const version = state.version;
      try {
        const query: LogQuery =
          direction === "context" || state.context !== null
            ? { limit: 100 }
            : { ...state.filters, limit: PAGE_SIZE };
        const high = state.snapshot?.lastSeq ?? state.info.stats.lastSeq;
        query.snapshot = high;
        if (direction === "older") query.before = state.page.before;
        else if (direction === "newer") query.after = state.page.scannedThrough;
        else if (direction === "context") query.before = anchor! + 1;
        let rows: LogRow[] = [],
          last: LogPage | undefined,
          total = 0;
        do {
          last = await logApi.page(query, state.snapshot?.id);
          if (version !== state.version || !state.alive) return;
          total += last.scanned;
          setScanned(total);
          rows =
            query.after !== undefined
              ? mergeRows(rows, last.rows)
              : mergeRows(last.rows, rows);
          if (rows.length >= query.limit || !last.hasMore) break;
          if (direction === "newer" && total >= 80000) break;
          if (query.after !== undefined) query.after = last.nextCursor;
          else query.before = last.nextCursor;
        } while (true);
        if (direction === "context") {
          const after = await logApi.page(
            { limit: 50, after: anchor, snapshot: high },
            state.snapshot?.id,
          );
          if (version !== state.version || !state.alive) return;
          rows = mergeRows(rows, after.rows);
          publish({
            rows,
            older: last!.hasMore,
            newer: after.hasMore,
            before: last!.nextCursor,
            scannedThrough: after.nextCursor,
          });
        } else if (direction === "latest") {
          setAppliedKey(JSON.stringify([state.filters, state.mode]));
          const next = bounded(rows);
          publish({
            rows: next,
            older: last!.hasMore,
            newer: false,
            before: last!.nextCursor,
            scannedThrough: high,
          });
        } else if (direction === "older") {
          const merged = mergeRows(rows, state.page.rows),
            next = bounded(merged, false);
          publish({
            ...state.page,
            rows: next,
            before: last!.nextCursor,
            older: last!.hasMore,
            newer: next.length < merged.length || state.page.newer,
            scannedThrough:
              next.length < merged.length
                ? (next.at(-1)?.seq ?? 0)
                : state.page.scannedThrough,
          });
        } else {
          const merged = mergeRows(state.page.rows, rows),
            next = bounded(merged);
          publish({
            ...state.page,
            rows: next,
            before:
              next.length < merged.length ? next[0].seq : state.page.before,
            older: state.page.older || next.length < merged.length,
            newer: last!.hasMore,
            scannedThrough: last!.nextCursor,
          });
        }
      } catch (e) {
        if (state.alive && version === state.version)
          setError(errorOf(e).message);
      } finally {
        if (state.alive && version === state.version) {
          state.busy = false;
          setBusy(false);
        }
      }
    },
    [publish],
  );
  const key = JSON.stringify([filters, mode]);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    const state = s.current;
    state.alive = true;
    state.version++;
    state.busy = false;
    state.context = null;
    state.returnPage = null;
    setAppliedKey(null);
    setContext(null);
    setBusy(false);
    setError("");
    const version = state.version;
    const job = crypto.randomUUID();
    state.job = job;
    state.snapshot = null;
    setSnapshot(null);
    if (info?.sourceId) {
      void (async () => {
        try {
          if (mode === "snapshot") {
            state.busy = true;
            setBusy(true);
            const value = await logApi.snapshot(job);
            if (!state.alive || version !== state.version) {
              void logApi.release(value.id).catch(() => {});
              return;
            }
            state.snapshot = value;
            setSnapshot(value);
            state.busy = false;
          }
          if (state.alive && version === state.version) await load("latest");
        } catch (e) {
          if (state.alive && version === state.version) {
            state.busy = false;
            setBusy(false);
            setError(errorOf(e).message);
          }
        }
      })();
    }
    return () => {
      state.alive = false;
      state.version++;
      void logApi.cancel(job).catch(() => {});
      if (state.snapshot) {
        void logApi.release(state.snapshot.id).catch(() => {});
        state.snapshot = null;
      }
    };
  }, [info?.sourceId, key, reload, load]);
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let change = changedFrom;
    const update = () => {
      const state = s.current;
      if (
        cancelled ||
        !state.alive ||
        state.mode !== "live" ||
        !state.follow ||
        state.context !== null
      )
        return;
      if (state.busy) {
        timer = setTimeout(update, 100);
        return;
      }
      const previousCursor = state.page.scannedThrough;
      const direction =
        change !== null && change <= (state.page.rows.at(-1)?.seq ?? 0)
          ? "latest"
          : "newer";
      change = null;
      void load(direction).then(() => {
        if (
          !cancelled &&
          state.page.newer &&
          state.page.scannedThrough > previousCursor
        )
          timer = setTimeout(update, 0);
      });
    };
    if (revision) update();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [revision, follow, mode, changedFrom, load]);
  const showContext = useCallback(
    (seq: number) => {
      const state = s.current;
      if (state.busy) return;
      state.returnPage = state.page;
      state.context = seq;
      setContext(seq);
      void load("context", seq);
    },
    [load],
  );
  const returnContext = useCallback(() => {
    const state = s.current;
    state.version++;
    state.busy = false;
    setBusy(false);
    if (state.returnPage) publish(state.returnPage);
    state.returnPage = null;
    state.context = null;
    setContext(null);
  }, [publish]);
  const cancel = useCallback(() => {
    const state = s.current;
    state.version++;
    state.busy = false;
    setBusy(false);
    void logApi.cancel(state.job).catch(() => {});
  }, []);
  return {
    ...page,
    ready: appliedKey === key,
    busy,
    error,
    setError,
    scanned,
    snapshot,
    context,
    load,
    refresh: () => setReload((v) => v + 1),
    showContext,
    returnContext,
    cancel,
  };
}
