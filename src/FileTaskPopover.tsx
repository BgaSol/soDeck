import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { ChevronDown } from "lucide-react";
import type {
  FileTask,
  FileTaskAction,
  TaskFilter,
  TaskSummary,
} from "./fileTaskApi";
import { TransferTaskRow } from "./TransferTaskRow";
import { DeleteTaskRow } from "./DeleteTaskRow";

interface Props {
  open: boolean;
  suspended: boolean;
  anchor: RefObject<HTMLButtonElement | null>;
  tasks: FileTask[];
  summary: TaskSummary;
  filter: TaskFilter;
  onFilter: (value: TaskFilter) => void;
  hasMore: boolean;
  loading: boolean;
  onLoadMore: () => void;
  onClearCompleted: () => void;
  clearing: boolean;
  rates: Record<string, number>;
  busy: Record<string, string>;
  errors: Record<string, string>;
  error: string;
  onClose: (restoreFocus?: boolean) => void;
  onAction: (task: FileTask, action: FileTaskAction) => void;
  onRetry: (task: FileTask) => void;
}

export function FileTaskPopover(props: Props) {
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
    scroll.current = { top: true, offset: 0 };
    previous.current = new Map();
    setNewCount(0);
    if (list.current) list.current.scrollTop = 0;
  }, [props.filter]);

  useLayoutEffect(() => {
    if (!open) return;
    const next = new Map(
      tasks.map(({ task }) => [task.id, `${task.sortOrder ?? 0}`]),
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
      id="file-task-popover"
      className="transfer-popover"
      aria-label="文件任务"
      hidden={!open}
      style={position}
    >
      <header className="transfer-popover-head">
        <strong>文件任务</strong>
        <span>共 {props.summary.total} 项</span>
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
      <nav className="file-task-filters" aria-label="筛选文件任务">
        {(
          [
            ["all", "全部"],
            ["active", "进行中"],
            ["attention", "需处理"],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            aria-pressed={props.filter === value}
            onClick={() => props.onFilter(value)}
          >
            {label}
          </button>
        ))}
        <button
          className="file-task-clear"
          disabled={!props.summary.completed || props.clearing}
          onClick={props.onClearCompleted}
        >
          清除已完成记录
        </button>
      </nav>
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
        {!tasks.length && (
          <p className="file-task-empty">
            {props.loading
              ? "正在读取任务…"
              : props.filter === "all"
                ? "暂无文件任务"
                : "没有符合条件的任务"}
          </p>
        )}
        {tasks.map((item) =>
          item.category === "transfer" ? (
            <TransferTaskRow
              key={item.task.id}
              task={item.task}
              actions={item.actions}
              rate={props.rates[item.task.id] ?? 0}
              busy={props.busy[item.task.id]}
              error={props.errors[item.task.id]}
              onAction={(_, action) => props.onAction(item, action)}
              onRetry={() => props.onRetry(item)}
            />
          ) : (
            <DeleteTaskRow
              key={item.task.id}
              item={item}
              busy={props.busy[item.task.id]}
              error={props.errors[item.task.id]}
              onAction={(action) => props.onAction(item, action)}
              onRetry={() => props.onRetry(item)}
            />
          ),
        )}
        {props.hasMore && (
          <button
            className="transfer-new"
            disabled={props.loading}
            onClick={props.onLoadMore}
          >
            加载更多任务
          </button>
        )}
      </div>
    </section>,
    document.body,
  );
}
