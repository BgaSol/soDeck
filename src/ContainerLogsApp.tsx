import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import {
  ScrollText,
  RefreshCw,
  Search,
  ArrowDownToLine,
  Download,
  Copy,
  X,
} from "lucide-react";
import { errorOf } from "./types";
import {
  logApi,
  type LogInfo,
  type LogPage,
  type LogQuery,
  type LogRow,
} from "./logApi";
import { LogText } from "./LogText";
import type { HighlightOptions } from "./logHighlight";
import "./files.css";
import "./logs.css";

type LogEvent =
  | { type: "ready"; following: boolean; tty: boolean; history: boolean }
  | { type: "status"; status: string; error: string | null }
  | { type: "saved"; lastSeq: number }
  | { type: "end"; error: string | null };
const PAGE_SIZE = 500,
  MAX_ROWS = 5000,
  MAX_BYTES = 16 * 1024 * 1024,
  INITIAL_INDEX = 1_000_000_000;
function bounded(rows: LogRow[], keepEnd: boolean) {
  let bytes = 0,
    result: LogRow[] = [];
  const ordered = keepEnd ? [...rows].reverse() : rows;
  for (const row of ordered) {
    const size =
      (row.raw.length + row.text.length + (row.prefix?.length ?? 0)) * 2;
    if (
      result.length >= MAX_ROWS ||
      (result.length > 0 && bytes + size > MAX_BYTES)
    )
      break;
    bytes += size;
    result.push(row);
  }
  return keepEnd ? result.reverse() : result;
}
function iso(value: string) {
  return value ? new Date(value).toISOString() : undefined;
}
export function ContainerLogsApp() {
  const [info, setInfo] = useState<LogInfo | null>(null),
    [error, setError] = useState(""),
    [status, setStatus] = useState("正在连接…");
  const [timestamps, setTimestamps] = useState(true),
    [wrap, setWrap] = useState(false),
    [highlight, setHighlight] = useState(true);
  const [follow, setFollow] = useState(true),
    [tty, setTty] = useState(false),
    [collectionFailed, setCollectionFailed] = useState(false),
    [running, setRunning] = useState(false);
  const [rows, setRows] = useState<LogRow[]>([]),
    [firstIndex, setFirstIndex] = useState(INITIAL_INDEX),
    [hasOlder, setHasOlder] = useState(false),
    [hasNewer, setHasNewer] = useState(false);
  const [busy, setBusy] = useState(false),
    [scanned, setScanned] = useState(0),
    [searchOpen, setSearchOpen] = useState(false),
    [draft, setDraft] = useState(""),
    [keyword, setKeyword] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false),
    [stream, setStream] = useState(""),
    [from, setFrom] = useState(""),
    [to, setTo] = useState("");
  const [contextQuery, setContextQuery] = useState<number | null>(null),
    [rangeFrom, setRangeFrom] = useState(""),
    [rangeTo, setRangeTo] = useState(""),
    [notice, setNotice] = useState("");
  const [exporting, setExporting] = useState(false),
    [reload, setReload] = useState(0);
  const resetIntent = useRef<"latest" | null>(null);
  const virtuoso = useRef<VirtuosoHandle>(null),
    searchInput = useRef<HTMLInputElement>(null);
  const current = useRef({
    rows: [] as LogRow[],
    firstIndex: INITIAL_INDEX,
    before: 0,
    snapshot: 0,
    version: 0,
    busy: false,
    follow: true,
    filtered: false,
    dirty: false,
    mounted: true,
  });
  const runEpoch = useRef(0),
    filtersRef = useRef<LogQuery>({ limit: PAGE_SIZE }),
    infoRef = useRef<LogInfo | null>(null);
  const filters = useMemo<LogQuery>(
    () => ({
      limit: PAGE_SIZE,
      keyword: contextQuery === null ? keyword : "",
      caseSensitive,
      stream: contextQuery === null ? stream : "",
      from: contextQuery === null ? iso(from) : undefined,
      to: contextQuery === null ? iso(to) : undefined,
    }),
    [keyword, caseSensitive, stream, from, to, contextQuery],
  );
  filtersRef.current = filters;
  current.current.filtered = !!(
    filters.keyword ||
    filters.stream ||
    filters.from ||
    filters.to ||
    contextQuery !== null
  );
  const options = useMemo<HighlightOptions>(
    () => ({ timestamps, highlight, keyword, caseSensitive }),
    [timestamps, highlight, keyword, caseSensitive],
  );
  const pause = useCallback(() => {
    current.current.follow = false;
    setFollow(false);
  }, []);
  const publish = useCallback((next: LogRow[], index: number) => {
    current.current.rows = next;
    current.current.firstIndex = index;
    setRows(next);
    setFirstIndex(index);
  }, []);
  const refreshInfo = useCallback(async () => {
    const value = await logApi.info();
    if (!current.current.mounted) return null;
    infoRef.current = value;
    setInfo(value);
    setTty(value.tty);
    return value;
  }, []);
  const load = useCallback(
    async (mode: "latest" | "older" | "newer" | "reset", anchor?: number) => {
      const state = current.current;
      if (state.busy || !state.mounted) return;
      state.busy = true;
      setBusy(true);
      const version = state.version;
      try {
        let query = { ...filtersRef.current };
        const filtered = state.filtered;
        if (mode === "latest" || mode === "reset") {
          const value = await refreshInfo();
          if (!value || version !== state.version) return;
          state.snapshot = value.stats.lastSeq;
          query.snapshot = state.snapshot;
          if (anchor !== undefined) {
            query.before = anchor + 1;
            query.limit = 100;
          } else query.limit = PAGE_SIZE;
        } else if (mode === "older") {
          query.before = state.before;
          query.snapshot = filtered ? state.snapshot : undefined;
        } else {
          query.after = Math.max(0, (state.rows.at(-1)?.seq ?? 1) - 1);
          query.snapshot = filtered ? state.snapshot : undefined;
        }
        const wanted = anchor !== undefined ? 100 : PAGE_SIZE;
        query.limit = Math.min(1000, wanted);
        let found: LogRow[] = [],
          page: LogPage,
          total = 0;
        do {
          page = await logApi.page(query);
          if (version !== state.version || !state.mounted) return;
          total += page.scanned;
          setScanned(total);
          found =
            query.after !== undefined
              ? [...found, ...page.rows]
              : [...page.rows, ...found];
          if (found.length >= wanted || !page.hasMore) break;
          if (query.after !== undefined) query.after = page.nextCursor;
          else query.before = page.nextCursor;
          query.limit = Math.min(1000, wanted - found.length);
        } while (true);
        if (anchor !== undefined) {
          const after = await logApi.page({
            limit: 50,
            after: anchor,
            snapshot: state.snapshot,
          });
          if (version !== state.version || !state.mounted) return;
          found = [...found, ...after.rows];
          setHasNewer(after.hasMore);
        }
        if (mode === "latest" || mode === "reset") {
          const next = bounded(found, true);
          publish(next, INITIAL_INDEX);
          state.before =
            found.length > next.length ? next[0].seq : page!.nextCursor;
          setHasOlder(page!.hasMore || found.length > next.length);
          if (anchor === undefined) setHasNewer(false);
        } else if (mode === "older") {
          const next = bounded([...found, ...state.rows], false);
          publish(next, state.firstIndex - found.length);
          state.before = page!.nextCursor;
          setHasOlder(page!.hasMore);
          setHasNewer(
            (next.at(-1)?.seq ?? 0) < (infoRef.current?.stats.lastSeq ?? 0),
          );
        } else {
          const merged = new Map(state.rows.map((row) => [row.seq, row]));
          for (const row of found) merged.set(row.seq, row);
          const combined = [...merged.values()].sort((a, b) => a.seq - b.seq),
            next = bounded(combined, true);
          const removed = combined.length - next.length;
          publish(next, state.firstIndex + removed);
          if (removed) {
            state.before = next[0].seq;
            setHasOlder(true);
          }
          setHasNewer(page!.hasMore);
          if (page!.hasMore && state.follow) state.dirty = true;
        }
        if (current.current.follow && !filtered)
          requestAnimationFrame(() =>
            virtuoso.current?.scrollToIndex({
              index: "LAST",
              align: "end",
              behavior: "auto",
            }),
          );
      } catch (e) {
        if (version === state.version && state.mounted)
          setError(errorOf(e).message);
      } finally {
        if (version === state.version && state.mounted) {
          state.busy = false;
          setBusy(false);
        }
      }
    },
    [publish, refreshInfo],
  );
  const loadRef = useRef(load);
  loadRef.current = load;
  const start = useCallback(async () => {
    const token = ++runEpoch.current;
    setError("");
    setStatus("正在连接…");
    setRunning(true);
    setCollectionFailed(false);
    const output = new Channel<LogEvent>();
    output.onmessage = (event) => {
      if (runEpoch.current !== token || !current.current.mounted) return;
      if (event.type === "ready") {
        setTty(event.tty);
        setError("");
        setStatus(
          event.history
            ? "正在读取历史日志…"
            : event.following
              ? "实时采集中"
              : "正在接续日志…",
        );
      } else if (event.type === "status") {
        setStatus(event.status);
        setError(event.error || "");
      } else if (event.type === "saved") current.current.dirty = true;
      else {
        setRunning(false);
        setStatus(event.error ? "读取已中断" : "日志读取已结束");
        setError(event.error || "");
        setCollectionFailed(!!event.error);
        current.current.dirty = true;
      }
    };
    try {
      await invoke("logs_start", { output });
    } catch (e) {
      if (runEpoch.current === token) {
        setError(errorOf(e).message);
        setStatus("读取失败");
        setRunning(false);
        setCollectionFailed(true);
      }
    }
  }, []);
  const startRef = useRef(start);
  startRef.current = start;
  useEffect(() => {
    const state = current.current;
    state.mounted = true;
    let disposed = false;
    void refreshInfo()
      .then(async (value) => {
        if (!value || disposed) return;
        await loadRef.current("latest");
        if (disposed) return;
        if (value.canCollect) void startRef.current();
        else {
          pause();
          setStatus("本地存档");
        }
      })
      .catch((e) => {
        if (!disposed) {
          setError(errorOf(e).message);
          setStatus("读取失败");
        }
      });
    const timer = window.setInterval(() => {
      if (!state.dirty || state.busy) return;
      state.dirty = false;
      void refreshInfo()
        .then(() => {
          if (state.follow && !state.filtered)
            return loadRef.current(
              state.rows.length &&
                (infoRef.current?.stats.lastSeq ?? 0) -
                  (state.rows.at(-1)?.seq ?? 0) <
                  MAX_ROWS
                ? "newer"
                : "latest",
            );
          setHasNewer(
            (infoRef.current?.stats.lastSeq ?? 0) >
              (state.rows.at(-1)?.seq ?? 0),
          );
        })
        .catch((e) => {
          if (!disposed) setError(errorOf(e).message);
        });
    }, 300);
    const key = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === "f") {
        event.preventDefault();
        pause();
        setSearchOpen(true);
      }
    };
    window.addEventListener("keydown", key);
    const focus = () => {
      const couldCollect = infoRef.current?.canCollect;
      void refreshInfo()
        .then((value) => {
          if (value?.canCollect && couldCollect === false)
            void startRef.current();
        })
        .catch((e) => {
          if (!disposed) setError(errorOf(e).message);
        });
    };
    window.addEventListener("focus", focus);
    return () => {
      disposed = true;
      state.mounted = false;
      state.version++;
      runEpoch.current++;
      clearInterval(timer);
      window.removeEventListener("keydown", key);
      window.removeEventListener("focus", focus);
      void invoke("logs_stop").catch(() => {});
    };
  }, [pause, refreshInfo]);
  useEffect(() => {
    const timer = setTimeout(() => setKeyword(draft), 300);
    return () => clearTimeout(timer);
  }, [draft]);
  const filterInit = useRef(true);
  useEffect(() => {
    if (filterInit.current) {
      filterInit.current = false;
      return;
    }
    current.current.version++;
    current.current.busy = false;
    setBusy(false);
    const latestRequested = resetIntent.current === "latest";
    resetIntent.current = null;
    if (latestRequested) {
      current.current.follow = true;
      setFollow(true);
    } else pause();
    void loadRef.current(
      latestRequested ? "latest" : "reset",
      contextQuery ?? undefined,
    );
  }, [filters, reload, pause]);
  useEffect(() => {
    if (searchOpen) searchInput.current?.focus();
  }, [searchOpen]);
  function latest() {
    setContextQuery(null);
    setDraft("");
    setKeyword("");
    setStream("");
    setFrom("");
    setTo("");
    resetIntent.current = "latest";
    setReload((value) => value + 1);
    setSearchOpen(false);
  }
  function cancel() {
    current.current.version++;
    current.current.busy = false;
    setBusy(false);
  }
  async function exportLogs(selected: boolean) {
    setExporting(true);
    setError("");
    try {
      const path = await logApi.export(
        selected ? Number(rangeFrom) : undefined,
        selected ? Number(rangeTo) : undefined,
      );
      if (path) setNotice(`已导出：${path}`);
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setExporting(false);
    }
  }
  const lastSeq = rows.at(-1)?.seq ?? 0,
    newCount = Math.max(0, (info?.stats.lastSeq ?? 0) - lastSeq);
  const rangeValid =
    Number(rangeFrom) >= 1 && Number(rangeTo) >= Number(rangeFrom);
  return (
    <div className="container-logs-app remote-editor-app">
      <header className="remote-editor-header">
        <ScrollText size={20} />
        <div className="log-title">
          <h1>
            {info
              ? `${info.serverName} / ${info.containerName} / 日志`
              : "容器日志"}
          </h1>
          <p title={info?.directory}>
            {info
              ? `${info.host}:${info.port} · ${info.containerId.slice(0, 12)} · 本地永久保存`
              : "正在读取本地存档…"}
          </p>
        </div>
        <span className="log-status" role="status">
          {status}
        </span>
      </header>
      <div className="log-tools">
        <button
          onClick={() => {
            pause();
            setSearchOpen(true);
          }}
        >
          <Search size={14} />
          查找
        </button>
        <label>
          <input
            type="checkbox"
            checked={timestamps}
            onChange={(e) => setTimestamps(e.target.checked)}
          />
          时间戳
        </label>
        <label>
          <input
            type="checkbox"
            checked={wrap}
            onChange={(e) => setWrap(e.target.checked)}
          />
          自动换行
        </label>
        <label>
          <input
            type="checkbox"
            checked={highlight}
            onChange={(e) => setHighlight(e.target.checked)}
          />
          高亮
        </label>
        <button
          disabled={exporting || !info?.stats.records}
          onClick={() => void exportLogs(false)}
        >
          <Download size={14} />
          导出全部
        </button>
      </div>
      {searchOpen && (
        <div className="log-search" role="search">
          <div className="log-search-main">
            <Search size={15} />
            <input
              ref={searchInput}
              aria-label="搜索全部本地日志"
              placeholder="搜索全部本地历史，支持中文子串"
              value={draft}
              onChange={(e) => {
                setContextQuery(null);
                setDraft(e.target.value);
              }}
            />
            <label>
              <input
                type="checkbox"
                checked={caseSensitive}
                onChange={(e) => setCaseSensitive(e.target.checked)}
              />
              区分大小写
            </label>
            <select
              aria-label="日志输出流"
              value={stream}
              onChange={(e) => setStream(e.target.value)}
            >
              <option value="">全部输出流</option>
              <option value="stdout">stdout</option>
              <option value="stderr">stderr</option>
            </select>
            <button aria-label="关闭查找" onClick={() => setSearchOpen(false)}>
              <X size={14} />
            </button>
          </div>
          <div className="log-search-filters">
            <label>
              从
              <input
                aria-label="日志开始时间"
                type="datetime-local"
                value={from}
                onChange={(e) => setFrom(e.target.value)}
              />
            </label>
            <label>
              至
              <input
                aria-label="日志结束时间"
                type="datetime-local"
                value={to}
                onChange={(e) => setTo(e.target.value)}
              />
            </label>
            <span>
              {contextQuery !== null
                ? `日志 #${contextQuery} 的上下文`
                : `已加载 ${rows.length} 条匹配 · 搜索范围截至 #${current.current.snapshot}`}
            </span>
            {contextQuery === null &&
              (info?.stats.lastSeq ?? 0) > current.current.snapshot && (
                <button
                  onClick={() => {
                    setContextQuery(null);
                    cancel();
                    void loadRef.current("reset");
                  }}
                >
                  更新搜索结果
                </button>
              )}
            {busy && <button onClick={cancel}>取消搜索</button>}
          </div>
        </div>
      )}
      {error && (
        <div className="editor-error" role="alert">
          {error}
          {collectionFailed && info?.canCollect && (
            <button onClick={() => void start()}>
              <RefreshCw size={14} />
              重试
            </button>
          )}
        </div>
      )}
      <div
        className={`log-viewer${wrap ? " log-wrap" : ""}`}
        onWheel={(e) => {
          if (e.deltaY < 0) pause();
        }}
        onKeyDown={(e) => {
          if (["ArrowUp", "PageUp", "Home"].includes(e.key)) pause();
        }}
        onPointerDown={pause}
      >
        <Virtuoso
          ref={virtuoso}
          className="log-virtual-list"
          data={rows}
          firstItemIndex={firstIndex}
          computeItemKey={(_, row) => row.seq}
          initialTopMostItemIndex={{
            index: rows.length ? firstIndex + rows.length - 1 : firstIndex,
            align: "end",
          }}
          alignToBottom
          increaseViewportBy={{ top: 200, bottom: 200 }}
          defaultItemHeight={27}
          followOutput={follow && !current.current.filtered ? "auto" : false}
          startReached={() => {
            if (hasOlder && rows.length && !current.current.follow)
              void loadRef.current("older");
          }}
          endReached={() => {
            if (hasNewer && !current.current.follow)
              void loadRef.current("newer");
          }}
          components={{
            Header: () =>
              hasOlder ? (
                <div className="log-page-control">
                  <button
                    disabled={busy}
                    onClick={() => {
                      pause();
                      void loadRef.current("older");
                    }}
                  >
                    加载更早日志
                  </button>
                </div>
              ) : null,
            Footer: () =>
              hasNewer ? (
                <div className="log-page-control">
                  <button
                    disabled={busy}
                    onClick={() => {
                      pause();
                      void loadRef.current("newer");
                    }}
                  >
                    加载后续日志
                  </button>
                </div>
              ) : null,
          }}
          itemContent={(_, row) => (
            <div
              className={`log-row${highlight && row.level ? ` log-level-${row.level}` : ""}`}
              data-seq={row.seq}
            >
              <button
                className="log-seq"
                title="点击选择起始序号，Shift 点击选择结束序号"
                onClick={(e) => {
                  if (e.shiftKey && rangeFrom) setRangeTo(String(row.seq));
                  else {
                    setRangeFrom(String(row.seq));
                    setRangeTo(String(row.seq));
                  }
                }}
              >
                #{row.seq}
              </button>
              <div className="log-row-body">
                {row.stream === "stderr" && !tty && (
                  <span className="log-stream">[stderr] </span>
                )}
                {row.level && highlight && (
                  <span className={`log-level-label log-level-${row.level}`}>
                    {row.level.toUpperCase()}
                  </span>
                )}
                {row.continuation && (
                  <span className="log-fragment">续行 </span>
                )}
                {!!keyword && !!row.prefix && (
                  <span className="log-fragment">跨片段上下文 </span>
                )}
                <LogText
                  raw={(keyword && row.prefix
                    ? row.prefix + row.text
                    : row.raw
                  ).slice(0, 12000)}
                  options={options}
                />
                {row.raw.length > 12000 && (
                  <span className="log-fragment">
                    {" "}
                    …长行已折叠，选择此序号可复制或导出全文
                  </span>
                )}
                {!row.complete && !row.continuation && (
                  <span className="log-fragment"> 未换行</span>
                )}
              </div>
              {!!keyword && contextQuery === null && (
                <button
                  className="log-context"
                  onClick={() => {
                    pause();
                    setContextQuery(row.seq);
                  }}
                >
                  上下文
                </button>
              )}
            </div>
          )}
        />
        {!rows.length && !busy && (
          <p className="log-empty">
            {current.current.filtered
              ? "没有匹配的本地日志"
              : running && status !== "等待容器运行"
                ? "暂无日志，正在等待新的输出…"
                : "容器暂无日志"}
          </p>
        )}
      </div>
      <div className="log-range">
        <label>
          序号范围
          <input
            type="number"
            aria-label="复制起始序号"
            min={1}
            value={rangeFrom}
            onChange={(e) => setRangeFrom(e.target.value)}
          />
        </label>
        <span>—</span>
        <input
          type="number"
          aria-label="复制结束序号"
          min={1}
          value={rangeTo}
          onChange={(e) => setRangeTo(e.target.value)}
        />
        <button
          disabled={!rangeValid}
          onClick={() => {
            void logApi
              .copy(Number(rangeFrom), Number(rangeTo))
              .then((text) => navigator.clipboard.writeText(text))
              .then(() => setNotice("已复制完整原文"))
              .catch((e) => setError(errorOf(e).message));
          }}
        >
          <Copy size={13} />
          复制原文
        </button>
        <button
          disabled={!rangeValid || exporting}
          onClick={() => void exportLogs(true)}
        >
          导出范围
        </button>
        <span className="log-notice" role="status">
          {notice}
        </span>
      </div>
      <footer className="log-footer">
        <span title={info?.directory}>
          已保存 {info?.stats.records ?? 0} 条 ·{" "}
          {((info?.stats.bytes ?? 0) / 1024 / 1024).toFixed(1)} MiB
          {busy
            ? ` · 正在读取，已检索 ${scanned} 条`
            : ` · 当前加载 ${rows.length} 条`}
          {tty ? " · 容器 TTY 输出已合并" : " · [stderr] 标记错误输出流"}
        </span>
        <label>
          <input
            type="checkbox"
            checked={follow}
            onChange={(e) => (e.target.checked ? latest() : pause())}
          />
          自动滚动
        </label>
        <button onClick={latest}>
          <ArrowDownToLine size={14} />
          {newCount && !follow ? `新增 ${newCount} 条 · ` : ""}跳到最新
        </button>
      </footer>
    </div>
  );
}
