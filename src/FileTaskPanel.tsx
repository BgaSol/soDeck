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
import { FileTaskPopover } from "./FileTaskPopover";
import { fileTaskApi, type FileTask, type FileTaskAction } from "./fileTaskApi";
import { fileTaskProgress } from "./fileTaskPresentation";
import { useFileTasks } from "./useFileTasks";
import { DeleteFilesDialog } from "./DeleteFilesDialog";
import { fileApi, type DeletePreview } from "./fileApi";
import { errorOf } from "./types";
import "./files.css";
import "./transfers.css";

export function FileTaskPanel({ embedded = false }: { embedded?: boolean }) {
  const {
    tasks,
    summary,
    rates,
    error,
    completion,
    refresh,
    filter,
    setFilter,
    hasMore,
    loading,
    loadMore,
  } = useFileTasks();
  const [deleteDraft, setDeleteDraft] = useState<DeletePreview | null>(null);
  const [clearing, setClearing] = useState(false);
  const [clearError, setClearError] = useState("");
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

  async function perform(id: string, type: string, run: () => Promise<void>) {
    if (pending.current.has(id)) return;
    pending.current.add(id);
    setBusy((current) => ({ ...current, [id]: type }));
    setErrors((current) => ({ ...current, [id]: "" }));
    try {
      await run();
      await refresh();
    } catch (e) {
      setErrors((current) => ({ ...current, [id]: errorOf(e).message }));
    } finally {
      pending.current.delete(id);
      setBusy((current) => {
        const next = { ...current };
        delete next[id];
        return next;
      });
    }
  }
  function action(item: FileTask, type: FileTaskAction) {
    void perform(item.task.id, type, async () => {
      if (type === "cleanup") await transferApi.cleanup(item.task.id);
      else await fileTaskApi[type](item.task.id);
    });
  }
  function retry(item: FileTask) {
    if (item.category === "delete") {
      void perform(item.task.id, "retry", async () =>
        setDeleteDraft(await fileApi.prepareDeleteRetry(item.task.id)),
      );
    } else {
      setErrors((current) => ({ ...current, [item.task.id]: "" }));
      setResume(false);
      setRetrying(item.task);
    }
  }
  async function clearCompleted() {
    if (clearing) return;
    setClearing(true);
    setClearError("");
    try {
      await fileTaskApi.clearCompleted();
      await refresh();
    } catch (e) {
      setClearError(errorOf(e).message);
    } finally {
      setClearing(false);
    }
  }

  function close(restoreFocus = false) {
    setExpanded(false);
    if (restoreFocus) anchor.current?.focus({ preventScroll: true });
  }

  const progress = fileTaskProgress(tasks, summary.active);
  const label = summary.active
    ? `文件任务 · ${summary.active} 项进行中`
    : "文件任务";
  return (
    <>
      <div
        className={`transfer-statusbar${embedded ? " embedded" : ""}`}
        aria-label="文件任务状态栏"
      >
        <button
          ref={anchor}
          type="button"
          className="transfer-status-trigger"
          aria-label="查看文件任务"
          aria-expanded={expanded}
          aria-controls="file-task-popover"
          onClick={() => setExpanded(!expanded)}
        >
          <ArrowRightLeft size={14} aria-hidden="true" />
          <span>{completion || label}</span>
          {summary.active > 0 ? (
            <progress max={1} value={progress} aria-label="文件任务进度" />
          ) : (
            <span className="transfer-history-count">{summary.total}</span>
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
      <FileTaskPopover
        open={expanded}
        suspended={!!retrying || !!deleteDraft}
        anchor={anchor}
        tasks={tasks}
        summary={summary}
        filter={filter}
        onFilter={setFilter}
        hasMore={hasMore}
        loading={loading}
        onLoadMore={loadMore}
        onClearCompleted={() => void clearCompleted()}
        clearing={clearing}
        rates={rates}
        busy={busy}
        errors={errors}
        error={error || clearError}
        onClose={close}
        onAction={action}
        onRetry={retry}
      />
      {deleteDraft && (
        <DeleteFilesDialog
          draft={deleteDraft}
          onClose={() => setDeleteDraft(null)}
          onStarted={() => {
            setDeleteDraft(null);
            void refresh();
          }}
        />
      )}
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
            if (retrying)
              void perform(retrying.id, "retry", async () => {
                await transferApi.retry(
                  retrying.id,
                  retrying.kind === "direct" ? retryAuth : undefined,
                  resume,
                );
                setRetrying(null);
                setRetryAuth({ mode: "savedPassword" });
              });
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
