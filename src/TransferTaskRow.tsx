import { useState } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ArrowRightLeft,
  CircleAlert,
} from "lucide-react";
import { transferActive, type TransferTask } from "./transferApi";
import type { TaskActions, FileTaskAction } from "./fileTaskApi";
import {
  taskNeedsAttention,
  transferLabels,
  transferProgress,
  transferTime,
  transferTitle,
} from "./transferPresentation";
import { formatSize } from "./types";

export function TransferTaskRow({
  task,
  actions,
  rate,
  busy,
  error,
  onAction,
  onRetry,
}: {
  task: TransferTask;
  actions: TaskActions;
  rate: number;
  busy?: string;
  error?: string;
  onAction: (task: TransferTask, action: FileTaskAction) => void;
  onRetry: (task: TransferTask) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const working = transferActive(task);
  const progress = transferProgress(task);
  const Icon =
    task.kind === "download"
      ? ArrowDownToLine
      : task.kind === "upload"
        ? ArrowUpFromLine
        : ArrowRightLeft;
  const label =
    task.status === "completed" && task.warnings.length
      ? "已完成，有处理说明"
      : transferLabels[task.status];
  const stamp = task.lastStartedAt ?? task.createdAt;
  return (
    <article
      className="transfer-task-row"
      data-status={task.status}
      data-task-id={task.id}
    >
      <div className="transfer-task-heading">
        <Icon size={15} aria-hidden="true" />
        <strong title={transferTitle(task)}>{transferTitle(task)}</strong>
        <time
          dateTime={stamp ? new Date(stamp).toISOString() : undefined}
          title={stamp ? new Date(stamp).toLocaleString() : undefined}
        >
          {transferTime(stamp)}
        </time>
      </div>
      <p
        className="transfer-task-route"
        title={`${task.sourceName} → ${task.targetName} · ${task.targetPath}`}
      >
        {task.sourceName} → {task.targetName} <span>· {task.targetPath}</span>
      </p>
      {working && <progress max={1} value={progress} aria-label="传输进度" />}
      <div className="transfer-task-meta">
        <span
          className={`transfer-state${taskNeedsAttention(task) ? " attention" : ""}`}
        >
          {taskNeedsAttention(task) && (
            <CircleAlert size={12} aria-hidden="true" />
          )}
          {label}
          {progress !== undefined && ` · ${Math.floor(progress * 100)}%`}
        </span>
        {task.status === "running" && rate > 0 && (
          <span className="transfer-rate">
            · {task.kind === "direct" ? "逻辑 " : ""}
            {formatSize(rate)}/s
          </span>
        )}
        <div className="transfer-row-actions">
          {actions.stop && (
            <button
              type="button"
              disabled={!!busy || task.status === "cancelling"}
              aria-label="取消传输"
              onClick={() => onAction(task, "cancel")}
            >
              {busy === "cancel" || task.status === "cancelling"
                ? "正在取消"
                : "取消"}
            </button>
          )}
          {actions.reconcile && (
            <button
              type="button"
              disabled={!!busy}
              onClick={() => onAction(task, "reconcile")}
            >
              核对状态
            </button>
          )}
          {actions.retry && (
            <button
              type="button"
              disabled={!!busy}
              aria-label={
                task.status === "interrupted" ? "重试未完成项" : "重试传输"
              }
              onClick={() => onRetry(task)}
            >
              重试
            </button>
          )}
          <button
            type="button"
            aria-expanded={expanded}
            aria-controls={`transfer-details-${task.id}`}
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? "收起详情" : "详情"}
          </button>
        </div>
      </div>
      {task.error && (
        <p className="transfer-error-summary" title={task.error}>
          {task.error}
        </p>
      )}
      {task.waitingReason && (
        <p className="transfer-waiting">{task.waitingReason}</p>
      )}
      {error && (
        <p className="transfer-action-error" role="alert">
          {error}
        </p>
      )}
      {expanded && (
        <div
          id={`transfer-details-${task.id}`}
          className="transfer-task-details"
        >
          {(task.sourcePaths?.length
            ? task.sourcePaths
            : [task.sourcePath]
          ).map((path, index) => (
            <p key={index}>来源：{path}</p>
          ))}
          <p>
            目标：{task.targetName} · {task.targetPath}
          </p>
          {task.directAddress && <p>本次直连 · {task.directAddress}</p>}
          <p>
            已完成 {task.filesDone} / {task.filesTotal || "—"} 个文件 ·{" "}
            {formatSize(task.bytesDone)}
            {task.bytesTotal > 0 && ` / ${formatSize(task.bytesTotal)}`}
            {task.skipped > 0 && ` · 跳过 ${task.skipped} 项`}
          </p>
          {task.status === "running" &&
            rate > 0 &&
            task.bytesTotal > task.bytesDone && (
              <p>
                预计剩余 {Math.ceil((task.bytesTotal - task.bytesDone) / rate)}{" "}
                秒
              </p>
            )}
          {task.currentFile && working && (
            <p>
              {task.currentFile} · 已接收 {formatSize(task.currentBytes ?? 0)}
              {task.kind === "direct" &&
                ` · 已提交 ${formatSize(task.committedBytes ?? 0)}`}
            </p>
          )}
          {task.error && <p className="transfer-action-error">{task.error}</p>}
          {task.lastAttempt && (
            <p>
              上次执行：{transferLabels[task.lastAttempt.status]}
              {task.lastAttempt.error && ` · ${task.lastAttempt.error}`}
            </p>
          )}
          {actions.cleanup && (
            <div className="transfer-cleanup">
              <p>临时内容待清理，其他文件传输不受影响。</p>
              <button
                type="button"
                disabled={!!busy}
                onClick={() => onAction(task, "cleanup")}
              >
                清理临时资源
              </button>
            </div>
          )}
          {!!task.temporaryPaths?.length && (
            <details>
              <summary>临时资源位置</summary>
              {task.temporaryPaths.map((path) => (
                <p key={path}>{path}</p>
              ))}
            </details>
          )}
          {!!task.items?.length && (
            <details>
              <summary>项目处理详情</summary>
              {task.items.map((item, i) => (
                <p key={i}>
                  {item.source} · {item.error ?? item.status}
                </p>
              ))}
            </details>
          )}
          {task.finishedAt && (
            <p>结束时间：{new Date(task.finishedAt).toLocaleString()}</p>
          )}
          {actions.dismiss && (
            <button disabled={!!busy} onClick={() => onAction(task, "dismiss")}>
              移除记录
            </button>
          )}
          {task.warnings.length > 0 && (
            <details>
              <summary>{task.warnings.length} 条处理说明</summary>
              {task.warnings.map((warning, i) => (
                <p key={i}>{warning}</p>
              ))}
            </details>
          )}
        </div>
      )}
    </article>
  );
}
