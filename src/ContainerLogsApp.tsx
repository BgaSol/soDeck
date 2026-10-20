import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { ScrollText, RefreshCw, X } from "lucide-react";
import { WindowTitlebar } from "./WindowTitlebar";
import { errorOf } from "./types";
import {
  logApi,
  type ExportProgress,
  type ExportScope,
  type LogReplacement,
  type LogQuery,
} from "./logApi";
import { useLogSession } from "./logs/useLogSession";
import { useLogQuery } from "./logs/useLogQuery";
import { useLogPreferences } from "./logs/preferences";
import { readReducer, collectionLabel, validRange } from "./logs/model";
import { LogToolbar } from "./logs/LogToolbar";
import { LogReadingBar } from "./logs/LogReadingBar";
import { LogViewer, type ViewerHandle, type Anchor } from "./logs/LogViewer";
import "./files.css";
import "./logs.css";
function iso(value: string) {
  if (!value) return undefined;
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? undefined : date.toISOString();
}
function localInput(value: string) {
  const d = new Date(value);
  return new Date(d.valueOf() - d.getTimezoneOffset() * 60000)
    .toISOString()
    .slice(0, 19);
}
export function ContainerLogsApp() {
  const session = useLogSession();
  const [preferences, setPreferences] = useLogPreferences();
  const [read, dispatch] = useReducer(readReducer, { mode: "follow" });
  const [searchOpen, setSearchOpen] = useState(false),
    [draft, setDraft] = useState(""),
    [keyword, setKeyword] = useState(""),
    [caseSensitive, setCaseSensitive] = useState(false),
    [liveSearch, setLiveSearch] = useState(false),
    [stream, setStream] = useState(""),
    [from, setFrom] = useState(""),
    [to, setTo] = useState(""),
    [boot, setBoot] = useState("");
  const [rangeOpen, setRangeOpen] = useState(false),
    [rangeFrom, setRangeFrom] = useState(""),
    [rangeTo, setRangeTo] = useState(""),
    [notice, setNotice] = useState(""),
    [exporting, setExporting] = useState(false),
    [exportProgress, setExportProgress] = useState<ExportProgress | null>(null),
    [exportError, setExportError] = useState(""),
    [replacements, setReplacements] = useState<LogReplacement[] | null>(null),
    [finding, setFinding] = useState(false),
    [now, setNow] = useState(Date.now());
  const viewer = useRef<ViewerHandle>(null),
    returnAnchor = useRef<Anchor | null>(null),
    exportJob = useRef(""),
    alive = useRef(true);
  const selectedBoot = session.timeline.boots.find((b) => b.id === boot);
  useEffect(() => {
    if (
      selectedBoot &&
      !selectedBoot.finishedAt &&
      session.timeline.boots[0]?.id !== selectedBoot.id
    ) {
      setBoot("");
      setNotice("无法确认所选启动周期的结束时间，现按起始时间查看已保存日志。");
    }
  }, [selectedBoot, session.timeline.boots]);
  const currentBoot =
    !!selectedBoot &&
    !selectedBoot.finishedAt &&
    session.collection.container?.startedAt === selectedBoot.startedAt;
  const mode =
    ((from || to) && !currentBoot) || (keyword && !liveSearch)
      ? "snapshot"
      : "live";
  const filters = useMemo<LogQuery>(
    () => ({
      limit: 500,
      keyword,
      caseSensitive,
      stream,
      from: selectedBoot?.startedAt ?? iso(from),
      to: selectedBoot?.finishedAt ?? iso(to),
    }),
    [keyword, caseSensitive, stream, from, to, selectedBoot],
  );
  const query = useLogQuery(
    session.info,
    filters,
    mode,
    read.mode === "follow",
    session.revision,
    session.changedFrom,
  );
  const filtered = !!(keyword || stream || from || to);
  const selection = validRange(rangeFrom, rangeTo);
  const filterIdentity = JSON.stringify(filters);
  useEffect(() => {
    setRangeFrom("");
    setRangeTo("");
    window.getSelection()?.removeAllRanges();
  }, [filterIdentity]);
  const following =
    read.mode === "follow" && mode === "live" && query.context === null;
  useEffect(() => {
    const t = setTimeout(() => setKeyword(draft), 250);
    return () => clearTimeout(t);
  }, [draft]);
  useEffect(() => {
    if (mode === "snapshot") dispatch({ type: "pause", reason: "manual" });
  }, [mode]);
  useEffect(() => {
    if (session.collection.phase !== "reconnecting") return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [session.collection.phase]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (exportJob.current)
        void logApi.cancel(exportJob.current).catch(() => {});
    };
  }, []);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setSearchOpen(true);
      } else if (e.key === "Escape" && !e.defaultPrevented)
        setSearchOpen(false);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  const options = useMemo(
    () => ({
      timestamps: preferences.timestamps,
      highlight: preferences.highlight,
      keyword: query.ready ? keyword : "",
      caseSensitive,
    }),
    [
      preferences.timestamps,
      preferences.highlight,
      keyword,
      caseSensitive,
      query.ready,
    ],
  );
  const events = useMemo(
    () =>
      filtered && query.context === null
        ? []
        : session.timeline.events.filter(
            (e) => !query.snapshot || e.id <= query.snapshot.eventCursor,
          ),
    [session.timeline.events, query.snapshot, filtered, query.context],
  );
  function clearSelection() {
    setRangeFrom("");
    setRangeTo("");
    setRangeOpen(false);
    window.getSelection()?.removeAllRanges();
  }
  function clearFilters() {
    setDraft("");
    setKeyword("");
    setStream("");
    setFrom("");
    setTo("");
    setBoot("");
    clearSelection();
  }
  function onBoot(id: string) {
    setBoot(id);
    const b = session.timeline.boots.find((b) => b.id === id);
    if (b) {
      setFrom(localInput(b.startedAt));
      setTo(b.finishedAt ? localInput(b.finishedAt) : "");
    } else {
      setFrom("");
      setTo("");
    }
  }
  function resume() {
    if (following) {
      dispatch({ type: "pause", reason: "manual" });
      return;
    }
    clearSelection();
    if (query.context !== null) query.returnContext();
    if (mode === "snapshot") {
      setFrom("");
      setTo("");
      setBoot("");
      setLiveSearch(true);
    } else void query.load("latest");
    dispatch({ type: "resume" });
    viewer.current?.latest();
  }
  async function exportLogs(kind: "all" | "query" | "selection") {
    if (exporting || (kind !== "all" && !query.ready)) return;
    setExportError("");
    setExporting(true);
    setExportProgress(null);
    const requestId = crypto.randomUUID();
    exportJob.current = requestId;
    let temporarySnapshot: string | null = null;
    try {
      let scope: ExportScope;
      if (kind === "selection")
        scope = {
          type: "selection",
          from: Number(rangeFrom),
          to: Number(rangeTo),
          snapshotId: query.snapshot?.id,
        };
      else if (kind === "query") {
        const snapshot =
          query.snapshot ?? (await logApi.snapshot(requestId + "-snapshot"));
        if (!query.snapshot) temporarySnapshot = snapshot.id;
        if (!alive.current) return;
        scope = { type: "query", snapshotId: snapshot.id, query: filters };
      } else scope = { type: "all" };
      const output = new Channel<ExportProgress>();
      output.onmessage = (p) => {
        if (alive.current) setExportProgress(p);
      };
      const path = await logApi.exportScope(scope, requestId, output);
      if (alive.current) setNotice(path ? "已导出：" + path : "已取消导出");
    } catch (e) {
      if (alive.current) {
        const error = errorOf(e);
        if (error.kind === "cancelled") setNotice("已取消导出");
        else setExportError(error.message);
      }
    } finally {
      if (temporarySnapshot)
        void logApi.release(temporarySnapshot).catch(() => {});
      exportJob.current = "";
      if (alive.current) setExporting(false);
    }
  }
  async function cancelExport() {
    if (exportJob.current) {
      await Promise.all([
        logApi.cancel(exportJob.current),
        logApi.cancel(exportJob.current + "-snapshot"),
      ]);
    }
  }
  async function copy() {
    if (!query.ready) return;
    try {
      const text = await logApi.copy(
        Number(rangeFrom),
        Number(rangeTo),
        query.snapshot?.id,
      );
      await navigator.clipboard.writeText(text);
      setNotice("已复制完整原文");
    } catch (e) {
      setExportError(errorOf(e).message);
    }
  }
  async function findReplacement() {
    setFinding(true);
    setExportError("");
    try {
      setReplacements(await logApi.replacements());
    } catch (e) {
      setExportError(errorOf(e).message);
    } finally {
      setFinding(false);
    }
  }
  const newer = Math.max(
    0,
    (session.info?.stats.lastSeq ?? 0) - (query.rows.at(-1)?.seq ?? 0),
  );
  const reading = !query.ready
    ? query.busy
      ? "正在读取 · 保留上次结果"
      : "保留上次结果 · 当前条件尚未应用"
    : query.context !== null
      ? "查看日志 #" + query.context + " 的上下文"
      : mode === "snapshot"
        ? "历史搜索 · " +
          (query.snapshot
            ? "快照截至 #" + query.snapshot.lastSeq
            : "正在建立快照")
        : following
          ? "跟随最新"
          : session.collection.phase === "streaming"
            ? "已暂停滚动，仍在接收"
            : "已暂停滚动";
  const action = following
    ? "暂停滚动"
    : mode === "snapshot"
      ? from || to
        ? "退出时间范围并跟随"
        : "实时筛选并跟随"
      : newer
        ? filtered
          ? "继续跟随 · 有新日志"
          : "继续跟随 · 新增 " + newer + " 条"
        : "继续跟随";
  const error = exportError || query.error || session.error;
  const title = session.info
    ? session.info.containerName + " · 日志"
    : "容器日志";
  return (
    <div className="container-logs-app remote-editor-app">
      <WindowTitlebar
        leading={<ScrollText size={20} />}
        title={title}
        subtitle={
          session.info
            ? session.info.serverName +
              " · " +
              session.info.containerId.slice(0, 12)
            : "正在读取日志来源"
        }
        actions={
          <span
            className={"log-status log-status-" + session.collection.phase}
            role="status"
          >
            <span className="log-status-dot" />
            {collectionLabel(session.collection, now)}
          </span>
        }
        onError={(e) => session.setError(errorOf(e).message)}
      />
      <LogToolbar
        searchOpen={searchOpen}
        setSearchOpen={setSearchOpen}
        draft={draft}
        setDraft={setDraft}
        caseSensitive={caseSensitive}
        setCaseSensitive={setCaseSensitive}
        liveSearch={liveSearch}
        setLiveSearch={setLiveSearch}
        stream={stream}
        setStream={setStream}
        from={from}
        setFrom={(v) => {
          setBoot("");
          setFrom(v);
        }}
        to={to}
        setTo={(v) => {
          setBoot("");
          setTo(v);
        }}
        clear={clearFilters}
        preferences={preferences}
        setPreferences={setPreferences}
        exporting={exporting}
        records={!!session.info?.stats.records}
        queryReady={query.ready}
        filtered={filtered}
        selected={selection}
        onExport={(kind) => void exportLogs(kind)}
        onRange={() => setRangeOpen((v) => !v)}
        boots={session.timeline.boots}
        boot={boot}
        onBoot={onBoot}
      />
      {(session.collection.phase === "reconnecting" ||
        session.collection.phase === "failed" ||
        session.collection.phase === "waitingContainer") && (
        <div
          className={
            "log-connection-banner" +
            (session.collection.phase === "failed"
              ? " log-connection-failed"
              : "")
          }
        >
          <span>
            {session.collection.phase === "waitingContainer"
              ? "容器启动后会自动接续日志，当前内容已保留。"
              : session.collection.phase === "reconnecting"
                ? "连接恢复后会自动补读，阅读位置保持不变。"
                : session.collection.reason === "containerMissing"
                  ? "原容器已不存在，已保存的日志仍可查看。"
                  : "采集已停止，已保存的日志仍可查看。"}
          </span>
          {session.info?.canCollect &&
            session.collection.phase !== "waitingContainer" && (
              <button onClick={() => void session.retry()}>
                <RefreshCw size={14} />
                {session.collection.phase === "reconnecting"
                  ? "立即重试"
                  : "重试"}
              </button>
            )}
          {session.collection.reason === "containerMissing" && (
            <button disabled={finding} onClick={() => void findReplacement()}>
              {finding ? "正在查找…" : "查找新容器"}
            </button>
          )}
          {session.collection.error && (
            <details>
              <summary>原因</summary>
              <pre>{session.collection.error}</pre>
            </details>
          )}
        </div>
      )}
      {error && (
        <div className="editor-error" role="alert">
          {error}
          <button
            aria-label="关闭错误提示"
            onClick={() => {
              setExportError("");
              query.setError("");
              session.setError("");
            }}
          >
            <X size={14} />
          </button>
        </div>
      )}
      {replacements && (
        <div className="log-replacements" role="region" aria-label="替代容器">
          {replacements.length ? (
            replacements.map((r) => (
              <button
                key={r.id}
                onClick={() =>
                  void logApi
                    .openReplacement(r.id)
                    .catch((e) => setExportError(errorOf(e).message))
                }
              >
                查看 {r.name} · {r.id.slice(0, 12)} 的日志
              </button>
            ))
          ) : (
            <span>
              未发现身份匹配的替代容器。请从服务器的 Docker 列表打开目标容器。
            </span>
          )}
          <button
            onClick={() => setReplacements(null)}
            aria-label="收起替代容器"
          >
            <X size={14} />
          </button>
        </div>
      )}
      {(query.context !== null || mode === "snapshot" || query.busy) && (
        <div className="log-query-status">
          <span>
            {!query.ready
              ? query.busy
                ? "正在应用查询条件"
                : "查询未完成，可重试或继续跟随"
              : query.context !== null
                ? "日志 #" + query.context + " 的上下文"
                : mode === "snapshot"
                  ? "稳定搜索 · " + query.rows.length + " 条已加载"
                  : "正在读取日志"}
            {query.busy
              ? " · 已检索 " + query.scanned.toLocaleString() + " 条"
              : ""}
          </span>
          {query.context !== null ? (
            <button
              onClick={() => {
                viewer.current?.restore(returnAnchor.current);
                query.returnContext();
              }}
            >
              返回搜索结果
            </button>
          ) : (
            mode === "snapshot" && (
              <button disabled={query.busy} onClick={query.refresh}>
                {(session.info?.stats.lastSeq ?? 0) >
                (query.snapshot?.lastSeq ?? 0)
                  ? "有新日志 · 更新结果"
                  : "更新结果"}
              </button>
            )
          )}
          {query.busy && <button onClick={query.cancel}>取消读取</button>}
        </div>
      )}
      <LogViewer
        ref={viewer}
        rows={query.rows}
        events={events}
        options={options}
        wrap={preferences.wrap}
        fontSize={preferences.fontSize}
        tty={session.collection.tty ?? session.info?.tty ?? false}
        read={read}
        live={mode === "live"}
        busy={query.busy}
        older={query.ready && query.older}
        newer={query.ready && query.newer}
        context={query.context}
        keyword={query.ready ? keyword : ""}
        selectedFrom={rangeFrom}
        selectedTo={rangeTo}
        onRead={dispatch}
        onOlder={() => void query.load("older")}
        onNewer={() => void query.load("newer")}
        onSelect={(seq, extend) => {
          if (!query.ready) return;
          dispatch({ type: "pause", reason: "selection" });
          if (extend && rangeFrom) {
            setRangeFrom(String(Math.min(Number(rangeFrom), seq)));
            setRangeTo(String(Math.max(Number(rangeFrom), seq)));
          } else {
            setRangeFrom(String(seq));
            setRangeTo(String(seq));
          }
        }}
        onContext={(seq) => {
          if (!query.ready) return;
          returnAnchor.current = viewer.current?.capture() ?? null;
          dispatch({ type: "pause", reason: "manual" });
          query.showContext(seq);
        }}
        empty={
          filtered
            ? "没有匹配的已保存日志"
            : session.collection.phase === "streaming"
              ? "正在接收，等待新的日志输出…"
              : session.collection.phase === "offline"
                ? "此存档暂无日志"
                : "容器暂无日志"
        }
      />
      {rangeOpen && (
        <div className="log-range">
          <label>
            序号范围
            <input
              type="number"
              min="1"
              aria-label="复制起始序号"
              value={rangeFrom}
              onChange={(e) => {
                dispatch({ type: "pause", reason: "selection" });
                setRangeFrom(e.target.value);
              }}
            />
          </label>
          <span>—</span>
          <input
            type="number"
            min="1"
            aria-label="复制结束序号"
            value={rangeTo}
            onChange={(e) => setRangeTo(e.target.value)}
          />
          <button aria-label="收起序号选择" onClick={() => setRangeOpen(false)}>
            <X size={14} />
          </button>
        </div>
      )}
      {exporting && (
        <div className="log-query-status" role="status">
          <span>
            {exportProgress
              ? "正在导出 " +
                exportProgress.records.toLocaleString() +
                " 条 · " +
                (exportProgress.bytes / 1024 / 1024).toFixed(1) +
                " MiB"
              : "正在准备导出…"}
          </span>
          <button onClick={() => void cancelExport()}>取消导出</button>
        </div>
      )}
      <LogReadingBar
        status={reading}
        action={action}
        following={following}
        onAction={resume}
        count={session.info?.stats.records ?? 0}
        selected={selection}
        onCopy={() => void copy()}
        onExport={() => void exportLogs("selection")}
        onClear={clearSelection}
        notice={notice}
        busy={exporting || !query.ready}
      />
      <details className="log-source-details">
        <summary>日志详情</summary>
        <div>
          {session.info?.host}:{session.info?.port} ·{" "}
          {session.info?.containerId}
          <br />
          已保存 {((session.info?.stats.bytes ?? 0) / 1024 / 1024).toFixed(
            1,
          )}{" "}
          MiB · {session.info?.directory}
          <br />
          关闭日志窗口后停止采集，已保存的内容仍可离线查看。
          {session.collection.container && (
            <>
              <br />
              容器状态：{session.collection.container.state} · 最近确认{" "}
              {new Date(
                session.collection.container.observedAt,
              ).toLocaleString()}
            </>
          )}
        </div>
      </details>
    </div>
  );
}
