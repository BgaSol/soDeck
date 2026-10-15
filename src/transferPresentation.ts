import { transferActive, type TransferTask } from "./transferApi";

export const transferLabels: Record<TransferTask["status"], string> = {
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
  sourceRetained: "已复制，来源未删除",
  cancelled: "已取消",
  unknown: "状态待确认",
  interrupted: "已中断，请检查目标",
  needsAttention: "需要处理",
};

const kinds: Record<TransferTask["kind"], string> = {
  download: "下载",
  upload: "上传",
  direct: "直传",
  serverCopy: "复制",
  serverMove: "移动",
};

export function sortTransfers(tasks: TransferTask[]) {
  return [...tasks].sort(
    (a, b) =>
      (b.sortOrder ?? 0) - (a.sortOrder ?? 0) ||
      (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
  );
}

export function transferTitle(task: TransferTask) {
  const paths = task.sourcePaths?.length ? task.sourcePaths : [task.sourcePath];
  // Old multi-selection summaries are opaque; filenames can contain newlines.
  const windowsPath =
    task.kind === "upload" && /^(?:[a-z]:[\\/]|\\\\)/i.test(paths[0]);
  const path = paths[0].replace(windowsPath ? /[\\/]+$/ : /\/+$/, "");
  const name =
    path.split(windowsPath ? /[\\/]/ : /\//).pop() || task.sourceName;
  return `${kinds[task.kind]} ${name}${paths.length > 1 ? ` 等 ${paths.length} 项` : ""}`;
}

export function taskNeedsAttention(task: TransferTask) {
  return [
    "failed",
    "partialFailed",
    "sourceRetained",
    "unknown",
    "interrupted",
    "needsAttention",
  ].includes(task.status);
}

export function transferProgress(task: TransferTask): number | undefined {
  if (task.status !== "running" || task.bytesTotal <= 0) return undefined;
  return Math.max(0, Math.min(0.99, task.bytesDone / task.bytesTotal));
}

export function summarizeTransfers(tasks: TransferTask[]) {
  const active = tasks.filter(transferActive);
  const known =
    active.length > 0 &&
    active.every((task) => task.status === "running" && task.bytesTotal > 0);
  const total = active.reduce((sum, task) => sum + task.bytesTotal, 0);
  const done = active.reduce(
    (sum, task) => sum + Math.max(0, Math.min(task.bytesDone, task.bytesTotal)),
    0,
  );
  const progress = known ? Math.min(0.99, done / total) : undefined;
  const phase =
    active.length === 1
      ? transferLabels[active[0].status]
      : `${active.length} 项进行中`;
  return {
    active: active.length,
    attention: tasks.filter(taskNeedsAttention).length,
    progress,
    label: active.length ? phase : "传输任务",
  };
}

export function transferTime(timestamp?: number | null) {
  if (!timestamp) return "历史任务";
  const date = new Date(timestamp);
  const today = new Date();
  return date.toLocaleDateString() === today.toLocaleDateString()
    ? date.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      })
    : date.toLocaleDateString([], { month: "2-digit", day: "2-digit" });
}
