import { WindowTitlebar } from "./WindowTitlebar";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import {
  terminalHistoryApi as api,
  boundedHistory,
  historyTitle,
  type HistoryBinding,
  type HistoryPosition,
  type HistoryRow,
  type HistorySession,
  type HistoryQuery,
  type SearchPage,
} from "./terminalHistoryApi";
import { LogText } from "./LogText";
import { TerminalReplayView } from "./TerminalReplayView";
import { errorOf } from "./types";
import "./terminal-history.css";
import "./logs.css";

const START = 1_000_000_000;
export function TerminalHistoryView({
  binding,
  initial,
  onPosition,
  global = false,
}: {
  binding?: HistoryBinding;
  initial?: HistoryPosition;
  onPosition?: (position: HistoryPosition) => void;
  global?: boolean;
}) {
  const [sessions, setSessions] = useState<HistorySession[]>([]);
  const [sessionId, setSessionId] = useState(
    initial?.sessionId ?? binding?.sessionId ?? "",
  );
  const [rows, setRows] = useState<HistoryRow[]>([]);
  const [firstIndex, setFirstIndex] = useState(START);
  const [older, setOlder] = useState(false),
    [newer, setNewer] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState(""),
    [keyword, setKeyword] = useState("");
  const [scope, setScope] = useState(global ? "all" : "session");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [from, setFrom] = useState(""),
    [to, setTo] = useState("");
  const [search, setSearch] = useState<SearchPage | null>(null);
  const [searchCursors, setSearchCursors] = useState<(string | undefined)[]>([
    undefined,
  ]);
  const [searchPage, setSearchPage] = useState(0);
  const [selected, setSelected] = useState<{ from: string; to: string } | null>(
    null,
  );
  const [replay, setReplay] = useState<HistoryRow | null>(null);
  const [updates, setUpdates] = useState(false);
  const [reload, setReload] = useState(0);
  const [listVersion, setListVersion] = useState(0);
  const scrollIntent = useRef(false);
  const list = useRef<VirtuosoHandle>(null);
  const requestId = useRef(crypto.randomUUID());
  const current = useRef({
    epoch: 0,
    busy: false,
    rows: [] as HistoryRow[],
    snapshot: "0",
    first: START,
  });
  const anchor = useRef(initial?.rowId);
  const positionCallback = useRef(onPosition);
  positionCallback.current = onPosition;
  const session = sessions.find((s) => s.id === sessionId);
  const sourceScope =
    scope === "source" ? (session?.source.id ?? binding?.sourceId) : undefined;
  const options = useMemo(
    () => ({ timestamps: true, highlight: true, keyword, caseSensitive }),
    [keyword, caseSensitive],
  );
  const query = useMemo<HistoryQuery>(
    () => ({
      sessionId: scope === "session" ? sessionId || undefined : undefined,
      sourceId: sourceScope,
      keyword,
      caseSensitive,
      from: from ? new Date(from).getTime() : undefined,
      to: to ? new Date(to).getTime() : undefined,
      limit: 200,
    }),
    [scope, sessionId, sourceScope, keyword, caseSensitive, from, to],
  );

  useEffect(() => {
    let cancelled = false,
      pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const next = await api.sessions();
        if (cancelled) return;
        setSessions(next);
        setSessionId((id) => id || next[0]?.id || "");
      } catch (e) {
        if (!cancelled) setError(errorOf(e).message);
      } finally {
        pending = false;
      }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 3000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [binding?.sourceId, global]);

  const publish = useCallback((next: HistoryRow[], first: number) => {
    current.current.rows = next;
    current.current.first = first;
    setRows(next);
    setFirstIndex(first);
  }, []);
  useEffect(() => {
    const epoch = ++current.current.epoch;
    scrollIntent.current = false;
    current.current.busy = true;
    setBusy(true);
    setError("");
    setSelected(null);
    setUpdates(false);
    const oldRequest = requestId.current;
    const thisRequest = crypto.randomUUID();
    requestId.current = thisRequest;
    void api.cancel(oldRequest).catch(() => {});
    const load = async () => {
      try {
        if (keyword) {
          const page = await api.search({ ...query, requestId: thisRequest });
          if (epoch !== current.current.epoch) return;
          setSearch(page);
          setSearchCursors([undefined]);
          setSearchPage(0);
        } else if (sessionId) {
          setSearch(null);
          const at = anchor.current;
          const page = await api.page({
            sessionId,
            from: query.from,
            to: query.to,
            limit: 500,
            requestId: thisRequest,
            before: at ? (BigInt(at) + 1n).toString() : undefined,
          });
          if (epoch !== current.current.epoch) return;
          anchor.current = undefined;
          current.current.snapshot = page.snapshot;
          setUpdates(false);
          publish(page.rows, START);
          setOlder(page.hasMore);
          setNewer(!!at && at !== page.snapshot);
          setListVersion((v) => v + 1);
        } else {
          publish([], START);
          setSearch(null);
        }
      } catch (e) {
        if (epoch === current.current.epoch) setError(errorOf(e).message);
      } finally {
        if (epoch === current.current.epoch) {
          current.current.busy = false;
          setBusy(false);
        }
      }
    };
    void load();
    return () => {
      current.current.epoch++;
      void api.cancel(thisRequest).catch(() => {});
    };
  }, [sessionId, keyword, query, reload, publish]);

  useEffect(() => {
    if (
      session &&
      BigInt(session.lastRecord) > BigInt(current.current.snapshot)
    )
      setUpdates(true);
  }, [session?.lastRecord, session]);
  useEffect(() => {
    if (!search) return;
    try {
      const saved = new Map<string, string>(
        JSON.parse(search.snapshot).sessions,
      );
      if (
        sessions.some(
          (s) =>
            (scope === "all" ||
              (scope === "session"
                ? s.id === sessionId
                : s.source.id === session?.source.id)) &&
            saved.get(s.id) !== s.lastRecord,
        )
      )
        setUpdates(true);
    } catch {
      /* Snapshot is opaque to older backends. */
    }
  }, [sessions, search, scope, sessionId, session?.source.id]);
  async function more(direction: "older" | "newer", navigate = false) {
    if (
      current.current.busy ||
      !sessionId ||
      keyword ||
      !current.current.rows.length
    )
      return;
    if ((direction === "older" && !older) || (direction === "newer" && !newer))
      return;
    current.current.busy = true;
    setBusy(true);
    if (navigate) scrollIntent.current = false;
    const epoch = current.current.epoch;
    try {
      const old = current.current.rows;
      const page = await api.page({
        sessionId,
        from: query.from,
        to: query.to,
        limit: 500,
        snapshot: current.current.snapshot,
        requestId: requestId.current,
        ...(direction === "older"
          ? { before: old[0].id }
          : { after: old.at(-1)!.id }),
      });
      if (epoch !== current.current.epoch) return;
      const merged =
        direction === "older" ? [...page.rows, ...old] : [...old, ...page.rows];
      const next = boundedHistory(merged, direction === "newer");
      const trimmed = merged.length - next.length;
      publish(
        next,
        current.current.first +
          (direction === "older" ? -page.rows.length : trimmed),
      );
      if (navigate)
        requestAnimationFrame(() =>
          list.current?.scrollToIndex({
            index: direction === "older" ? current.current.first : "LAST",
            align: direction === "older" ? "start" : "end",
          }),
        );
      if (direction === "older") {
        setOlder(page.hasMore);
        if (trimmed) setNewer(true);
      } else {
        setNewer(page.hasMore);
        if (trimmed) setOlder(true);
      }
    } catch (e) {
      if (epoch === current.current.epoch) setError(errorOf(e).message);
    } finally {
      if (epoch === current.current.epoch) {
        current.current.busy = false;
        setBusy(false);
      }
    }
  }
  async function searchNext(index: number) {
    if (!search || busy) return;
    const cursor =
      index > searchPage ? (search.cursor ?? undefined) : searchCursors[index];
    setBusy(true);
    const epoch = current.current.epoch;
    try {
      const page = await api.search(
        { ...query, snapshot: search.snapshot, requestId: requestId.current },
        cursor,
      );
      if (epoch !== current.current.epoch) return;
      setSearch(page);
      setSearchPage(index);
      setSearchCursors((old) => {
        const next = [...old];
        next[index] = cursor;
        return next;
      });
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      if (epoch === current.current.epoch) setBusy(false);
    }
  }
  function context(row: HistoryRow) {
    anchor.current = row.id;
    setSessionId(row.sessionId);
    setKeyword("");
    setDraft("");
    setReload((v) => v + 1);
  }
  async function copy(fromId: string, toId: string) {
    try {
      await navigator.clipboard.writeText(
        await api.copy(sessionId, fromId, toId),
      );
      setNotice("已复制历史记录");
    } catch (e) {
      setError(errorOf(e).message);
    }
  }
  async function exportSession(raw: boolean, range = false) {
    if (!sessionId) return;
    try {
      const path = await api.export(
        sessionId,
        raw,
        range ? selected?.from : undefined,
        range ? selected?.to : undefined,
      );
      if (path) setNotice(`已导出到 ${path}`);
    } catch (e) {
      setError(errorOf(e).message);
    }
  }
  function selectRow(row: HistoryRow, extend: boolean) {
    const start = extend && selected ? selected.from : row.id;
    setSelected(
      BigInt(start) <= BigInt(row.id)
        ? { from: start, to: row.id }
        : { from: row.id, to: start },
    );
  }
  const renderRow = (row: HistoryRow, searching = false) => {
    const text = <LogText raw={row.ansi || row.text} options={options} />;
    const inRange =
      !searching &&
      selected &&
      BigInt(row.id) >= BigInt(selected.from) &&
      BigInt(row.id) <= BigInt(selected.to);
    return (
      <div
        className={`terminal-history-row ${row.kind === "marker" ? "history-marker" : ""}${inRange ? " history-selected" : ""}`}
        data-history-id={row.id}
      >
        <button
          className="history-row-number"
          title="点击选择记录，Shift 点击选择范围"
          onClick={(e) => selectRow(row, e.shiftKey)}
          disabled={searching}
        >
          {row.id}
        </button>
        <time title={new Date(row.time).toLocaleString()}>
          {new Date(row.time).toLocaleTimeString()}
        </time>
        <div className="history-row-content">
          {row.kind === "screen" || row.kind === "revision" ? (
            <details open={searching}>
              <summary>
                {row.kind === "screen" ? "屏幕交互" : "行内更新"} ·{" "}
                {row.text.slice(0, 72)}
              </summary>
              <pre>{text}</pre>
            </details>
          ) : (
            <pre>{text}</pre>
          )}
        </div>
        <div className="history-row-actions">
          {searching ? (
            <button onClick={() => context(row)}>上下文</button>
          ) : (
            <button
              title="复制此记录"
              onClick={() => void copy(row.id, row.id)}
            >
              复制
            </button>
          )}
          <button onClick={() => setReplay(row)}>回放</button>
        </div>
      </div>
    );
  };
  return (
    <section className="terminal-history-view" aria-label="终端历史">
      <div className="history-controls">
        <label>
          会话{" "}
          <select
            aria-label="历史会话"
            value={sessionId}
            onChange={(e) => {
              anchor.current = undefined;
              setSessionId(e.target.value);
            }}
          >
            {!sessions.length && <option value="">暂无历史</option>}
            {sessions.map((s) => (
              <option key={s.id} value={s.id}>
                {historyTitle(s)} · {s.id.slice(0, 8)}
              </option>
            ))}
          </select>
        </label>
        <button disabled={!sessionId} onClick={() => void exportSession(false)}>
          导出文本
        </button>
        <button disabled={!sessionId} onClick={() => void exportSession(true)}>
          导出原始记录
        </button>
      </div>
      <form
        className="history-search"
        onSubmit={(e) => {
          e.preventDefault();
          setKeyword(draft.trim());
          setReload((v) => v + 1);
        }}
      >
        <input
          aria-label="搜索终端历史"
          placeholder="搜索已保存的终端内容…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          maxLength={4096}
        />
        <select
          aria-label="搜索范围"
          value={scope}
          onChange={(e) => setScope(e.target.value)}
        >
          <option value="session">当前会话</option>
          <option value="source">当前服务器 / 容器</option>
          <option value="all">全部终端</option>
        </select>
        <button type="submit" disabled={busy}>
          搜索
        </button>
        {keyword && (
          <button
            type="button"
            onClick={() => {
              setKeyword("");
              setDraft("");
            }}
          >
            退出搜索
          </button>
        )}
        {busy && (
          <button
            type="button"
            onClick={() => {
              void api.cancel(requestId.current);
              current.current.epoch++;
              current.current.busy = false;
              setBusy(false);
              setNotice("查询已取消");
            }}
          >
            取消查询
          </button>
        )}
        <label>
          <input
            type="checkbox"
            checked={caseSensitive}
            onChange={(e) => setCaseSensitive(e.target.checked)}
          />
          区分大小写
        </label>
        <details className="history-time-filter">
          <summary>时间范围</summary>
          <label>
            开始{" "}
            <input
              type="datetime-local"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
            />
          </label>
          <label>
            结束{" "}
            <input
              type="datetime-local"
              value={to}
              onChange={(e) => setTo(e.target.value)}
            />
          </label>
        </details>
      </form>
      {session?.error && (
        <div className="history-error" role="alert">
          {session.error}
        </div>
      )}
      {error && (
        <div className="history-error" role="alert">
          {error}
          <button onClick={() => setReload((v) => v + 1)}>重试读取</button>
        </div>
      )}
      <div className="history-status">
        <span role="status">
          {busy
            ? "正在读取…"
            : session
              ? `${session.state === "recording" ? "持续记录" : session.state === "interrupted" ? "上次异常中断" : "已结束"} · 已保存事件 ${session.savedSeq}${session.savedSeq !== session.indexedSeq ? " · 正在建立历史索引" : ""}`
              : "暂无终端历史"}
        </span>
        {updates && (
          <button
            onClick={() => {
              anchor.current = undefined;
              setReload((v) => v + 1);
            }}
          >
            {keyword ? "更新搜索结果" : "跳到最新记录"}
          </button>
        )}
        {selected && !keyword && (
          <>
            <span>
              已选 {selected.from}–{selected.to}
            </span>
            <button onClick={() => void copy(selected.from, selected.to)}>
              复制范围
            </button>
            <button onClick={() => void exportSession(false, true)}>
              导出范围
            </button>
          </>
        )}
      </div>
      <div
        className="history-list"
        onWheel={() => {
          scrollIntent.current = true;
        }}
        onTouchStart={() => {
          scrollIntent.current = true;
        }}
        onPointerDown={() => {
          scrollIntent.current = true;
        }}
        onKeyDown={() => {
          scrollIntent.current = true;
        }}
      >
        {search ? (
          <Virtuoso
            key="search"
            data={search.hits}
            computeItemKey={(_, hit) => `${hit.session.id}:${hit.row.id}`}
            itemContent={(_, hit) => (
              <div className="history-search-hit">
                <div className="history-hit-source">
                  {historyTitle(hit.session)} · {hit.session.source.address} ·{" "}
                  {hit.session.id.slice(0, 8)}
                </div>
                {renderRow(hit.row, true)}
              </div>
            )}
            components={{
              EmptyPlaceholder: () => (
                <div className="history-empty">没有匹配的历史记录</div>
              ),
            }}
          />
        ) : (
          <Virtuoso
            key={`history-${listVersion}`}
            ref={list}
            data={rows}
            firstItemIndex={firstIndex}
            computeItemKey={(_, row) => `${row.sessionId}:${row.id}`}
            initialTopMostItemIndex={{
              index: firstIndex + Math.max(0, rows.length - 1),
              align: "end",
            }}
            startReached={() => {
              if (scrollIntent.current) void more("older");
            }}
            endReached={() => {
              if (scrollIntent.current) void more("newer");
            }}
            rangeChanged={({ startIndex }) => {
              const row =
                current.current.rows[startIndex - current.current.first];
              if (row) positionCallback.current?.({ sessionId, rowId: row.id });
            }}
            itemContent={(_, row) => renderRow(row)}
            components={{
              EmptyPlaceholder: () => (
                <div className="history-empty">
                  {busy
                    ? "正在读取历史…"
                    : "暂无可显示记录。已保存的输出会在索引完成后出现。"}
                </div>
              ),
            }}
          />
        )}
      </div>
      <footer className="history-pagination">
        {search ? (
          <>
            <button
              disabled={busy || !searchPage}
              onClick={() => void searchNext(searchPage - 1)}
            >
              上一页
            </button>
            <span>搜索结果 · 第 {searchPage + 1} 页</span>
            <button
              disabled={busy || !search.cursor}
              onClick={() => void searchNext(searchPage + 1)}
            >
              下一页
            </button>
          </>
        ) : (
          <>
            <button
              disabled={busy || !older}
              onClick={() => void more("older", true)}
            >
              更早记录
            </button>
            <span>当前已加载 {rows.length} 条 · 历史保存在本机</span>
            <button
              disabled={busy || !newer}
              onClick={() => void more("newer", true)}
            >
              更晚记录
            </button>
          </>
        )}
        {notice && <span role="status">{notice}</span>}
      </footer>
      {replay && (
        <TerminalReplayView row={replay} onClose={() => setReplay(null)} />
      )}
    </section>
  );
}

export function TerminalHistoryApp() {
  return (
    <main className="terminal-history-app">
      <WindowTitlebar
        title="终端历史"
        subtitle="查找服务器和 Docker 容器终端中已保存的内容"
      />
      <TerminalHistoryView global />
    </main>
  );
}
