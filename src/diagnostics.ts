import {
  invoke as rawInvoke,
  isTauri,
  type InvokeArgs,
  type InvokeOptions,
} from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type DiagnosticEvent = {
  event: string;
  traceId?: string;
  command?: string;
  outcome?: string;
  durationMs?: number;
  frontendSeq?: number;
  frontendMs?: number;
  errorCode?: string;
  nativeCode?: number;
  subject?: string;
  stack?: string;
  dropped?: number;
};
let queue: DiagnosticEvent[] = [];
let sequence = 0,
  dropped = 0;
let scheduled: ReturnType<typeof setTimeout> | undefined;
let sending: Promise<void> | undefined;
const recent = new Map<string, number>();
const token = (value: unknown): value is string =>
  typeof value === "string" && /^[A-Za-z0-9_.-]{1,96}$/.test(value);

export function recordDiagnostic(event: DiagnosticEvent) {
  if (!isTauri()) return;
  if (queue.length >= 256) {
    dropped++;
    return;
  }
  queue.push({
    ...event,
    frontendSeq: ++sequence,
    frontendMs: performance.now(),
  });
  if (!scheduled)
    scheduled = setTimeout(() => {
      scheduled = undefined;
      void flushDiagnostics();
    }, 100);
}

export async function flushDiagnostics(): Promise<void> {
  if (sending) {
    await sending;
    if (queue.length) await flushDiagnostics();
    return;
  }
  clearTimeout(scheduled);
  scheduled = undefined;
  const work = async () => {
    if (dropped) {
      const count = dropped;
      dropped = 0;
      queue.unshift({
        event: "frontend.dropped",
        outcome: "omitted",
        dropped: count,
        frontendSeq: ++sequence,
        frontendMs: performance.now(),
      });
    }
    // A bounded drain: events arriving after this cutoff belong to a later flush.
    let remaining = queue.length;
    while (remaining > 0) {
      const batch = queue.splice(0, Math.min(32, remaining));
      remaining -= batch.length;
      try {
        await rawInvoke("append_frontend_diagnostics", { events: batch });
      } catch {
        dropped += batch.length;
      }
    }
  };
  sending = work();
  try {
    await sending;
  } finally {
    sending = undefined;
  }
}

export function recordFrontendError(
  kind: "frontend.error" | "frontend.rejection" | "frontend.render.error",
  error: unknown,
  componentStack?: string,
) {
  const stack = error instanceof Error ? error.stack : undefined;
  // Never serialize Error.message or arbitrary rejected objects. The backend filters frames again.
  const frames =
    `${stack ?? ""}\n${componentStack ?? ""}`
      .match(/\/assets\/[\w.-]+\.js:\d+:\d+/g)
      ?.slice(0, 24)
      .join("\n") ?? "";
  recordDiagnostic({
    event: kind,
    outcome: "failed",
    errorCode:
      error instanceof TypeError
        ? "TypeError"
        : error instanceof RangeError
          ? "RangeError"
          : "UnhandledError",
    stack: frames,
  });
  void flushDiagnostics();
}

let installed = false;
export function installDiagnostics() {
  if (installed || !isTauri()) return;
  installed = true;
  window.addEventListener("error", (event) =>
    recordFrontendError("frontend.error", event.error),
  );
  window.addEventListener("unhandledrejection", (event) =>
    recordFrontendError("frontend.rejection", event.reason),
  );
  void listen<string>("diagnostics-flush-request", async ({ payload }) => {
    await flushDiagnostics();
    await rawInvoke("ack_diagnostics_flush", { token: payload }).catch(
      () => {},
    );
  })
    .then(() => rawInvoke("register_diagnostics_window"))
    .catch(() => {});
  recordDiagnostic({ event: "frontend.ready", outcome: "ok" });
  window.addEventListener("pagehide", () => {
    void flushDiagnostics();
  });
}

// Hot byte streams and acknowledgement commands are deliberately not logged.
function excluded(command: string) {
  return (
    /^(terminal_input|terminal_resize|terminal_ack|ack_.*|.*_ack|append_frontend_diagnostics|(?:get|open|export|cancel|reveal|clear|register)_diagnostics(?:_.*)?)$/.test(
      command,
    ) || command.startsWith("plugin:")
  );
}
function polling(command: string) {
  return /^(list_|get_|.*_snapshot$|.*_page$|.*_summary$|update_.*_interest$|.*_info$|.*_status$)/.test(
    command,
  );
}
export async function invokeDiagnosed<T>(
  command: string,
  args?: InvokeArgs,
  options?: InvokeOptions,
): Promise<T> {
  if (!isTauri() || excluded(command))
    return rawInvoke<T>(command, args, options);
  const traceId = crypto.randomUUID(),
    start = performance.now();
  const last = recent.get(command) ?? -Infinity;
  const sampled = !polling(command) || start - last >= 30000;
  if (sampled) {
    recent.set(command, start);
    recordDiagnostic({ event: "invoke.start", command, traceId });
  }
  const params =
    args && !(args instanceof Uint8Array) && !(args instanceof ArrayBuffer)
      ? { ...args, diagnosticTraceId: traceId }
      : args;
  try {
    const result = await rawInvoke<T>(command, params, options);
    const resultId =
      typeof result === "string"
        ? result
        : result && typeof result === "object" && "id" in result
          ? (result as { id: unknown }).id
          : undefined;
    if (sampled)
      recordDiagnostic({
        event: "invoke.end",
        command,
        traceId,
        outcome: /^(start_|open_connection|connect_container|submit_)/.test(
          command,
        )
          ? "accepted"
          : "ok",
        durationMs: performance.now() - start,
        subject:
          typeof resultId === "string" && /^[0-9a-f-]{36}$/i.test(resultId)
            ? resultId
            : undefined,
      });
    return result;
  } catch (error) {
    const safe =
      error && typeof error === "object"
        ? (error as { kind?: unknown; nativeCode?: unknown; traceId?: unknown })
        : {};
    const errorKey = `${command}:${token(safe.kind) ? safe.kind : "unknown"}:failed`;
    if (
      !polling(command) ||
      start - (recent.get(errorKey) ?? -Infinity) >= 30000
    ) {
      recent.set(errorKey, start);
      recordDiagnostic({
        event: "invoke.end",
        command,
        traceId,
        outcome: safe.kind === "cancelled" ? "cancelled" : "failed",
        durationMs: performance.now() - start,
        errorCode: token(safe.kind) ? safe.kind : "unknown",
        nativeCode:
          typeof safe.nativeCode === "number" ? safe.nativeCode : undefined,
      });
    }
    if (
      error &&
      typeof error === "object" &&
      Object.isExtensible(error) &&
      !safe.traceId
    )
      Object.assign(error, { traceId });
    throw error;
  }
}
