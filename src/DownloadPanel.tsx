import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronUp, Download, RotateCw, X } from "lucide-react";
import { desktop } from "./api";
import { fileApi, isDownloadActive } from "./fileApi";
import type { DownloadTask } from "./fileApi";
import { errorOf, formatSize } from "./types";
import "./files.css";

const statusLabels: Record<DownloadTask["status"], string> = {
  queued: "等待下载",
  scanning: "正在扫描目录",
  running: "正在下载",
  completed: "已完成",
  failed: "下载失败",
  cancelled: "已取消",
};

export function DownloadPanel() {
  const [tasks, setTasks] = useState<DownloadTask[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const loaded = useRef(false);
  const previousIds = useRef(new Set<string>());
  useEffect(() => {
    if (!desktop) return;
    let live = true,
      polling = false;
    async function refresh() {
      if (polling) return;
      polling = true;
      try {
        const next = await fileApi.downloads();
        if (!live) return;
        if (
          loaded.current &&
          next.some((task) => !previousIds.current.has(task.id))
        )
          setExpanded(true);
        loaded.current = true;
        previousIds.current = new Set(next.map((task) => task.id));
        setTasks(next);
        setError("");
      } catch (error) {
        if (live) setError(errorOf(error).message);
      } finally {
        polling = false;
      }
    }
    function newDownload() {
      setExpanded(true);
      void refresh();
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1000);
    window.addEventListener("workspace-download-started", newDownload);
    return () => {
      live = false;
      window.clearInterval(timer);
      window.removeEventListener("workspace-download-started", newDownload);
    };
  }, []);

  async function action(task: DownloadTask, retry: boolean) {
    if (busy) return;
    setBusy(task.id);
    setError("");
    try {
      if (retry) await fileApi.retryDownload(task.id);
      else await fileApi.cancelDownload(task.id);
      setTasks(await fileApi.downloads());
    } catch (error) {
      setError(errorOf(error).message);
    } finally {
      setBusy(null);
    }
  }
  const active = tasks.filter(isDownloadActive).length;
  const failed = tasks.filter((task) => task.status === "failed").length;
  if (!tasks.length && !error) return null;
  return (
    <section
      className={"downloads-panel" + (expanded ? " expanded" : "")}
      aria-label="下载任务"
    >
      <button
        className="downloads-toggle"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        aria-controls="download-task-list"
      >
        <Download size={15} />
        <strong>下载任务</strong>
        <span>
          {active ? active + " 项正在下载" : tasks.length + " 项任务"}
          {failed ? " · " + failed + " 项失败" : ""}
        </span>
        {expanded ? <ChevronDown size={15} /> : <ChevronUp size={15} />}
      </button>
      {expanded && (
        <div className="download-task-list" id="download-task-list">
          {tasks.map((task) => {
            const working = isDownloadActive(task);
            const fraction = task.bytesTotal
              ? task.bytesDone / task.bytesTotal
              : task.filesTotal
                ? task.filesDone / task.filesTotal
                : 0;
            return (
              <article
                className={"download-task " + task.status}
                key={task.id}
                data-task-id={task.id}
              >
                <div className="download-task-title">
                  <Download size={15} />
                  <strong title={task.remotePath}>
                    {task.remotePath.split("/").filter(Boolean).pop() || "/"}
                  </strong>
                  <span className="download-task-state">
                    {statusLabels[task.status]}
                  </span>
                  {working ? (
                    <button
                      className="icon-button"
                      aria-label="取消下载"
                      title="取消下载"
                      disabled={busy === task.id}
                      onClick={() => void action(task, false)}
                    >
                      <X size={14} />
                    </button>
                  ) : (
                    (task.status === "failed" ||
                      task.status === "cancelled") && (
                      <button
                        className="icon-button"
                        aria-label="重试下载"
                        title="重试下载"
                        disabled={busy === task.id}
                        onClick={() => void action(task, true)}
                      >
                        <RotateCw size={14} />
                      </button>
                    )
                  )}
                </div>
                <p
                  className="download-task-path"
                  title={task.serverName + ":" + task.remotePath}
                >
                  {task.serverName} · {task.remotePath}
                </p>
                <p className="download-task-path" title={task.localPath}>
                  保存到 {task.localPath}
                </p>
                {working && (
                  <progress
                    aria-label="下载进度"
                    max={1}
                    value={
                      task.status === "scanning"
                        ? undefined
                        : Math.max(0, Math.min(1, fraction))
                    }
                  />
                )}
                <p className="download-task-detail">
                  {formatSize(task.bytesDone)} / {formatSize(task.bytesTotal)} ·{" "}
                  {task.filesDone} / {task.filesTotal} 个文件
                  {task.skipped
                    ? " · 跳过 " + task.skipped + " 项（同名项目或符号链接等）"
                    : ""}
                </p>
                {task.error && (
                  <p className="error" role="alert">
                    {task.error}
                  </p>
                )}
              </article>
            );
          })}
        </div>
      )}
      {error && (
        <p className="panel-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
