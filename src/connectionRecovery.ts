import type { AppError, TerminalEvent } from "./types";

export type RecoveryAction = "retry" | "userAction" | "none";
export type RecoveryPhase =
  "idle" | "waiting" | "connecting" | "awaitingUser" | "stopped";
export interface RecoverySnapshot {
  phase: RecoveryPhase;
  attempts: number;
  nextRetryAt: number | null;
  hasConnected: boolean;
  connectedAt: number | null;
  automatic: boolean;
}
export const initialRecovery = (connected = false): RecoverySnapshot => ({
  phase: "idle",
  attempts: 0,
  nextRetryAt: null,
  hasConnected: connected,
  connectedAt: connected ? Date.now() : null,
  automatic: false,
});
export function recoveryAction(error: AppError | null): RecoveryAction {
  if (!error) return "none";
  if (["network", "timeout", "io", "remoteInterrupted"].includes(error.kind))
    return "retry";
  if (["authentication", "hostKey"].includes(error.kind)) return "userAction";
  return "none";
}
export function retryDelay(attempts: number, random = Math.random): number {
  const base = [2000, 5000, 10000, 20000, 30000][
    Math.min(Math.max(0, attempts), 4)
  ];
  return Math.round(base * (0.9 + random() * 0.2));
}

// One scheduler per workspace. Pausing transfers ownership without changing the
// absolute retry deadline; the receiver resumes only after workspace_ready.
export class ConnectionRecovery {
  state: RecoverySnapshot;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private paused = true;
  constructor(
    initial: RecoverySnapshot,
    private readonly change: (state: RecoverySnapshot) => void,
    private readonly connect: () => void,
    private readonly random = Math.random,
  ) {
    this.state = { ...initial };
  }
  private update(patch: Partial<RecoverySnapshot>) {
    this.state = { ...this.state, ...patch };
    this.change(this.state);
  }
  private clear() {
    clearTimeout(this.timer);
    this.timer = undefined;
  }
  private arm() {
    this.clear();
    if (
      this.paused ||
      this.state.phase !== "waiting" ||
      this.state.nextRetryAt === null
    )
      return;
    this.timer = setTimeout(
      () => this.start(true),
      Math.max(0, this.state.nextRetryAt - Date.now()),
    );
  }
  pause() {
    this.paused = true;
    this.clear();
    return { ...this.state };
  }
  resume() {
    this.paused = false;
    this.arm();
  }
  start(automatic = false) {
    if (
      this.paused ||
      this.state.phase === "connecting" ||
      this.state.phase === "awaitingUser"
    )
      return false;
    this.clear();
    this.update({
      phase: "connecting",
      automatic,
      nextRetryAt: null,
      attempts: automatic ? this.state.attempts + 1 : 0,
      connectedAt: null,
    });
    this.connect();
    return true;
  }
  connecting() {
    this.clear();
    this.update({ phase: "connecting", nextRetryAt: null });
  }
  prompt() {
    this.clear();
    this.update({ phase: "awaitingUser", nextRetryAt: null });
  }
  connected() {
    this.clear();
    this.update({
      phase: "idle",
      hasConnected: true,
      connectedAt: Date.now(),
      nextRetryAt: null,
      automatic: false,
    });
  }
  closed(event: Extract<TerminalEvent, { type: "closed" }>) {
    if (this.state.phase === "stopped") return;
    this.clear();
    const action =
      event.reason === "failed"
        ? (event.recovery ?? recoveryAction(event.error))
        : "none";
    const attempts =
      this.state.connectedAt !== null &&
      Date.now() - this.state.connectedAt >= 30000
        ? 0
        : this.state.attempts;
    if (action === "retry" && this.state.hasConnected) {
      this.update({
        phase: "waiting",
        automatic: true,
        attempts,
        connectedAt: null,
        nextRetryAt: Date.now() + retryDelay(attempts, this.random),
      });
      this.arm();
    } else {
      this.update({
        phase: "idle",
        automatic: false,
        nextRetryAt: null,
        connectedAt: null,
      });
    }
  }
  stop() {
    this.clear();
    this.update({
      phase: "stopped",
      automatic: false,
      nextRetryAt: null,
      connectedAt: null,
    });
  }
}
