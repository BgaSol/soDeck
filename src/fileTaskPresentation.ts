import type { DeleteTask } from "./fileApi";
import type { FileTask } from "./fileTaskApi";
import { summarizeTransfers, transferTitle } from "./transferPresentation";

export const deleteLabels: Record<DeleteTask["status"], string> = {
  checking: "正在检查删除条件",
  scanning: "正在扫描目录",
  running: "正在删除",
  cancelling: "正在停止删除",
  completed: "删除完成",
  partialFailed: "部分项目未删除",
  failed: "删除未完成",
  cancelled: "已停止删除",
  interrupted: "删除已中断",
  unknown: "删除结果待核对",
};
export const deleteOutcomes: Record<string, string> = {
  pending: "未执行",
  deleted: "已删除",
  missing: "已不存在",
  partial: "部分删除",
  failed: "未完成",
  notExecuted: "未执行",
};
export function fileTaskActive(task: FileTask) {
  return [
    "queued",
    "checking",
    "scanning",
    "running",
    "verifying",
    "committing",
    "cancelling",
  ].includes(task.task.status);
}
export function fileTaskNeedsAttention(task: FileTask) {
  return [
    "failed",
    "partialFailed",
    "sourceRetained",
    "unknown",
    "interrupted",
    "needsAttention",
  ].includes(task.task.status);
}
export function fileTaskTitle(task: FileTask) {
  if (task.category === "transfer") return transferTitle(task.task);
  const first =
    task.task.entries[0]?.path.replace(/\/+$/, "").split("/").pop() ?? "项目";
  return `删除 ${first}${task.task.entryCount > 1 ? ` 等 ${task.task.entryCount} 项` : ""}`;
}
export function fileTaskProgress(tasks: FileTask[], activeCount: number) {
  const active = tasks.filter(fileTaskActive);
  if (
    active.length !== activeCount ||
    active.some((t) => t.category === "delete")
  )
    return undefined;
  return summarizeTransfers(
    active.flatMap((t) => (t.category === "transfer" ? [t.task] : [])),
  ).progress;
}
