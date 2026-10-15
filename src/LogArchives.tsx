import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowUpRight,
  Box,
  HardDrive,
  RefreshCw,
  ScrollText,
  Search,
  X,
} from "lucide-react";
import { logApi, type LogArchive } from "./logApi";
import { errorOf } from "./types";
import "./log-archives.css";

const number = new Intl.NumberFormat("zh-CN");
const date = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});
const time = new Intl.DateTimeFormat("zh-CN", {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
}

function timestamp(archive: LogArchive) {
  return Date.parse(archive.lastTimestamp ?? "") || 0;
}

function address(archive: LogArchive) {
  const host = archive.host.includes(":") ? `[${archive.host}]` : archive.host;
  return `${host}:${archive.port}`;
}

export function LogArchives() {
  const [archives, setArchives] = useState<LogArchive[]>([]);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState("recent");
  const [loadError, setLoadError] = useState("");
  const [openError, setOpenError] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(true);
  const [opening, setOpening] = useState<string | null>(null);
  const refreshing = useRef(false);
  const openingRef = useRef(false);
  const mounted = useRef(false);
  const search = useRef<HTMLInputElement>(null);

  const refresh = useCallback(async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    setBusy(true);
    try {
      const result = await logApi.archives();
      if (!mounted.current) return;
      setArchives(result);
      setLoadError("");
      setLoaded(true);
    } catch (e) {
      if (mounted.current) setLoadError(errorOf(e).message);
    } finally {
      refreshing.current = false;
      if (mounted.current) setBusy(false);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const onFocus = () => void refresh();
    const onVisibility = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      mounted.current = false;
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh]);

  async function open(archive: LogArchive) {
    if (openingRef.current) return;
    openingRef.current = true;
    setOpening(archive.sourceId);
    setOpenError("");
    try {
      await logApi.open(archive.sourceId);
    } catch (e) {
      if (mounted.current) setOpenError(errorOf(e).message);
    } finally {
      openingRef.current = false;
      if (mounted.current) setOpening(null);
    }
  }

  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return archives
      .filter((a) =>
        `${a.serverName} ${a.containerName} ${address(a)} ${a.containerId}`
          .toLocaleLowerCase()
          .includes(needle),
      )
      .sort((a, b) => {
        if (sort === "recent") {
          const difference = timestamp(b) - timestamp(a);
          if (difference) return difference;
        }
        if (sort === "size" && a.bytes !== b.bytes) return b.bytes - a.bytes;
        return (
          a.serverName.localeCompare(b.serverName, "zh-CN", {
            numeric: true,
          }) ||
          a.containerName.localeCompare(b.containerName, "zh-CN", {
            numeric: true,
          })
        );
      });
  }, [archives, query, sort]);

  const totalBytes = archives.reduce(
    (total, archive) => total + archive.bytes,
    0,
  );
  const totalRecords = archives.reduce(
    (total, archive) => total + archive.records,
    0,
  );
  const error = openError || (loaded ? loadError : "");

  function clearSearch() {
    setQuery("");
    search.current?.focus();
  }

  return (
    <main className="archive-app" aria-label="本地日志存档">
      <header className="archive-header">
        <div className="archive-heading">
          <span className="archive-mark">
            <ScrollText size={24} />
          </span>
          <div>
            <h1>本地日志</h1>
            <p>已保存在这台电脑上的容器日志，离线也能随时查看。</p>
          </div>
        </div>
        <span className="archive-local">
          <HardDrive size={14} />
          本机存档
        </span>
      </header>

      <section className="archive-content" aria-label="日志存档列表">
        <div className="archive-toolbar">
          <div className="archive-search">
            <Search size={17} aria-hidden="true" />
            <input
              ref={search}
              autoFocus
              type="text"
              aria-label="筛选日志存档"
              placeholder="搜索容器、服务器或地址…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") clearSearch();
              }}
            />
            {query && (
              <button
                className="icon-button"
                onClick={clearSearch}
                aria-label="清除搜索"
              >
                <X size={14} />
              </button>
            )}
          </div>
          <select
            aria-label="日志存档排序"
            value={sort}
            onChange={(e) => setSort(e.target.value)}
          >
            <option value="recent">最近日志优先</option>
            <option value="name">按服务器名称</option>
            <option value="size">占用空间优先</option>
          </select>
          <button
            className="archive-refresh"
            disabled={busy}
            onClick={() => void refresh()}
            aria-label="刷新日志存档"
          >
            <RefreshCw
              size={15}
              className={busy ? "archive-spinning" : undefined}
            />
            刷新
          </button>
        </div>
        <div className="archive-list-caption" role="status">
          <h2>
            全部存档{" "}
            <span>{loaded ? number.format(archives.length) : "—"}</span>
          </h2>
          <span>
            {busy
              ? "正在读取存档…"
              : query.trim() && loaded
                ? `找到 ${number.format(filtered.length)} 个存档`
                : "点击存档，在新窗口中查看日志"}
          </span>
        </div>
        {error && (
          <div className="archive-error" role="alert">
            <AlertCircle size={16} />
            <span>{error}</span>
          </div>
        )}

        <div className="archive-list-panel" aria-busy={busy}>
          {loaded && filtered.length > 0 ? (
            <div className="archive-table">
              <div className="archive-columns archive-grid" aria-hidden="true">
                <span>容器</span>
                <span>服务器</span>
                <span className="archive-numeric">日志 / 占用空间</span>
                <span>最近日志</span>
                <span />
              </div>
              <ul className="archive-list">
                {filtered.map((a) => {
                  const lastTime = timestamp(a);
                  return (
                    <li key={a.sourceId}>
                      <button
                        className="archive-row archive-grid"
                        aria-label={`打开 ${a.serverName} / ${a.containerName} 日志存档`}
                        disabled={opening !== null}
                        onClick={() => void open(a)}
                      >
                        <span className="archive-container">
                          <span className="archive-container-icon">
                            <Box size={19} />
                          </span>
                          <span className="archive-cell">
                            <strong title={a.containerName}>
                              {a.containerName}
                            </strong>
                            <small
                              className="archive-mono"
                              title={a.containerId}
                            >
                              {a.containerId.slice(0, 12)}
                            </small>
                          </span>
                        </span>
                        <span className="archive-cell">
                          <span title={a.serverName}>{a.serverName}</span>
                          <small title={address(a)}>{address(a)}</small>
                        </span>
                        <span className="archive-cell archive-numeric">
                          <span>{number.format(a.records)} 条</span>
                          <small>{size(a.bytes)}</small>
                        </span>
                        <span className="archive-cell archive-date">
                          <span>
                            {lastTime ? date.format(lastTime) : "暂无日志"}
                          </span>
                          <small>
                            {lastTime ? time.format(lastTime) : "尚无时间记录"}
                          </small>
                        </span>
                        {opening === a.sourceId ? (
                          <RefreshCw size={16} className="archive-spinning" />
                        ) : (
                          <ArrowUpRight
                            size={17}
                            className="archive-open-icon"
                          />
                        )}
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          ) : (
            <div
              className="archive-empty"
              role={!loaded && loadError ? "alert" : "status"}
            >
              <span className="archive-empty-icon">
                {!loaded && busy ? (
                  <RefreshCw size={28} className="archive-spinning" />
                ) : !loaded && loadError ? (
                  <AlertCircle size={28} />
                ) : query.trim() && archives.length ? (
                  <Search size={28} />
                ) : (
                  <ScrollText size={28} />
                )}
              </span>
              <h2>
                {!loaded && busy
                  ? "正在读取本地存档"
                  : !loaded && loadError
                    ? "无法读取日志存档"
                    : archives.length
                      ? "没有找到匹配的存档"
                      : "还没有本地日志"}
              </h2>
              <p>
                {!loaded && busy
                  ? "保存过的容器日志会显示在这里。"
                  : !loaded && loadError
                    ? loadError
                    : archives.length
                      ? "试试其他容器名称、服务器名称或地址。"
                      : "在服务器的 Docker 列表中打开容器日志，采集的内容会自动保存在这里。"}
              </p>
              {!loaded && loadError && !busy && (
                <button onClick={() => void refresh()}>
                  <RefreshCw size={14} />
                  重新读取
                </button>
              )}
              {loaded && archives.length > 0 && (
                <button onClick={clearSearch}>清除搜索</button>
              )}
            </div>
          )}
        </div>
      </section>

      <footer className="archive-footer">
        <span>
          <HardDrive size={13} />
          {loaded
            ? `${number.format(totalRecords)} 条日志 · 占用 ${size(totalBytes)}`
            : "本地日志存档"}
        </span>
        <span>服务器离线后仍可查看、搜索和导出</span>
      </footer>
    </main>
  );
}
