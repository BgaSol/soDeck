import { useEffect, useRef, useState } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ArrowRightLeft,
  ChevronDown,
  ChevronUp,
  RotateCw,
  X,
  CircleHelp,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { transferApi, transferActive } from "./transferApi";
import type { TransferTask } from "./transferApi";
import type { TransferAuth } from "./transferApi";
import { TransferAuthFields } from "./TransferAuthFields";
import { errorOf, formatSize } from "./types";
import "./files.css";
const labels: Record<TransferTask["status"], string> = {
  queued: "等待传输",
  checking: "检查连接与权限",
  scanning: "扫描目录",
  running: "正在传输",
  verifying: "正在校验",
  committing: "提交文件",
  cancelling: "正在停止",
  completed: "已完成",
  failed: "失败",
  partialFailed: "部分完成",
  cancelled: "已取消",
  unknown: "状态待确认",
  interrupted: "已中断，请检查目标",
  needsAttention: "需要处理",
};
export function TransferPanel() {
  const [tasks, setTasks] = useState<TransferTask[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [retrying, setRetrying] = useState<TransferTask | null>(null);
  const [retryAuth, setRetryAuth] = useState<TransferAuth>({
    mode: "savedPassword",
  });
  const [resume, setResume] = useState(false);
  const retryDialog = useRef<HTMLDialogElement>(null);
  const samples = useRef(
    new Map<string, { at: number; bytes: number; run: string }>(),
  );
  const [rates, setRates] = useState<Record<string, number>>({});
  useEffect(() => {
    if (retrying) retryDialog.current?.showModal();
    else retryDialog.current?.close();
  }, [retrying]);
  const seen = useRef(new Set<string>());
  useEffect(() => {
    if (!desktop) return;
    let live = true,
      polling = false;
    async function refresh() {
      if (polling) return;
      polling = true;
      try {
        const next = await transferApi.list();
        if (!live) return;
        if (next.some((t) => !seen.current.has(t.id))) setExpanded(true);
        seen.current = new Set(next.map((t) => t.id));
        setTasks(next);
        const now = performance.now();
        const rate: Record<string, number> = {};
        for (const task of next) {
          const before = samples.current.get(task.id);
          if (
            before?.run === task.runId &&
            task.status === "running" &&
            now > before.at &&
            task.bytesDone >= before.bytes
          ) {
            rate[task.id] =
              ((task.bytesDone - before.bytes) * 1000) / (now - before.at);
          }
          samples.current.set(task.id, {
            at: now,
            bytes: task.bytesDone,
            run: task.runId,
          });
        }
        setRates(rate);
      } catch (e) {
        if (live) setError(errorOf(e).message);
      } finally {
        polling = false;
      }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 900);
    const started = () => {
      setExpanded(true);
      void refresh();
    };
    window.addEventListener("workspace-transfer-started", started);
    window.addEventListener("workspace-download-started", started);
    let off: (() => void) | undefined;
    void getCurrentWindow()
      .listen("transfer-updated", () => void refresh())
      .then((cleanup) => {
        if (live) off = cleanup;
        else cleanup();
      });
    return () => {
      live = false;
      clearInterval(timer);
      off?.();
      window.removeEventListener("workspace-transfer-started", started);
      window.removeEventListener("workspace-download-started", started);
    };
  }, []);
  async function action(
    task: TransferTask,
    type: "cancel" | "retry" | "reconcile" | "cleanup",
  ) {
    if (busy) return;
    setBusy(task.id);
    setError("");
    try {
      if (type === "retry") {
        await transferApi.retry(
          task.id,
          task.kind === "direct" ? retryAuth : undefined,
          resume,
        );
        setRetrying(null);
        setRetryAuth({ mode: "savedPassword" });
      } else await transferApi[type](task.id);
      setTasks(await transferApi.list());
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy(null);
    }
  }
  if (!tasks.length && !error) return null;
  const active = tasks.filter(transferActive).length;
  return (
    <section
      className={
        "downloads-panel transfer-panel" + (expanded ? " expanded" : "")
      }
      aria-label="传输任务"
    >
      <button
        className="downloads-toggle"
        aria-expanded={expanded}
        aria-controls="transfer-task-list"
        onClick={() => setExpanded(!expanded)}
      >
        <ArrowRightLeft size={15} />
        <strong>传输任务</strong>
        <span>
          {active ? `${active} 项正在进行` : `${tasks.length} 项任务`}
        </span>
        {expanded ? <ChevronDown size={15} /> : <ChevronUp size={15} />}
      </button>
      {expanded && (
        <div id="transfer-task-list" className="download-task-list">
          {tasks.map((task) => {
            const working = transferActive(task);
            const Icon =
              task.kind === "download"
                ? ArrowDownToLine
                : task.kind === "upload"
                  ? ArrowUpFromLine
                  : ArrowRightLeft;
            const total = task.bytesTotal;
            const done = task.bytesDone;
            // Receiving all bytes is not yet a successful commit.
            const fraction = total
              ? Math.min(working ? 0.99 : 1, done / total)
              : 0;
            return (
              <article
                className={`download-task ${task.status}`}
                key={task.id}
                data-task-id={task.id}
              >
                <div className="download-task-title">
                  <Icon size={15} />
                  <strong>
                    {task.kind === "download"
                      ? "下载"
                      : task.kind === "upload"
                        ? "上传"
                        : "服务器直传"}
                  </strong>
                  <span className="download-task-state">
                    {task.status === "completed" && task.warnings.length
                      ? "已完成，有处理说明"
                      : labels[task.status]}
                  </span>
                  {working ? (
                    <button
                      className="icon-button"
                      aria-label="取消传输"
                      disabled={
                        busy === task.id || task.status === "cancelling"
                      }
                      onClick={() => void action(task, "cancel")}
                    >
                      <X size={14} />
                    </button>
                  ) : ["unknown", "needsAttention", "interrupted"].includes(
                      task.status,
                    ) ? (
                    <button
                      className="quiet"
                      disabled={busy === task.id}
                      onClick={() => void action(task, "reconcile")}
                    >
                      <CircleHelp size={14} />
                      核对状态
                    </button>
                  ) : ["failed", "partialFailed", "cancelled"].includes(
                      task.status,
                    ) ? (
                    <button
                      className="icon-button"
                      aria-label="重试传输"
                      disabled={busy === task.id}
                      onClick={() => {
                        setResume(false);
                        setRetrying(task);
                      }}
                    >
                      <RotateCw size={14} />
                    </button>
                  ) : null}
                  {!working && task.status === "interrupted" && (
                    <button
                      className="quiet"
                      onClick={() => {
                        setResume(false);
                        setRetrying(task);
                      }}
                    >
                      重试未完成项
                    </button>
                  )}
                  {!working && task.cleanupPending && (
                    <button
                      className="quiet"
                      disabled={busy === task.id}
                      onClick={() => void action(task, "cleanup")}
                    >
                      清理临时资源
                    </button>
                  )}
                </div>
                <div className="transfer-route">
                  <span title={task.sourcePath}>
                    {task.sourceName} · {task.sourcePath}
                  </span>
                  <span aria-hidden="true">→</span>
                  <span title={task.targetPath}>
                    {task.targetName} · {task.targetPath}
                  </span>
                </div>
                {task.directAddress && (
                  <p className="transfer-detail">
                    本次直连 · {task.directAddress}
                  </p>
                )}
                {working && (
                  <progress
                    max={1}
                    value={total > 0 ? fraction : undefined}
                    aria-label="传输进度"
                  />
                )}
                <div className="transfer-counts">
                  已完成 {task.filesDone} / {task.filesTotal || "—"} 个文件 ·{" "}
                  {formatSize(task.bytesDone)}
                  {task.bytesTotal ? ` / ${formatSize(task.bytesTotal)}` : ""}
                  {task.skipped > 0 ? ` · 跳过 ${task.skipped} 项` : ""}
                </div>
                {task.waitingReason && (
                  <p className="transfer-detail" role="status">
                    {task.waitingReason}
                  </p>
                )}
                {task.status === "running" && rates[task.id] > 0 && (
                  <p className="transfer-detail">
                    {task.kind === "direct" ? "逻辑处理速度" : "传输速度"} ·{" "}
                    {formatSize(rates[task.id])}/s
                    {task.bytesTotal > task.bytesDone &&
                      ` · 预计剩余 ${Math.ceil((task.bytesTotal - task.bytesDone) / rates[task.id])} 秒`}
                  </p>
                )}
                {task.currentFile && working && (
                  <p className="transfer-detail" title={task.currentFile}>
                    {task.currentFile} · 已接收{" "}
                    {formatSize(task.currentBytes ?? 0)}
                    {task.kind === "direct" &&
                      ` · 已提交 ${formatSize(task.committedBytes ?? 0)}`}
                  </p>
                )}
                {task.error && <p className="panel-error">{task.error}</p>}
                {!working && task.cleanupPending && (
                  <p className="transfer-detail">
                    临时内容待清理，其他文件传输不受影响。
                  </p>
                )}
                {!!task.temporaryPaths?.length && (
                  <details>
                    <summary>临时资源位置</summary>
                    {task.temporaryPaths.map((p) => (
                      <p key={p}>{p}</p>
                    ))}
                  </details>
                )}
                {!!task.items?.length && (
                  <details>
                    <summary>项目处理详情</summary>
                    {task.items.map((item, index) => (
                      <p key={index}>
                        {item.source} · {item.error ?? item.status}
                      </p>
                    ))}
                  </details>
                )}
                {task.warnings.length > 0 && (
                  <details>
                    <summary>{task.warnings.length} 条处理说明</summary>
                    {task.warnings.map((w, i) => (
                      <p key={i}>{w}</p>
                    ))}
                  </details>
                )}
              </article>
            );
          })}
          {error && (
            <p className="panel-error" role="alert">
              {error}
            </p>
          )}
        </div>
      )}
      <dialog
        ref={retryDialog}
        className="file-operation-dialog"
        aria-label="重试未完成项目"
        onCancel={(e) => {
          e.preventDefault();
          if (!busy) {
            setRetrying(null);
            setRetryAuth({ mode: "savedPassword" });
          }
        }}
      >
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (retrying) void action(retrying, "retry");
          }}
        >
          <header className="dialog-head">
            <h2>重试未完成项目</h2>
          </header>
          <div className="file-operation-fields">
            <p>
              {retrying?.sourceName} → {retrying?.targetName} ·{" "}
              {retrying?.targetPath}
            </p>
            {retrying?.kind === "direct" && (
              <>
                <TransferAuthFields
                  value={retryAuth}
                  onChange={setRetryAuth}
                  disabled={!!busy}
                />
                <label>
                  <input
                    type="checkbox"
                    checked={resume}
                    disabled={!retrying.canResume || !!busy}
                    onChange={(e) => setResume(e.target.checked)}
                  />
                  复用已确认停止的临时数据
                </label>
              </>
            )}
            <p className="muted">
              默认使用新临时目录。已提交项目保留；旧执行未确认退出时不可复用或清理。
            </p>
            {error && (
              <p role="alert" className="panel-error">
                {error}
              </p>
            )}
          </div>
          <footer className="dialog-actions">
            <button
              type="button"
              disabled={!!busy}
              onClick={() => {
                setRetrying(null);
                setRetryAuth({ mode: "savedPassword" });
              }}
            >
              取消
            </button>
            <button className="primary" disabled={!!busy}>
              重试传输
            </button>
          </footer>
        </form>
      </dialog>
    </section>
  );
}
