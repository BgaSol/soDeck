import { useEffect, useRef, useState } from "react";
import {
  ArrowRightLeft,
  ChevronDown,
  ChevronUp,
  CircleAlert,
} from "lucide-react";
import {
  transferApi,
  type TransferTask,
  type TransferAuth,
} from "./transferApi";
import { TransferAuthFields } from "./TransferAuthFields";
import { TransferPopover, type TransferAction } from "./TransferPopover";
import { summarizeTransfers } from "./transferPresentation";
import { useTransferTasks } from "./useTransferTasks";
import { errorOf } from "./types";
import "./files.css";
import "./transfers.css";

export function TransferPanel({ embedded = false }: { embedded?: boolean }) {
  const { tasks, rates, error, completion, refresh } = useTransferTasks();
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState<Record<string, string>>({});
  const pending = useRef(new Set<string>());
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [retrying, setRetrying] = useState<TransferTask | null>(null);
  const [retryAuth, setRetryAuth] = useState<TransferAuth>({
    mode: "savedPassword",
  });
  const [resume, setResume] = useState(false);
  const retryDialog = useRef<HTMLDialogElement>(null);
  const anchor = useRef<HTMLButtonElement>(null);
  const retryBusy = !!(retrying && busy[retrying.id]);
  const retryError = retrying ? errors[retrying.id] : "";

  useEffect(() => {
    if (retrying) retryDialog.current?.showModal();
    else retryDialog.current?.close();
  }, [retrying]);

  async function action(task: TransferTask, type: TransferAction | "retry") {
    if (pending.current.has(task.id)) return;
    pending.current.add(task.id);
    setBusy((current) => ({ ...current, [task.id]: type }));
    setErrors((current) => ({ ...current, [task.id]: "" }));
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
      await refresh();
    } catch (e) {
      setErrors((current) => ({ ...current, [task.id]: errorOf(e).message }));
    } finally {
      pending.current.delete(task.id);
      setBusy((current) => {
        const next = { ...current };
        delete next[task.id];
        return next;
      });
    }
  }

  function close(restoreFocus = false) {
    setExpanded(false);
    if (restoreFocus) anchor.current?.focus({ preventScroll: true });
  }

  if (!tasks.length && !error) return null;
  const summary = summarizeTransfers(tasks);
  return (
    <>
      <div
        className={`transfer-statusbar${embedded ? " embedded" : ""}`}
        aria-label="传输状态栏"
      >
        <button
          ref={anchor}
          type="button"
          className="transfer-status-trigger"
          aria-label="查看传输任务"
          aria-expanded={expanded}
          aria-controls="transfer-task-popover"
          onClick={() => setExpanded(!expanded)}
        >
          <ArrowRightLeft size={14} aria-hidden="true" />
          <span>
            {summary.active ? summary.label : completion || summary.label}
          </span>
          {summary.active > 0 ? (
            <progress
              max={1}
              value={summary.progress}
              aria-label="总传输进度"
            />
          ) : (
            <span className="transfer-history-count">{tasks.length}</span>
          )}
          {(summary.attention > 0 || error) && (
            <span
              className="transfer-status-alert"
              aria-label={
                error ? "任务刷新失败" : `${summary.attention} 项任务需要处理`
              }
              title={error || `${summary.attention} 项任务需要处理`}
            >
              <CircleAlert size={13} />
              {summary.attention || ""}
            </span>
          )}
          {expanded ? (
            <ChevronDown size={13} aria-hidden="true" />
          ) : (
            <ChevronUp size={13} aria-hidden="true" />
          )}
        </button>
        <span className="sr-only" role="status">
          {completion}
        </span>
      </div>
      <TransferPopover
        open={expanded}
        suspended={!!retrying}
        anchor={anchor}
        tasks={tasks}
        rates={rates}
        busy={busy}
        errors={errors}
        error={error}
        onClose={close}
        onAction={(task, type) => void action(task, type)}
        onRetry={(task) => {
          setErrors((current) => ({ ...current, [task.id]: "" }));
          setResume(false);
          setRetrying(task);
        }}
      />
      <dialog
        ref={retryDialog}
        className="file-operation-dialog"
        aria-label="重试未完成项目"
        onCancel={(e) => {
          e.preventDefault();
          if (!retryBusy) {
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
                  disabled={retryBusy}
                />
                <label>
                  <input
                    type="checkbox"
                    checked={resume}
                    disabled={!retrying.canResume || retryBusy}
                    onChange={(e) => setResume(e.target.checked)}
                  />
                  复用已确认停止的临时数据
                </label>
              </>
            )}
            <p className="muted">
              {retrying?.kind === "serverMove"
                ? "只继续未完成步骤。已经提交的文件不重复复制；删除来源前会再次校验，来源或目标变化时保留来源。"
                : "默认使用新临时目录。已提交项目保留；旧执行未确认退出时不可复用或清理。"}
            </p>
            {retryError && (
              <p role="alert" className="panel-error">
                {retryError}
              </p>
            )}
          </div>
          <footer className="dialog-actions">
            <button
              type="button"
              disabled={retryBusy}
              onClick={() => {
                setRetrying(null);
                setRetryAuth({ mode: "savedPassword" });
              }}
            >
              取消
            </button>
            <button className="primary" disabled={retryBusy}>
              重试传输
            </button>
          </footer>
        </form>
      </dialog>
    </>
  );
}
