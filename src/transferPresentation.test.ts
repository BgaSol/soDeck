import { describe, expect, it } from "vitest";
import type { TransferTask } from "./transferApi";
import {
  sortTransfers,
  summarizeTransfers,
  taskNeedsAttention,
  transferProgress,
  transferTitle,
} from "./transferPresentation";

function task(patch: Partial<TransferTask> = {}): TransferTask {
  return {
    id: "one",
    runId: "run",
    kind: "upload",
    sourceName: "本机",
    sourcePath: "/data/file.txt",
    targetName: "server",
    targetPath: "/data",
    conflict: "rename",
    status: "running",
    bytesDone: 50,
    bytesTotal: 100,
    filesDone: 0,
    filesTotal: 1,
    skipped: 0,
    error: null,
    warnings: [],
    ...patch,
  };
}

describe("transfer presentation", () => {
  it("orders mixed kinds by attempt order, regardless of completion or timestamp", () => {
    const input = [
      task({ id: "legacy" }),
      task({ id: "up", sortOrder: 2, status: "completed", lastStartedAt: 400 }),
      task({ id: "down", kind: "download", sortOrder: 3, lastStartedAt: 100 }),
    ];
    expect(sortTransfers(input).map((t) => t.id)).toEqual([
      "down",
      "up",
      "legacy",
    ]);
    expect(input[0].id).toBe("legacy");
    input[0].sortOrder = 4;
    expect(sortTransfers(input)[0].id).toBe("legacy");
  });
  it("weights active progress by bytes and excludes history", () => {
    const summary = summarizeTransfers([
      task(),
      task({ id: "big", bytesDone: 90, bytesTotal: 900 }),
      task({
        id: "done",
        status: "completed",
        bytesDone: 9999,
        bytesTotal: 9999,
      }),
    ]);
    expect(summary.progress).toBeCloseTo(0.14);
    expect(summary.active).toBe(2);
  });
  it("does not claim a known overall percentage during scanning or finalization", () => {
    for (const status of [
      "scanning",
      "checking",
      "queued",
      "verifying",
      "committing",
      "cancelling",
    ] as const) {
      expect(
        summarizeTransfers([task(), task({ id: "phase", status })]).progress,
      ).toBeUndefined();
      expect(transferProgress(task({ status }))).toBeUndefined();
    }
    expect(
      summarizeTransfers([task({ bytesTotal: 0 })]).progress,
    ).toBeUndefined();
    expect(transferProgress(task({ bytesDone: 100 }))).toBe(0.99);
    expect(summarizeTransfers([]).progress).toBeUndefined();
  });
  it("keeps interruption and partial outcomes visible without flagging cancellation", () => {
    for (const status of [
      "failed",
      "partialFailed",
      "sourceRetained",
      "interrupted",
      "unknown",
      "needsAttention",
    ] as const)
      expect(taskNeedsAttention(task({ status }))).toBe(true);
    expect(taskNeedsAttention(task({ status: "cancelled" }))).toBe(false);
  });
  it("uses structured selections including filenames containing newlines", () => {
    expect(
      transferTitle(
        task({
          sourcePaths: [
            "C:\\release\\first\nsecond.zip",
            "C:\\release\\other.zip",
          ],
        }),
      ),
    ).toBe("上传 first\nsecond.zip 等 2 项");
    expect(
      transferTitle(
        task({ kind: "download", sourcePaths: ["/data/first\\second.zip"] }),
      ),
    ).toBe("下载 first\\second.zip");
  });
});
