import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ArrowRightLeft,
  ChevronDown,
  CircleAlert,
} from "lucide-react";
import { transferActive, type TransferTask } from "./transferApi";
import {
  taskNeedsAttention,
  transferLabels,
  transferProgress,
  transferTime,
  transferTitle,
} from "./transferPresentation";
import { formatSize } from "./types";

export type TransferAction = "cancel" | "reconcile" | "cleanup";
interface Props {
  open: boolean;
  suspended: boolean;
  anchor: RefObject<HTMLButtonElement | null>;
  tasks: TransferTask[];
  rates: Record<string, number>;
  busy: Record<string, string>;
  errors: Record<string, string>;
  error: string;
  onClose: (restoreFocus?: boolean) => void;
  onAction: (task: TransferTask, action: TransferAction) => void;
  onRetry: (task: TransferTask) => void;
}

export function TransferPopover(props: Props) {
  const { open, suspended, anchor, tasks, onClose } = props;
  const panel = useRef<HTMLElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const [position, setPosition] = useState({
    right: 8,
    bottom: 40,
    maxHeight: 400,
  });
  const [newCount, setNewCount] = useState(0);
  const previous = useRef(new Map<string, string>());
  const scroll = useRef<{ top: boolean; id?: string; offset: number }>({
    top: true,
    offset: 0,
  });
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  function rememberScroll() {
    const element = list.current;
    if (!element) return;
    const top = element.scrollTop < 8;
    const boundary = element.getBoundingClientRect().top;
    const row = [
      ...element.querySelectorAll<HTMLElement>("[data-task-id]"),
    ].find((item) => item.getBoundingClientRect().bottom > boundary + 1);
    scroll.current = {
      top,
      id: row?.dataset.taskId,
      offset: (row?.getBoundingClientRect().top ?? boundary) - boundary,
    };
    if (top) setNewCount(0);
  }

  useLayoutEffect(() => {
    if (!open) return;
    const next = new Map(
      tasks.map((task) => [task.id, `${task.runId}:${task.sortOrder ?? 0}`]),
    );
    const changed = [...next].filter(
      ([id, run]) => previous.current.get(id) !== run,
    ).length;
    if (scroll.current.top) {
      if (list.current) list.current.scrollTop = 0;
    } else if (list.current) {
      const row = [
        ...list.current.querySelectorAll<HTMLElement>("[data-task-id]"),
      ].find((item) => item.dataset.taskId === scroll.current.id);
      if (row)
        list.current.scrollTop +=
          row.getBoundingClientRect().top -
          list.current.getBoundingClientRect().top -
          scroll.current.offset;
      if (changed && previous.current.size)
        setNewCount((count) => count + changed);
    }
    previous.current = next;
  }, [tasks, open]);

  useLayoutEffect(() => {
    if (!open) return;
    const update = () => {
      const rect = anchor.current?.getBoundingClientRect();
      if (!rect) return;
      const width = Math.min(420, window.innerWidth - 16);
      setPosition({
        right: Math.max(
          8,
          Math.min(
            window.innerWidth - rect.right,
            window.innerWidth - width - 8,
          ),
        ),
        bottom: Math.max(8, window.innerHeight - rect.top + 7),
        maxHeight: Math.max(80, Math.min(420, rect.top - 16)),
      });
    };
    update();
    const observer = new ResizeObserver(update);
    if (anchor.current) observer.observe(anchor.current);
    window.addEventListener("resize", update);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
    };
  }, [open, anchor]);

  useEffect(() => {
    if (open) closeButton.current?.focus({ preventScroll: true });
  }, [open]);

  useEffect(() => {
    if (!open || suspended) return;
    const modalOpen = () => document.querySelector("dialog[open]") !== null;
    const outside = (event: PointerEvent) => {
      if (modalOpen()) return;
      const path = event.composedPath();
      if (!path.includes(panel.current!) && !path.includes(anchor.current!))
        closeRef.current(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !modalOpen() && !event.defaultPrevented) {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current(true);
      }
    };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("keydown", escape, true);
    };
  }, [open, suspended, anchor]);

  return createPortal(
    <section
      ref={panel}
      id="transfer-task-popover"
      className="transfer-popover"
      aria-label="传输任务"
      hidden={!open}
      style={position}
    >
      <header className="transfer-popover-head">
        <strong>传输任务</strong>
        <span>共 {tasks.length} 项</span>
        <button
          ref={closeButton}
          type="button"
          className="transfer-close"
          aria-label="收起任务列表"
          onClick={() => onClose(true)}
        >
          <ChevronDown size={16} />
        </button>
      </header>
      {props.error && (
        <p className="transfer-fetch-error" role="alert">
          任务刷新失败：{props.error}
        </p>
      )}
      {newCount > 0 && (
        <button
          type="button"
          className="transfer-new"
          onClick={() => {
            if (list.current) list.current.scrollTop = 0;
            scroll.current = { top: true, offset: 0 };
            setNewCount(0);
          }}
        >
          有 {newCount} 项新任务 · 返回顶部
        </button>
      )}
      <div ref={list} className="transfer-task-list" onScroll={rememberScroll}>
        {tasks.map((task) => (
          <TransferTaskRow
            key={task.id}
            task={task}
            rate={props.rates[task.id] ?? 0}
            busy={props.busy[task.id]}
            error={props.errors[task.id]}
            onAction={props.onAction}
            onRetry={props.onRetry}
          />
        ))}
      </div>
    </section>,
    document.body,
  );
}

function TransferTaskRow({
  task,
  rate,
  busy,
  error,
  onAction,
  onRetry,
}: {
  task: TransferTask;
  rate: number;
  busy?: string;
  error?: string;
  onAction: Props["onAction"];
  onRetry: Props["onRetry"];
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
  const reconcile = ["unknown", "needsAttention", "interrupted"].includes(
    task.status,
  );
  const retry = [
    "failed",
    "partialFailed",
    "sourceRetained",
    "cancelled",
    "interrupted",
  ].includes(task.status);
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
          {working && (
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
          {!working && reconcile && (
            <button
              type="button"
              disabled={!!busy}
              onClick={() => onAction(task, "reconcile")}
            >
              核对状态
            </button>
          )}
          {!working && retry && (
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
          {!working && task.cleanupPending && (
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
