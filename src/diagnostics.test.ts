import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
});
afterEach(() => vi.useRealTimers());

describe("local diagnostic boundary", () => {
  it("correlates a command and returned task without recording its arguments or result", async () => {
    const api = await import("./diagnostics");
    const task = crypto.randomUUID();
    invoke.mockImplementation(async (command: string) =>
      command === "start_download"
        ? { id: task, path: "PRIVATE_PATH" }
        : undefined,
    );
    await api.invokeDiagnosed("start_download", {
      password: "PRIVATE_PASSWORD",
    });
    await api.flushDiagnostics();
    const events = invoke.mock.calls
      .filter(([c]) => c === "append_frontend_diagnostics")
      .flatMap(([, args]) => args.events);
    expect(events).toHaveLength(2);
    expect(events[1]).toMatchObject({
      event: "invoke.end",
      traceId: events[0].traceId,
      outcome: "accepted",
      subject: task,
    });
    expect(JSON.stringify(events)).not.toContain("PRIVATE_");
    expect(invoke.mock.calls[0][1].diagnosticTraceId).toBe(events[0].traceId);
  });

  it("keeps error codes but excludes free-form rejection messages", async () => {
    const api = await import("./diagnostics");
    const error = {
      kind: "network",
      nativeCode: 10061,
      message: "SECRET_HOST password=SECRET",
    };
    invoke.mockImplementation(async (command: string) => {
      if (command === "open_connection") throw error;
    });
    await expect(api.invokeDiagnosed("open_connection", {})).rejects.toBe(
      error,
    );
    await api.flushDiagnostics();
    const events = invoke.mock.calls
      .filter(([c]) => c === "append_frontend_diagnostics")
      .flatMap(([, args]) => args.events);
    expect(events[1]).toMatchObject({
      errorCode: "network",
      nativeCode: 10061,
      outcome: "failed",
    });
    expect(JSON.stringify(events)).not.toContain("SECRET");
  });

  it("never logs terminal byte streams and plugin clipboard payloads", async () => {
    const api = await import("./diagnostics");
    await api.invokeDiagnosed("terminal_input", { data: "SECRET" });
    await api.invokeDiagnosed("plugin:clipboard-manager|write_text", {
      text: "SECRET",
    });
    await api.flushDiagnostics();
    expect(invoke.mock.calls.map(([c]) => c)).toEqual([
      "terminal_input",
      "plugin:clipboard-manager|write_text",
    ]);
  });

  it("reports bounded queue loss at flush even without another user operation", async () => {
    const api = await import("./diagnostics");
    for (let i = 0; i < 300; i++)
      api.recordDiagnostic({ event: "frontend.ready" });
    await api.flushDiagnostics();
    const batches = invoke.mock.calls.map(([, args]) => args.events);
    expect(batches.every((batch) => batch.length <= 32)).toBe(true);
    expect(
      batches.flat().find((e) => e.event === "frontend.dropped").dropped,
    ).toBe(44);
  });

  it("removes function names and messages from captured frontend stacks", async () => {
    const api = await import("./diagnostics");
    const error = new Error("SECRET_PASSWORD");
    error.stack =
      "Error: SECRET_PASSWORD\n at SECRET_FUNCTION (http://tauri.localhost/assets/index-A1b2.js:12:4)\n at /private/SECRET_PATH:1:2";
    api.recordFrontendError("frontend.error", error);
    await api.flushDiagnostics();
    const events = invoke.mock.calls.flatMap(([, args]) => args.events ?? []);
    expect(events[0].stack).toBe("/assets/index-A1b2.js:12:4");
    expect(JSON.stringify(events)).not.toContain("SECRET");
  });
});
