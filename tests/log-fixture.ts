import type { Page } from "@playwright/test";
export async function logFixture(
  page: Page,
  options: { offline?: boolean; count?: number } = {},
) {
  await page.addInitScript(({ offline, count }) => {
    const w = window as any;
    let serial = 0;
    let output: any;
    let eventRevision = 0;
    const make = (seq: number, text = "request completed") => ({
      seq,
      stream: seq % 3 === 0 ? "stderr" : "stdout",
      raw:
        "2026-10-20T06:32:18.000000000Z " +
        (seq % 7 === 0 ? "ERROR timeout " : "INFO ") +
        text +
        " " +
        seq,
      text: (seq % 7 === 0 ? "ERROR timeout " : "INFO ") + text + " " + seq,
      timestamp: "2026-10-20T06:32:18.000000000Z",
      level: seq % 7 === 0 ? "error" : "info",
      complete: true,
      continuation: false,
      prefix: "",
    });
    const data = {
      rows: Array.from({ length: count ?? 1200 }, (_, i) => make(i + 1)),
      snapshots: new Map<string, any[]>(),
      calls: [] as any[],
      source: "a".repeat(64),
      phase: "streaming",
      failSnapshot: false,
      exports: [] as any[],
    };
    const event = (phase: string, extra: any = {}) => {
      data.phase = phase;
      output?.onmessage({
        type: "state",
        collectorId: "collector",
        eventRevision: ++eventRevision,
        phase,
        reason: null,
        retryAt: null,
        error: null,
        tty: false,
        container: {
          state: phase === "waitingContainer" ? "stopped" : "running",
          startedAt: "2026-10-20T06:00:00Z",
          finishedAt: null,
          restartCount: 0,
          observedAt: Date.now(),
        },
        ...extra,
      });
    };
    w.__LOG_VIEW__ = {
      data,
      event,
      append: (text = "new output") => {
        const row = make(data.rows.length + 1, text);
        data.rows.push(row);
        output?.onmessage({
          type: "saved",
          lastSeq: row.seq,
          changedFrom: row.seq,
        });
      },
      update: (seq: number, text: string) => {
        data.rows[seq - 1] = make(seq, text);
        output?.onmessage({
          type: "saved",
          lastSeq: data.rows.length,
          changedFrom: seq,
        });
      },
    };
    const info = () => ({
      sourceId: data.source,
      serverId: "test",
      serverName: "生产服务器",
      containerName: "api-service",
      containerId: "b".repeat(64),
      endpoint: "unix:///var/run/docker.sock",
      host: "10.20.0.12",
      port: 22,
      tty: false,
      canCollect: !offline,
      directory: "local/logs",
      stats: {
        records: data.rows.length,
        lastSeq: data.rows.length,
        bytes: data.rows.length * 100,
        lastTimestamp: data.rows.at(-1)?.timestamp ?? null,
        runs: 1,
        interruptedRuns: 0,
      },
    });
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "logs-test" },
        currentWebview: { label: "logs-test" },
      },
      transformCallback: () => ++serial,
      unregisterCallback: () => {},
      invoke: async (command: string, args: any = {}) => {
        data.calls.push({ command, args });
        if (command === "window_chrome") return "custom";
        if (command === "plugin:event|listen") return ++serial;
        if (command === "logs_info") return info();
        if (command === "logs_start") {
          output = args.output;
          event("streaming");
          return "collector";
        }
        if (command === "logs_retry") {
          event("streaming");
          return true;
        }
        if (command === "logs_events_page")
          return {
            events: [],
            boots: [
              {
                id: "boot",
                startedAt: "2026-10-20T06:00:00Z",
                observedAt: Date.now(),
                finishedAt: null,
              },
            ],
            nextCursor: 0,
            hasMore: false,
          };
        if (command === "logs_snapshot_create") {
          if (data.failSnapshot) throw { kind: "io", message: "快照空间不足" };
          const id = "snapshot-" + ++serial;
          data.snapshots.set(id, structuredClone(data.rows));
          return { id, lastSeq: data.rows.length, eventCursor: 0 };
        }
        if (command === "logs_snapshot_release") {
          data.snapshots.delete(args.snapshotId);
          return;
        }
        if (command === "logs_page") {
          const rows = args.snapshotId
            ? (data.snapshots.get(args.snapshotId) ?? [])
            : data.rows;
          const q = args.query;
          const high = Math.min(q.snapshot ?? rows.length, rows.length);
          const forward = q.after !== undefined;
          const start = forward ? q.after : (q.before ?? high + 1);
          const lower = forward ? start + 1 : Math.max(1, start - 20000);
          const upper = forward
            ? Math.min(high, start + 20000)
            : Math.min(high, start - 1);
          let found = rows.filter(
            (r: any) =>
              r.seq >= lower &&
              r.seq <= upper &&
              (!q.keyword ||
                (q.caseSensitive ? r.text : r.text.toLowerCase()).includes(
                  q.caseSensitive ? q.keyword : q.keyword.toLowerCase(),
                )) &&
              (!q.stream || q.stream === r.stream) &&
              (!q.from || r.timestamp >= q.from) &&
              (!q.to || r.timestamp <= q.to),
          );
          if (!forward) found.reverse();
          const more = found.length > q.limit;
          found = found.slice(0, q.limit);
          const cursor = more
            ? (found.at(-1)?.seq ?? start)
            : forward
              ? upper
              : lower;
          if (!forward) found.reverse();
          return {
            rows: structuredClone(found),
            nextCursor: cursor,
            hasMore: forward ? cursor < high : cursor > 1,
            scanned: Math.abs(cursor - start),
            snapshot: high,
          };
        }
        if (command === "logs_export_start") {
          data.exports.push(args.scope);
          args.output.onmessage({ records: 20, bytes: 2048 });
          return "C:/exports/container.log";
        }
        if (command === "logs_copy_range")
          return data.rows
            .filter((r: any) => r.seq >= args.from && r.seq <= args.to)
            .map((r: any) => r.raw)
            .join("\n");
        if (command === "logs_replacements") return [];
        if (command === "get_system_settings")
          return {
            developerMode: false,
            activeDeveloperMode: false,
            revision: 0,
            loadError: null,
            platform: "windows",
          };
        return null;
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  }, options);
  await page.goto("/?logs=1");
}
