import { useEffect, useState } from "react";
import { ScrollText, X, RefreshCw } from "lucide-react";
import { logApi, type LogArchive } from "./logApi";
import { errorOf } from "./types";
import "./logs.css";
export function LogArchives({ onClose }: { onClose: () => void }) {
  const [archives, setArchives] = useState<LogArchive[]>([]),
    [query, setQuery] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(true);
  async function refresh() {
    setBusy(true);
    try {
      setArchives(await logApi.archives());
      setError("");
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    void refresh();
  }, []);
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <section
        className="log-archives"
        role="dialog"
        aria-modal="true"
        aria-label="本地日志存档"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
        }}
      >
        <header>
          <ScrollText size={20} />
          <h2>本地日志存档</h2>
          <button
            disabled={busy}
            onClick={() => void refresh()}
            aria-label="刷新日志存档"
          >
            <RefreshCw size={15} />
          </button>
          <button onClick={onClose} aria-label="关闭日志存档">
            <X size={16} />
          </button>
        </header>
        <p>
          已采集的日志永久保存在本机。服务器离线或容器删除后，仍可查看、搜索和导出。
        </p>
        <input
          autoFocus
          aria-label="筛选日志存档"
          placeholder="搜索服务器、容器名称或地址"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        {error && (
          <p role="alert" className="editor-error">
            {error}
          </p>
        )}
        <div className="log-archive-list">
          {archives
            .filter((a) =>
              `${a.serverName} ${a.containerName} ${a.host} ${a.containerId}`
                .toLowerCase()
                .includes(query.toLowerCase()),
            )
            .map((a) => (
              <button
                key={a.sourceId}
                onClick={() =>
                  void logApi
                    .open(a.sourceId)
                    .catch((e) => setError(errorOf(e).message))
                }
              >
                <span>
                  <strong>
                    {a.serverName} / {a.containerName}
                  </strong>
                  <small>
                    {a.host}:{a.port} · {a.containerId.slice(0, 12)}
                  </small>
                </span>
                <span>
                  {a.records} 条
                  <small>{(a.bytes / 1024 / 1024).toFixed(1)} MiB</small>
                </span>
              </button>
            ))}
          {!archives.length && (
            <p className="muted">
              {busy
                ? "正在读取存档…"
                : "暂无存档，从 Docker 树打开日志后会自动保存。"}
            </p>
          )}
        </div>
      </section>
    </div>
  );
}
