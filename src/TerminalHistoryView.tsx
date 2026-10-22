import { copyText } from "./terminalClipboard/clipboardService";
import { historySelection } from "./terminalClipboard/historySelection";
import { WindowTitlebar } from "./WindowTitlebar";
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ArrowDown,
  Box,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Copy,
  Download,
  HardDrive,
  History,
  LockKeyhole,
  Monitor,
  Search,
  Server,
  SlidersHorizontal,
  X,
} from "lucide-react";
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
const timeLabel = (time: number) =>
  new Date(time).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
const recordTime = (time: number) =>
  new Date(time).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
function dayLabel(time: number) {
  const date = new Date(time);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (date.toDateString() === today.toDateString()) return "今天";
  if (date.toDateString() === yesterday.toDateString()) return "昨天";
  return date.toLocaleDateString("zh-CN", {
    year: date.getFullYear() === today.getFullYear() ? undefined : "numeric",
    month: "long",
    day: "numeric",
  });
}
function sessionState(session: HistorySession) {
  return session.error
    ? "记录异常"
    : session.state === "recording"
      ? "记录中"
      : session.state === "interrupted"
        ? "异常中断"
        : "已结束";
}
function sessionTime(session: HistorySession) {
  return `${timeLabel(session.startedAt)}${session.endedAt ? `–${new Date(session.endedAt).toDateString() !== new Date(session.startedAt).toDateString() ? `${dayLabel(session.endedAt)} ` : ""}${timeLabel(session.endedAt)}` : ""}`;
}
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
  const [selectedRecord, setSelectedRecord] = useState<HistoryRow | null>(null);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const filterId = useId();
  const exportId = useId();
  const filterHost = useRef<HTMLDivElement>(null);
  const exportHost = useRef<HTMLDivElement>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const replayTrigger = useRef<HTMLElement | null>(null);
  const selectionAnchor = useRef<string | null>(null);
  const pendingSelection = useRef<HistoryRow | null>(null);
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
  const sessionGroups = useMemo(() => {
    const groups = new Map<string, HistorySession[]>();
    for (const item of [...sessions].sort(
      (a, b) => b.startedAt - a.startedAt,
    )) {
      const day = dayLabel(item.startedAt);
      groups.set(day, [...(groups.get(day) ?? []), item]);
    }
    return [...groups];
  }, [sessions]);
  const copyLifetime = useRef({ sessionId, epoch: 0, alive: true });
  if (copyLifetime.current.sessionId !== sessionId) {
    copyLifetime.current.sessionId = sessionId;
    copyLifetime.current.epoch++;
  }
  useEffect(() => {
    copyLifetime.current.alive = true;
    return () => {
      copyLifetime.current.alive = false;
      copyLifetime.current.epoch++;
    };
  }, []);
  const filterCount = Number(caseSensitive) + Number(!!from || !!to);
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
    if (!filtersOpen && !exportOpen) return;
    const dismiss = (event: PointerEvent) => {
      if (!filterHost.current?.contains(event.target as Node))
        setFiltersOpen(false);
      if (!exportHost.current?.contains(event.target as Node))
        setExportOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      const host = exportOpen ? exportHost : filterHost;
      host.current?.querySelector<HTMLButtonElement>("button")?.focus();
      setFiltersOpen(false);
      setExportOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", escape, true);
    };
  }, [filtersOpen, exportOpen]);

  useEffect(() => {
    let cancelled = false,
      pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const next = await api.sessions(
          !global && binding && scope === "session"
            ? { workspaceId: binding.workspaceId }
            : !global && binding && scope === "source"
              ? { sourceId: binding.sourceId }
              : {},
        );
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
  }, [binding?.sourceId, binding?.workspaceId, global, scope]);

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
    setSelectedRecord(null);
    selectionAnchor.current = null;
    setUpdates(false);
    const oldRequest = requestId.current;
    const thisRequest = crypto.randomUUID();
    requestId.current = thisRequest;
    void api.cancel(oldRequest).catch(() => {});
    const load = async () => {
      try {
        if (
          query.from !== undefined &&
          query.to !== undefined &&
          query.from > query.to
        ) {
          setError("开始时间不能晚于结束时间");
          return;
        }
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
          if (pendingSelection.current?.sessionId === sessionId) {
            const target = pendingSelection.current;
            setSelected({ from: target.id, to: target.id });
            setSelectedRecord(target);
            selectionAnchor.current = target.id;
            pendingSelection.current = null;
          }
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
    pendingSelection.current = row;
    setSessionId(row.sessionId);
    setKeyword("");
    setDraft("");
    setReload((v) => v + 1);
  }
  async function copy(fromId: string, toId: string) {
    const epoch = copyLifetime.current.epoch;
    const valid = () =>
      copyLifetime.current.alive && copyLifetime.current.epoch === epoch;
    try {
      if (await copyText(() => api.copy(sessionId, fromId, toId), valid)) {
        if (valid()) setNotice("已复制历史记录");
      }
    } catch {
      if (valid()) setError("复制失败，请重试");
    }
  }
  async function copySelectedText(text: string) {
    const epoch = copyLifetime.current.epoch;
    const valid = () =>
      copyLifetime.current.alive && copyLifetime.current.epoch === epoch;
    try {
      if (await copyText(text, valid)) {
        if (valid()) setNotice("已复制");
      }
    } catch {
      if (valid()) setError("复制失败，请重试");
    }
  }
  async function exportSession(raw: boolean, range = false) {
    if (!sessionId) return;
    setExportOpen(false);
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
    const start =
      extend && selectionAnchor.current ? selectionAnchor.current : row.id;
    if (!extend || !selectionAnchor.current) selectionAnchor.current = row.id;
    setSelectedRecord(row);
    setSelected(
      BigInt(start) <= BigInt(row.id)
        ? { from: start, to: row.id }
        : { from: row.id, to: start },
    );
  }
  function chooseSession(id: string) {
    anchor.current = undefined;
    pendingSelection.current = null;
    setSessionId(id);
    setKeyword("");
    setDraft("");
    setReplay(null);
    setNotice("");
  }
  function openReplay(row: HistoryRow, trigger: HTMLElement) {
    replayTrigger.current = trigger;
    setReplay(row);
    setFiltersOpen(false);
    setExportOpen(false);
  }
  function closeReplay() {
    setReplay(null);
    requestAnimationFrame(() => {
      if (replayTrigger.current?.isConnected) replayTrigger.current.focus();
      else searchInput.current?.focus();
    });
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
        className={`terminal-history-row${row.kind === "marker" ? " history-marker" : ""}${inRange ? " history-selected" : ""}`}
        data-history-id={row.id}
        onClick={(event) => {
          if (
            searching ||
            (event.target as HTMLElement).closest("button, summary, a") ||
            window.getSelection()?.toString()
          )
            return;
          selectRow(row, event.shiftKey);
        }}
      >
        {searching ? (
          <time
            dateTime={new Date(row.time).toISOString()}
            title={new Date(row.time).toLocaleString()}
          >
            {recordTime(row.time)}
          </time>
        ) : (
          <button
            className="history-select-record"
            aria-label={`选择 ${recordTime(row.time)} 的记录（第 ${row.id} 条）`}
            aria-pressed={!!inRange}
            title={`${new Date(row.time).toLocaleString()} · 第 ${row.id} 条记录 · Shift 点击选择范围`}
            onClick={(event) => selectRow(row, event.shiftKey)}
          >
            <time dateTime={new Date(row.time).toISOString()}>
              {recordTime(row.time)}
            </time>
          </button>
        )}
        <div className="history-row-content">
          {row.kind === "screen" || row.kind === "revision" ? (
            <details open={searching || undefined}>
              <summary>
                {row.kind === "screen" ? "全屏程序画面" : "行内内容更新"} ·{" "}
                {row.text.slice(0, 72)}
              </summary>
              <pre>{text}</pre>
            </details>
          ) : (
            <pre>{text}</pre>
          )}
        </div>
      </div>
    );
  };
  const scopeLabel =
    scope === "all"
      ? "全部终端"
      : scope === "source"
        ? "此服务器 / 容器"
        : "本次连接";
  return (
    <section
      className={`terminal-history-view ${global ? "history-global" : "history-embedded"}`}
      aria-label="终端历史"
      onCopy={(event) => {
        if (
          (event.target as HTMLElement).closest(
            "input, textarea, [contenteditable=true]",
          )
        )
          return;
        const text = historySelection(event.currentTarget);
        if (text) {
          event.preventDefault();
          void copySelectedText(text);
        }
      }}
      onKeyDown={(event) => {
        if (
          !replay &&
          !event.nativeEvent.isComposing &&
          (event.ctrlKey || event.metaKey) &&
          !event.altKey &&
          event.key.toLowerCase() === "c" &&
          !(event.target as HTMLElement).closest(
            "input, textarea, [contenteditable=true]",
          )
        ) {
          const text = historySelection(event.currentTarget);
          if (text || selected) {
            event.preventDefault();
            event.stopPropagation();
            if (!event.repeat) {
              if (text) void copySelectedText(text);
              else if (selected) void copy(selected.from, selected.to);
            }
            return;
          }
        }
        if (
          (event.ctrlKey || event.metaKey) &&
          event.key.toLowerCase() === "f" &&
          !replay
        ) {
          event.preventDefault();
          searchInput.current?.focus();
        }
        if (event.key === "Escape" && !replay && keyword) {
          event.preventDefault();
          setKeyword("");
          setDraft("");
          searchInput.current?.focus();
        }
      }}
    >
      <div className="history-layout">
        {global && (
          <aside className="history-sidebar" aria-label="历史会话导航">
            <h2>最近会话</h2>
            {sessionGroups.map(([day, items]) => (
              <section
                className="history-session-group"
                key={day}
                aria-label={day}
              >
                <h3>{day}</h3>
                {items.map((item) => (
                  <button
                    key={item.id}
                    className="history-session-item"
                    aria-pressed={sessionId === item.id}
                    title={`${historyTitle(item)} · ${item.source.username}@${item.source.address} · ${item.id.slice(0, 8)}`}
                    onClick={() => chooseSession(item.id)}
                  >
                    {item.source.containerName ? (
                      <Box size={16} aria-hidden="true" />
                    ) : (
                      <Server size={16} aria-hidden="true" />
                    )}
                    <span className="history-session-copy">
                      <strong>
                        {item.source.containerName || item.source.serverName}
                      </strong>
                      <span>
                        {item.source.containerName
                          ? `${item.source.serverName} · Docker`
                          : `${item.source.username}@${item.source.address}`}
                      </span>
                      <span
                        className={
                          item.error || item.state === "interrupted"
                            ? "history-session-warning"
                            : undefined
                        }
                      >
                        {item.state === "recording" && !item.error && (
                          <i
                            className="history-record-dot"
                            aria-hidden="true"
                          />
                        )}
                        {sessionTime(item)} · {sessionState(item)}
                      </span>
                    </span>
                  </button>
                ))}
              </section>
            ))}
            {!sessions.length && (
              <p className="history-sidebar-empty">暂无已保存的会话</p>
            )}
          </aside>
        )}
        <div className="history-detail">
          <div
            className={`history-reader${replay ? " history-reader-covered" : ""}`}
            inert={!!replay}
            aria-hidden={!!replay}
          >
            <div className="history-session-picker">
              <label>
                会话
                <select
                  aria-label="历史会话"
                  value={sessionId}
                  onChange={(event) => chooseSession(event.target.value)}
                >
                  {!sessions.length && <option value="">暂无历史</option>}
                  {sessionGroups
                    .flatMap(([, items]) => items)
                    .map((item) => (
                      <option key={item.id} value={item.id}>
                        {item.source.containerName
                          ? `${item.source.serverName} / ${item.source.containerName}`
                          : item.source.serverName}{" "}
                        · {dayLabel(item.startedAt)} {sessionTime(item)} ·{" "}
                        {sessionState(item)}
                      </option>
                    ))}
                </select>
              </label>
            </div>
            <header className="history-session-header">
              <div className="history-session-identity">
                <div className="history-session-source">
                  {session?.source.containerName
                    ? `${session.source.serverName} / Docker`
                    : "SSH 终端"}
                </div>
                <h2>
                  {session
                    ? session.source.containerName || session.source.serverName
                    : "终端历史"}
                </h2>
                <p>
                  {session
                    ? `${session.source.username}@${session.source.address} · ${dayLabel(session.startedAt)} ${sessionTime(session)} · ${sessionState(session)}`
                    : "查找服务器和容器中已保存的终端内容"}
                </p>
              </div>
              <div className="history-export" ref={exportHost}>
                <button
                  className="history-outlined"
                  disabled={!sessionId}
                  aria-expanded={exportOpen}
                  aria-controls={exportId}
                  onClick={() => {
                    setExportOpen(!exportOpen);
                    setFiltersOpen(false);
                  }}
                >
                  <Download size={15} aria-hidden="true" />
                  导出
                  <ChevronDown size={13} aria-hidden="true" />
                </button>
                {exportOpen && (
                  <div
                    className="history-export-popover"
                    id={exportId}
                    role="group"
                    aria-label="导出格式"
                  >
                    <button onClick={() => void exportSession(false)}>
                      导出会话文本<span>.txt</span>
                    </button>
                    <button onClick={() => void exportSession(true)}>
                      导出原始记录<span>.jsonl</span>
                    </button>
                  </div>
                )}
              </div>
            </header>
            <form
              className="history-search"
              onSubmit={(event) => {
                event.preventDefault();
                setKeyword(draft.trim());
                setReload((value) => value + 1);
              }}
            >
              <div className="history-search-field">
                <Search size={16} aria-hidden="true" />
                <input
                  ref={searchInput}
                  type="search"
                  aria-label="搜索终端历史"
                  placeholder="搜索已保存的终端内容…"
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  maxLength={4096}
                />
                <button
                  className="history-text-button"
                  type="submit"
                  disabled={busy}
                >
                  搜索
                </button>
              </div>
              <div className="history-search-options">
                <div
                  className="history-search-scopes"
                  role="group"
                  aria-label="搜索范围"
                >
                  {(
                    [
                      ["session", "本次连接"],
                      ["source", "此服务器 / 容器"],
                      ["all", "全部终端"],
                    ] as const
                  ).map(([value, label]) => (
                    <button
                      type="button"
                      key={value}
                      aria-pressed={scope === value}
                      disabled={value !== "all" && !sessionId}
                      onClick={() => setScope(value)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <div className="history-filter" ref={filterHost}>
                  <button
                    type="button"
                    className="history-filter-trigger"
                    aria-expanded={filtersOpen}
                    aria-controls={filterId}
                    onClick={() => {
                      setFiltersOpen(!filtersOpen);
                      setExportOpen(false);
                    }}
                  >
                    <SlidersHorizontal size={14} aria-hidden="true" />
                    筛选{filterCount > 0 && <span> · {filterCount}</span>}
                  </button>
                  {filtersOpen && (
                    <div
                      className="history-filter-popover"
                      id={filterId}
                      role="group"
                      aria-label="历史搜索筛选"
                    >
                      <div className="history-filter-heading">
                        <strong>筛选</strong>
                        <button
                          type="button"
                          aria-label="关闭筛选"
                          onClick={() => {
                            setFiltersOpen(false);
                            filterHost.current
                              ?.querySelector("button")
                              ?.focus();
                          }}
                        >
                          <X size={15} aria-hidden="true" />
                        </button>
                      </div>
                      <label className="history-case-filter">
                        <input
                          type="checkbox"
                          checked={caseSensitive}
                          onChange={(event) =>
                            setCaseSensitive(event.target.checked)
                          }
                        />
                        区分大小写
                      </label>
                      <fieldset>
                        <legend>时间范围</legend>
                        <div className="history-time-fields">
                          <label>
                            开始
                            <input
                              type="datetime-local"
                              value={from}
                              onChange={(event) => setFrom(event.target.value)}
                            />
                          </label>
                          <label>
                            结束
                            <input
                              type="datetime-local"
                              value={to}
                              onChange={(event) => setTo(event.target.value)}
                            />
                          </label>
                        </div>
                      </fieldset>
                      {filterCount > 0 && (
                        <button
                          type="button"
                          className="history-text-button"
                          onClick={() => {
                            setCaseSensitive(false);
                            setFrom("");
                            setTo("");
                          }}
                        >
                          清除筛选
                        </button>
                      )}
                    </div>
                  )}
                </div>
              </div>
            </form>
            {(session?.error || error) && (
              <div className="history-error" role="alert">
                <CircleAlert size={16} aria-hidden="true" />
                <span>{error || session?.error}</span>
                <button onClick={() => setReload((value) => value + 1)}>
                  重试读取
                </button>
              </div>
            )}
            <div className="history-status">
              <span role="status">
                {busy
                  ? "正在读取…"
                  : search
                    ? `${scopeLabel} · 本页 ${search.hits.length} 条匹配`
                    : session
                      ? `${dayLabel(session.startedAt)} · 终端输出`
                      : "暂无终端历史"}
              </span>
              {session && session.savedSeq !== session.indexedSeq && (
                <span className="history-indexing">正在整理最新记录</span>
              )}
              {busy ? (
                <button
                  className="history-text-button"
                  onClick={() => {
                    void api.cancel(requestId.current).catch(() => {});
                    current.current.epoch++;
                    current.current.busy = false;
                    setBusy(false);
                    setNotice("查询已取消");
                  }}
                >
                  取消查询
                </button>
              ) : (
                (updates || (!keyword && newer)) && (
                  <button
                    className="history-text-button"
                    onClick={() => {
                      anchor.current = undefined;
                      setReload((value) => value + 1);
                    }}
                  >
                    <ArrowDown size={14} aria-hidden="true" />
                    {keyword ? "更新搜索结果" : "查看最新记录"}
                  </button>
                )
              )}
              {keyword && (
                <button
                  className="history-text-button"
                  onClick={() => {
                    setKeyword("");
                    setDraft("");
                  }}
                >
                  返回记录
                </button>
              )}
            </div>
            <div
              className="history-list"
              aria-busy={busy}
              onWheel={() => {
                scrollIntent.current = true;
              }}
              onTouchMove={() => {
                scrollIntent.current = true;
              }}
              onPointerDown={(event) => {
                if (
                  (event.target as HTMLElement).hasAttribute(
                    "data-virtuoso-scroller",
                  )
                )
                  scrollIntent.current = true;
              }}
              onKeyDown={(event) => {
                if (
                  [
                    "ArrowUp",
                    "ArrowDown",
                    "PageUp",
                    "PageDown",
                    "Home",
                    "End",
                    " ",
                  ].includes(event.key)
                )
                  scrollIntent.current = true;
              }}
            >
              {search ? (
                <Virtuoso
                  key="search"
                  data={search.hits}
                  computeItemKey={(_, hit) => `${hit.session.id}:${hit.row.id}`}
                  itemContent={(_, hit) => (
                    <article className="history-search-hit">
                      <div className="history-hit-source">
                        {hit.session.source.containerName ? (
                          <Box size={15} aria-hidden="true" />
                        ) : (
                          <Server size={15} aria-hidden="true" />
                        )}
                        <div>
                          <strong>
                            {hit.session.source.containerName ||
                              hit.session.source.serverName}
                          </strong>
                          <span>
                            {hit.session.source.containerName
                              ? `${hit.session.source.serverName} · `
                              : ""}
                            {hit.session.source.username}@
                            {hit.session.source.address} ·{" "}
                            {dayLabel(hit.session.startedAt)}{" "}
                            {timeLabel(hit.session.startedAt)}
                          </span>
                        </div>
                      </div>
                      {renderRow(hit.row, true)}
                      <div className="history-hit-actions">
                        <button
                          className="history-text-button"
                          onClick={() => context(hit.row)}
                        >
                          查看前后记录
                        </button>
                        <button
                          className="history-text-button"
                          onClick={(event) =>
                            openReplay(hit.row, event.currentTarget)
                          }
                        >
                          <Monitor size={14} aria-hidden="true" />
                          查看当时屏幕
                        </button>
                      </div>
                    </article>
                  )}
                  components={{
                    EmptyPlaceholder: () => (
                      <div className="history-empty">
                        <Search size={24} aria-hidden="true" />
                        <h3>没有找到匹配内容</h3>
                        <p>试试其他关键词，或扩大搜索范围。</p>
                        {scope !== "all" && (
                          <button onClick={() => setScope("all")}>
                            搜索全部终端
                          </button>
                        )}
                      </div>
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
                    if (row)
                      positionCallback.current?.({ sessionId, rowId: row.id });
                  }}
                  itemContent={(_, row) => renderRow(row)}
                  components={{
                    EmptyPlaceholder: () => (
                      <div className="history-empty">
                        <History size={24} aria-hidden="true" />
                        <h3>
                          {busy
                            ? "正在读取历史…"
                            : error
                              ? "暂时无法读取历史"
                              : "暂无可显示记录"}
                        </h3>
                        <p>
                          {busy
                            ? ""
                            : error
                              ? "已保存内容仍在本机，请重试读取。"
                              : sessions.length
                                ? "已保存的输出会在整理完成后出现。"
                                : "连接服务器或容器后，终端输出会自动保存在本机。"}
                        </p>
                      </div>
                    ),
                  }}
                />
              )}
            </div>
            {!keyword && (
              <div className="history-selection-bar">
                <span>
                  {selected
                    ? selected.from === selected.to && selectedRecord
                      ? `已选 ${recordTime(selectedRecord.time)} 的记录`
                      : `已选第 ${selected.from}–${selected.to} 条记录`
                    : "选择记录查看操作 · Shift 可选择范围"}
                </span>
                {selected && (
                  <div className="history-selection-actions">
                    <button
                      className="history-outlined"
                      onClick={() => void copy(selected.from, selected.to)}
                    >
                      <Copy size={14} aria-hidden="true" />
                      {selected.from === selected.to ? "复制" : "复制范围"}
                    </button>
                    {selected.from !== selected.to && (
                      <button
                        className="history-outlined"
                        onClick={() => void exportSession(false, true)}
                      >
                        导出范围
                      </button>
                    )}
                    {selectedRecord && (
                      <button
                        className="history-text-button"
                        onClick={(event) =>
                          openReplay(selectedRecord, event.currentTarget)
                        }
                      >
                        <Monitor size={14} aria-hidden="true" />
                        查看当时屏幕
                      </button>
                    )}
                    <button
                      className="history-clear-selection"
                      aria-label="取消选择"
                      onClick={() => {
                        setSelected(null);
                        setSelectedRecord(null);
                        selectionAnchor.current = null;
                      }}
                    >
                      <X size={14} aria-hidden="true" />
                    </button>
                  </div>
                )}
              </div>
            )}
            <nav
              className="history-pagination"
              aria-label={search ? "搜索结果翻页" : "历史记录翻页"}
            >
              {search ? (
                <>
                  <button
                    disabled={busy || !searchPage}
                    onClick={() => void searchNext(searchPage - 1)}
                  >
                    <ChevronLeft size={14} aria-hidden="true" />
                    上一页
                  </button>
                  <span>搜索结果 · 第 {searchPage + 1} 页</span>
                  <button
                    disabled={busy || !search.cursor}
                    onClick={() => void searchNext(searchPage + 1)}
                  >
                    下一页
                    <ChevronRight size={14} aria-hidden="true" />
                  </button>
                </>
              ) : (
                <>
                  <button
                    disabled={busy || !older}
                    onClick={() => void more("older", true)}
                  >
                    <ChevronLeft size={14} aria-hidden="true" />
                    更早记录
                  </button>
                  <span>已加载 {rows.length} 条</span>
                  <button
                    disabled={busy || !newer}
                    onClick={() => void more("newer", true)}
                  >
                    更晚记录
                    <ChevronRight size={14} aria-hidden="true" />
                  </button>
                </>
              )}
            </nav>
          </div>
          {replay && (
            <TerminalReplayView
              row={replay}
              session={sessions.find((item) => item.id === replay.sessionId)}
              onClose={closeReplay}
            />
          )}
        </div>
      </div>
      <footer className="history-footer">
        <span>
          <LockKeyhole size={12} aria-hidden="true" />
          只读历史 · 保存在本机
        </span>
        <span role="status">{notice || "断线后仍可查看"}</span>
      </footer>
    </section>
  );
}

export function TerminalHistoryApp() {
  return (
    <main className="terminal-history-app">
      <WindowTitlebar
        leading={<History size={18} aria-hidden="true" />}
        title="终端历史"
        subtitle="SoDeck · 查找已保存的终端内容"
        actions={
          <span className="history-local-label">
            <HardDrive size={14} aria-hidden="true" />
            保存在本机
          </span>
        }
      />
      <TerminalHistoryView global />
    </main>
  );
}
