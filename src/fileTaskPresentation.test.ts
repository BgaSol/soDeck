import { expect, it } from "vitest";
import type { FileTask } from "./fileTaskApi";
import {
  fileTaskActive,
  fileTaskNeedsAttention,
  fileTaskProgress,
  fileTaskTitle,
} from "./fileTaskPresentation";

const actions = {
  stop: true,
  retry: false,
  reconcile: false,
  cleanup: false,
  dismiss: false,
};
const deletion: FileTask = {
  category: "delete",
  task: {
    id: "delete",
    status: "running",
    entryCount: 2,
    entries: [
      {
        path: "/data/日志",
        kind: "directory",
        status: "pending",
        deletedCount: 0,
        missingCount: 0,
        failures: [],
      },
    ],
    sessionId: "session",
    serverName: "server",
    revision: 1,
    createdAt: 1,
    lastStartedAt: 1,
    finishedAt: null,
    sortOrder: 1,
    parentTaskId: null,
    lastCheckedAt: null,
    currentPath: null,
    deletedCount: 0,
    missingCount: 0,
    affectedDirectories: [],
    error: null,
  },
  actions,
};
const transfer: FileTask = {
  category: "transfer",
  task: {
    id: "upload",
    status: "running",
    bytesDone: 50,
    bytesTotal: 100,
    runId: "run",
    kind: "upload",
    sourceName: "local",
    sourcePath: "/file",
    targetName: "server",
    targetPath: "/data",
    conflict: "rename",
    filesDone: 0,
    filesTotal: 1,
    skipped: 0,
    error: null,
    warnings: [],
  },
  actions,
};

it("does not mix deletion counts with byte progress or partially loaded active tasks", () => {
  expect(fileTaskProgress([transfer], 1)).toBe(0.5);
  expect(fileTaskProgress([transfer, deletion], 2)).toBeUndefined();
  expect(fileTaskProgress([transfer], 2)).toBeUndefined();
});
it("uses selected root count independently of recursively deleted children", () => {
  expect(fileTaskTitle(deletion)).toBe("删除 日志 等 2 项");
});
it("keeps uncertain and interrupted deletion visible without treating stopped tasks as running", () => {
  expect(fileTaskActive(deletion)).toBe(true);
  for (const status of ["unknown", "interrupted"] as const) {
    const task = {
      ...deletion,
      task: { ...deletion.task, status },
    } as FileTask;
    expect(fileTaskActive(task)).toBe(false);
    expect(fileTaskNeedsAttention(task)).toBe(true);
  }
  const stopped = {
    ...deletion,
    task: { ...deletion.task, status: "cancelled" },
  } as FileTask;
  expect(fileTaskActive(stopped)).toBe(false);
  expect(fileTaskNeedsAttention(stopped)).toBe(false);
});
