import { invokeDiagnosed as invoke } from "./diagnostics";
import type { DeleteTask } from "./fileApi";
import type { TransferTask } from "./transferApi";

export interface TaskActions {
  stop: boolean;
  retry: boolean;
  reconcile: boolean;
  cleanup: boolean;
  dismiss: boolean;
}
export type FileTask = (
  | { category: "transfer"; task: TransferTask }
  | { category: "delete"; task: DeleteTask }
) & { actions: TaskActions };
export type TaskFilter = "all" | "active" | "attention";
export type FileTaskAction = "cancel" | "reconcile" | "cleanup" | "dismiss";
export interface TaskSummary {
  total: number;
  active: number;
  attention: number;
  completed: number;
}
export interface TaskPage {
  tasks: FileTask[];
  summary: TaskSummary;
  nextCursor: string | null;
}
export interface TaskDetailItem {
  path: string;
  status: string;
  root: boolean;
  deletedCount: number;
  missingCount: number;
  error: string | null;
  uncertain: boolean;
  currentState: "missing" | "exists" | null;
}
export interface TaskDetailPage {
  task: FileTask;
  items: TaskDetailItem[];
  total: number;
  nextOffset: number | null;
}
export const fileTaskApi = {
  list: (
    filter: TaskFilter = "all",
    cursor: string | null = null,
    limit = 50,
  ) => invoke<TaskPage>("list_file_tasks", { filter, cursor, limit }),
  detail: (id: string, offset = 0, limit = 100) =>
    invoke<TaskDetailPage>("get_file_task", { id, offset, limit }),
  cancel: (id: string) => invoke<void>("cancel_file_task", { id }),
  reconcile: (id: string) => invoke<void>("reconcile_file_task", { id }),
  dismiss: (id: string) => invoke<void>("dismiss_file_task", { id }),
  clearCompleted: () => invoke<void>("clear_completed_file_tasks"),
};
export function notifyFileTaskStarted() {
  window.dispatchEvent(new Event("workspace-file-task-started"));
}
