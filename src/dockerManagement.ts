import { dockerApi, type DockerFrame } from "./dockerApi";
import { errorOf } from "./types";
export type DockerManagementApi = Pick<
  typeof dockerApi,
  "subscribe" | "unsubscribe" | "interest" | "ack" | "refresh"
>;
const empty = (): DockerFrame => ({
  subscriptionId: "",
  revision: 0,
  engine: null,
  operations: [],
  detail: null,
  error: null,
});
export class DockerManagementStore {
  private state = empty();
  private listeners = new Set<() => void>();
  private subscription: string | null = null;
  private epoch = 0;
  private target: string | null = null;
  private interestVersion = 0;
  private sendingInterest = false;
  constructor(
    readonly sessionId: string,
    private api: DockerManagementApi = dockerApi,
  ) {}
  getState = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(next: DockerFrame) {
    this.state = next;
    this.listeners.forEach((listener) => listener());
  }
  async start() {
    const epoch = ++this.epoch;
    this.update(empty());
    try {
      const id = await this.api.subscribe(this.sessionId, (frame) => {
        if (epoch !== this.epoch) return;
        // A Channel frame may arrive before the subscribe promise resolves.
        if (
          this.state.subscriptionId === frame.subscriptionId &&
          frame.revision <= this.state.revision
        ) {
          void this.api
            .ack(frame.subscriptionId, frame.revision)
            .catch(() => {});
          return;
        }
        this.update(frame);
        void this.api.ack(frame.subscriptionId, frame.revision).catch(() => {});
      });
      if (epoch !== this.epoch) {
        if (id) await this.api.unsubscribe(id);
        return;
      }
      this.subscription = id;
      await this.sendInterest();
    } catch (error) {
      if (epoch === this.epoch)
        this.update({ ...this.state, error: errorOf(error).message });
    }
  }
  stop() {
    ++this.epoch;
    if (this.subscription)
      void this.api.unsubscribe(this.subscription).catch(() => {});
    this.subscription = null;
  }
  setTarget(target: string | null) {
    if (target === this.target) return;
    this.target = target;
    ++this.interestVersion;
    this.update({ ...this.state, detail: null });
    void this.sendInterest();
  }
  private async sendInterest() {
    if (this.sendingInterest || !this.subscription) return;
    this.sendingInterest = true;
    const epoch = this.epoch;
    try {
      let version: number;
      do {
        version = this.interestVersion;
        await this.api.interest(this.subscription!, this.target);
      } while (
        version !== this.interestVersion &&
        this.subscription &&
        epoch === this.epoch
      );
    } catch (error) {
      if (epoch === this.epoch)
        this.update({ ...this.state, error: errorOf(error).message });
    } finally {
      this.sendingInterest = false;
      if (epoch !== this.epoch && this.subscription) void this.sendInterest();
    }
  }
  async refresh() {
    await this.api.refresh(this.sessionId);
  }
}
