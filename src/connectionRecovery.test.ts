import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ConnectionRecovery,
  initialRecovery,
  retryDelay,
} from "./connectionRecovery";
import type { TerminalEvent } from "./types";
const closed = (
  kind = "network",
  reason: "failed" | "ended" | "cancelled" = "failed",
): Extract<TerminalEvent, { type: "closed" }> => ({
  type: "closed",
  reason,
  message: kind,
  error: { kind, message: kind },
});
function fixture(connected = true) {
  const connect = vi.fn();
  const change = vi.fn();
  const recovery = new ConnectionRecovery(
    initialRecovery(connected),
    change,
    connect,
    () => 0.5,
  );
  recovery.resume();
  return { recovery, connect, change };
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(100000);
});
afterEach(() => vi.useRealTimers());
describe("connection recovery", () => {
  it("backs off sequential attempts and caps the delay", () => {
    const { recovery, connect } = fixture();
    for (const seconds of [2, 5, 10, 20, 30, 30]) {
      recovery.closed(closed());
      vi.advanceTimersByTime(seconds * 1000 - 1);
      const count = connect.mock.calls.length;
      expect(recovery.state.phase).toBe("waiting");
      vi.advanceTimersByTime(1);
      expect(connect).toHaveBeenCalledTimes(count + 1);
      expect(recovery.start()).toBe(false);
      vi.advanceTimersByTime(60000);
      expect(connect).toHaveBeenCalledTimes(count + 1);
    }
  });
  it("does not retry initial failure, normal exit, cancellation or configuration errors", () => {
    const first = fixture(false);
    first.recovery.closed(closed());
    vi.advanceTimersByTime(60000);
    expect(first.connect).not.toHaveBeenCalled();
    for (const event of [
      closed("network", "ended"),
      closed("network", "cancelled"),
      closed("authentication"),
      closed("hostKey"),
      closed("validation"),
    ]) {
      const { recovery, connect } = fixture();
      recovery.closed(event);
      vi.advanceTimersByTime(60000);
      expect(connect).not.toHaveBeenCalled();
    }
  });
  it("stopping cancels a deadline and ignores subsequent closure", () => {
    const { recovery, connect } = fixture();
    recovery.closed(closed());
    recovery.stop();
    recovery.closed(closed());
    vi.advanceTimersByTime(60000);
    expect(connect).not.toHaveBeenCalled();
    expect(recovery.state.phase).toBe("stopped");
    expect(recovery.start()).toBe(true);
    expect(connect).toHaveBeenCalledTimes(1);
  });
  it("immediate retry replaces the timer and authentication suspends retries", () => {
    const { recovery, connect } = fixture();
    recovery.closed(closed());
    recovery.start(true);
    recovery.prompt();
    vi.advanceTimersByTime(60000);
    expect(connect).toHaveBeenCalledTimes(1);
    expect(recovery.start()).toBe(false);
    recovery.connected();
    expect(recovery.state.phase).toBe("idle");
  });
  it("preserves backoff across flapping and resets only after a stable connection", () => {
    const { recovery } = fixture();
    recovery.closed(closed());
    vi.advanceTimersByTime(2000);
    recovery.connected();
    vi.advanceTimersByTime(1000);
    recovery.closed(closed());
    expect(recovery.state.nextRetryAt! - Date.now()).toBe(5000);
    vi.advanceTimersByTime(5000);
    recovery.connected();
    vi.advanceTimersByTime(30000);
    recovery.closed(closed());
    expect(recovery.state.nextRetryAt! - Date.now()).toBe(2000);
  });
  it("transfers a deadline only after commit and supports rollback", () => {
    const source = fixture();
    source.recovery.closed(closed());
    const snapshot = source.recovery.pause();
    const connect = vi.fn();
    const destination = new ConnectionRecovery(
      snapshot,
      vi.fn(),
      connect,
      () => 0.5,
    );
    vi.advanceTimersByTime(5000);
    expect(connect).not.toHaveBeenCalled();
    expect(source.connect).not.toHaveBeenCalled();
    destination.resume();
    vi.advanceTimersByTime(0);
    expect(connect).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(60000);
    expect(source.connect).not.toHaveBeenCalled();
    const rollback = fixture();
    rollback.recovery.closed(closed());
    rollback.recovery.pause();
    vi.advanceTimersByTime(5000);
    rollback.recovery.resume();
    vi.advanceTimersByTime(0);
    expect(rollback.connect).toHaveBeenCalledTimes(1);
  });
  it("respects backend recovery classification and bounded jitter", () => {
    const { recovery, connect } = fixture();
    recovery.closed({ ...closed(), recovery: "none" });
    vi.advanceTimersByTime(60000);
    expect(connect).not.toHaveBeenCalled();
    expect(retryDelay(0, () => 0)).toBe(1800);
    expect(retryDelay(12, () => 1)).toBe(33000);
  });
});
