import { netbirdApi, type Frame, type Interest } from "./netbirdApi";
import { errorOf } from "../types";
export const emptyFrame = (): Frame => ({
  subscriptionId: "",
  revision: 0,
  connectionIdentity: "",
  refreshing: false,
  operations: [],
  snapshot: {
    binding: null,
    collectionStatus: "collecting",
    updatedAt: null,
    error: null,
    installation: "unknown",
    service: "unknown",
    authentication: "unknown",
    connection: "unknown",
    localPeer: null,
    management: { state: "unknown", address: null, message: null },
    signal: { state: "unknown", address: null, message: null },
    relay: { state: "unknown", address: null, message: null },
    dns: { state: "unknown", address: null, message: null },
    peers: [],
    peersComplete: false,
    reportedTotal: null,
  },
});
type Api = Pick<
  typeof netbirdApi,
  "subscribe" | "interest" | "unsubscribe" | "ack" | "refresh"
>;
export class NetBirdStore {
  private state = emptyFrame();
  private listeners = new Set<() => void>();
  private epoch = 0;
  private subscription: string | null = null;
  private level: Interest = "none";
  private interestVersion = 0;
  private sending = false;
  private refreshing: Promise<void> | null = null;
  constructor(
    readonly contextId: string,
    private api: Api = netbirdApi,
  ) {}
  getState = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(state: Frame) {
    this.state = state;
    this.listeners.forEach((listener) => listener());
  }
  private fail(error: unknown) {
    this.update({
      ...this.state,
      refreshing: false,
      snapshot: {
        ...this.state.snapshot,
        collectionStatus: this.state.snapshot.updatedAt
          ? "stale"
          : "unavailable",
        error: errorOf(error).message,
      },
    });
  }
  async start() {
    const epoch = ++this.epoch;
    try {
      const id = await this.api.subscribe(
        this.contextId,
        this.level,
        (frame) => {
          if (epoch !== this.epoch) return;
          if (this.subscription && frame.subscriptionId !== this.subscription)
            return;
          if (
            frame.subscriptionId !== this.state.subscriptionId ||
            frame.revision > this.state.revision
          )
            this.update(frame);
          void this.api
            .ack(frame.subscriptionId, frame.revision)
            .catch(() => {});
        },
      );
      if (epoch !== this.epoch) {
        await this.api.unsubscribe(id);
        return;
      }
      this.subscription = id;
      await this.sendInterest();
    } catch (error) {
      if (epoch === this.epoch) this.fail(error);
    }
  }
  stop() {
    ++this.epoch;
    if (this.subscription)
      void this.api.unsubscribe(this.subscription).catch(() => {});
    this.subscription = null;
    this.refreshing = null;
    this.update({
      ...this.state,
      refreshing: false,
      snapshot: { ...this.state.snapshot, collectionStatus: "disconnected" },
    });
  }
  setInterest(level: Interest) {
    if (this.level === level) return;
    this.level = level;
    ++this.interestVersion;
    void this.sendInterest();
  }
  private async sendInterest() {
    if (this.sending || !this.subscription) return;
    this.sending = true;
    const epoch = this.epoch;
    try {
      let version: number;
      do {
        version = this.interestVersion;
        await this.api.interest(this.subscription!, this.level);
      } while (
        version !== this.interestVersion &&
        epoch === this.epoch &&
        this.subscription
      );
    } catch (error) {
      if (epoch === this.epoch) this.fail(error);
    } finally {
      this.sending = false;
      if (epoch !== this.epoch && this.subscription) void this.sendInterest();
    }
  }
  refresh() {
    if (!this.subscription) return Promise.reject(new Error("请先连接服务器"));
    if (this.refreshing) return this.refreshing;
    const epoch = this.epoch;
    const promise = this.api
      .refresh(this.subscription)
      .catch((error) => {
        if (epoch === this.epoch) this.fail(error);
        throw error;
      })
      .finally(() => {
        if (epoch === this.epoch) this.refreshing = null;
      });
    this.refreshing = promise;
    return promise;
  }
}
