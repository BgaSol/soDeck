import { useEffect, useState } from "react";
import { CircleAlert, Trash2 } from "lucide-react";
import {
  fileTaskApi,
  type FileTask,
  type FileTaskAction,
  type TaskDetailPage,
} from "./fileTaskApi";
import {
  deleteLabels,
  deleteOutcomes,
  fileTaskActive,
  fileTaskNeedsAttention,
  fileTaskTitle,
} from "./fileTaskPresentation";
import { transferTime } from "./transferPresentation";
import { errorOf } from "./types";

type DeleteItem = Extract<FileTask, { category: "delete" }>;
export function DeleteTaskRow({
  item,
  busy,
  error,
  onAction,
  onRetry,
}: {
  item: DeleteItem;
  busy?: string;
  error?: string;
  onAction: (action: FileTaskAction) => void;
  onRetry: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const { task, actions } = item;
  const working = fileTaskActive(item);
  return (
    <article
      className="transfer-task-row delete-task-row"
      data-status={task.status}
      data-task-id={task.id}
    >
      <div className="transfer-task-heading">
        <Trash2 size={15} aria-hidden="true" />
        <strong title={fileTaskTitle(item)}>{fileTaskTitle(item)}</strong>
        <time
          dateTime={
            task.createdAt ? new Date(task.createdAt).toISOString() : undefined
          }
          title={
            task.createdAt
              ? new Date(task.createdAt).toLocaleString()
              : undefined
          }
        >
          {transferTime(task.createdAt)}
        </time>
      </div>
      <p
        className="transfer-task-route"
        title={`${task.serverName} · ${task.entries[0]?.path ?? ""}`}
      >
        {task.serverName} <span>· {task.entries[0]?.path}</span>
      </p>
      {working && <progress aria-label="删除进度" />}
      <div className="transfer-task-meta">
        <span
          className={`transfer-state${fileTaskNeedsAttention(item) ? " attention" : ""}`}
        >
          {fileTaskNeedsAttention(item) && (
            <CircleAlert size={12} aria-hidden="true" />
          )}
          {deleteLabels[task.status]}
        </span>
        <div className="transfer-row-actions">
          {(working || busy === "cancel") && (
            <button
              disabled={!!busy || !actions.stop}
              onClick={() => onAction("cancel")}
            >
              {task.status === "cancelling" || busy === "cancel"
                ? "正在停止…"
                : "停止删除"}
            </button>
          )}
          {actions.reconcile && (
            <button disabled={!!busy} onClick={() => onAction("reconcile")}>
              {busy === "reconcile" ? "正在核对…" : "核对结果"}
            </button>
          )}
          {actions.retry && (
            <button disabled={!!busy} onClick={onRetry}>
              {busy === "retry" ? "正在检查…" : "重新发起删除"}
            </button>
          )}
          <button
            aria-expanded={expanded}
            aria-controls={`delete-details-${task.id}`}
            onClick={() => setExpanded((v) => !v)}
          >
            {expanded ? "收起详情" : "详情"}
          </button>
        </div>
      </div>
      <p className="delete-task-count">
        已删除 {task.deletedCount} 个文件、目录或链接
        {task.missingCount > 0 && ` · ${task.missingCount} 个已不存在`}
      </p>
      {working && task.currentPath && (
        <p className="delete-current-path" title={task.currentPath}>
          当前：{task.currentPath}
        </p>
      )}
      {task.error && <p className="transfer-error-summary">{task.error}</p>}
      {error && (
        <p className="transfer-action-error" role="alert">
          {error}
        </p>
      )}
      {expanded && (
        <div id={`delete-details-${task.id}`} className="transfer-task-details">
          <p>选中 {task.entryCount} 项，删除数量包含目录内部子项。</p>
          <p>开始时间：{new Date(task.lastStartedAt).toLocaleString()}</p>
          {task.finishedAt && (
            <p>结束时间：{new Date(task.finishedAt).toLocaleString()}</p>
          )}
          {task.lastCheckedAt && (
            <p>最近核对：{new Date(task.lastCheckedAt).toLocaleString()}</p>
          )}
          {task.parentTaskId && <p>由历史任务重新确认发起，原结果已保留。</p>}
          <p className="muted">
            停止后已删除的内容不会恢复。核对结果只检查当前状态，不会再次删除。
          </p>
          <DeleteDetails item={item} />
          {actions.dismiss && (
            <button disabled={!!busy} onClick={() => onAction("dismiss")}>
              移除记录
            </button>
          )}
        </div>
      )}
    </article>
  );
}
function DeleteDetails({ item }: { item: DeleteItem }) {
  const [page, setPage] = useState<TaskDetailPage | null>(null);
  const [limit, setLimit] = useState(100);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const { id, status, lastCheckedAt } = item.task;
  useEffect(() => {
    let live = true;
    let flight = false;
    const refresh = async () => {
      if (flight) return;
      flight = true;
      try {
        let offset = 0;
        let result: TaskDetailPage | null = null;
        do {
          const next = await fileTaskApi.detail(
            id,
            offset,
            Math.min(1000, limit - offset),
          );
          result = result
            ? { ...next, items: [...result.items, ...next.items] }
            : next;
          offset = result.items.length;
        } while (live && result.nextOffset !== null && offset < limit);
        if (live) {
          setPage(result);
          setError("");
        }
      } catch (e) {
        if (live) setError(errorOf(e).message);
      } finally {
        flight = false;
      }
    };
    void refresh();
    const timer = setInterval(() => {
      if (fileTaskActive(item)) void refresh();
    }, 900);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [id, status, lastCheckedAt, limit, retry]);
  return (
    <>
      {error && (
        <p role="alert">
          读取详情失败：{error}{" "}
          <button onClick={() => setRetry((v) => v + 1)}>重新读取</button>
        </p>
      )}
      {!page && !error && <p>正在读取详情…</p>}
      <ul className="delete-task-results">
        {page?.items.map((entry, index) => (
          <li key={`${entry.path}:${index}`}>
            <span className="delete-entry-path">{entry.path}</span>：
            {entry.error ?? deleteOutcomes[entry.status] ?? entry.status}
            {entry.root && entry.deletedCount > 0 && (
              <span> · 已删除 {entry.deletedCount} 个子项或项目</span>
            )}
            {entry.uncertain && (
              <span>
                {entry.currentState === "missing"
                  ? "；核对时路径已不存在，不代表本次删除已获确认"
                  : entry.currentState === "exists"
                    ? "；当前路径存在，可能是同名新项目，未继续删除"
                    : "；未确认是否已删除"}
              </span>
            )}
          </li>
        ))}
      </ul>
      {page?.nextOffset != null && (
        <button onClick={() => setLimit((v) => v + 100)}>
          加载更多结果（已显示 {page.items.length} / {page.total}）
        </button>
      )}
    </>
  );
}
