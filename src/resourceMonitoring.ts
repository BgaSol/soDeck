import type { MountState, MountTable, CachedMountUsage } from "./remoteMounts";
import { errorOf } from "./types";
import {
  emptyDocker,
  mergeTrafficHistory,
  type DockerInterest,
  type DockerState,
  type ClientTrafficSample,
  type TrafficPoint,
  type TrafficHistory,
} from "./serverMonitoring";

export interface Metric<T> {
  value: T | null;
  status:
    | "collecting"
    | "ready"
    | "stale"
    | "unsupported"
    | "unavailable"
    | "disconnected";
  updatedAt: number | null;
  error: string | null;
}
export interface CpuSample {
  percent: number | null;
  cores: Record<string, number | null>;
  load: [number, number, number] | null;
}
export interface MemorySample {
  totalBytes: string;
  availableBytes: string;
  usedBytes: string;
  usedPercent: number;
  cachedBytes: string | null;
  swapTotalBytes: string | null;
  swapUsedBytes: string | null;
}
export interface NetworkSample {
  name: string;
  index: number | null;
  isDefault: boolean;
  virtualInterface: boolean;
  rxBytes: string;
  txBytes: string;
  rxPerSecond: number | null;
  txPerSecond: number | null;
  rxErrors: string;
  txErrors: string;
  rxDropped: string;
  txDropped: string;
}
export interface Metrics {
  cpu: Metric<CpuSample>;
  memory: Metric<MemorySample>;
  network: Metric<NetworkSample[]>;
}
export interface HistoryPoint {
  sequence: number;
  at: number;
  cpu: number | null;
  memory: number | null;
  network: Record<string, [number | null, number | null]>;
  gap: boolean;
}
export interface MountUpdate {
  table: MountTable | null;
  usage: Record<
    string,
    Omit<CachedMountUsage, "value" | "error"> & {
      value: CachedMountUsage["value"] | null;
      error: string | null;
    }
  >;
  fetchedAt: number;
  loading: boolean;
  error: string;
}
export type MonitorEvent =
  | {
      type: "snapshot";
      generation: string;
      metricsRevision: number;
      mountsRevision: number;
      metrics: Metrics;
      mounts: MountUpdate;
      dockerRevision?: number;
      docker?: DockerState;
      traffic?: ClientTrafficSample;
    }
  | {
      type: "metrics";
      generation: string;
      revision: number;
      metrics: Metrics;
      point: HistoryPoint;
    }
  | {
      type: "mounts";
      generation: string;
      revision: number;
      mounts: MountUpdate;
    }
  | {
      type: "docker";
      generation: string;
      revision: number;
      docker: DockerState;
    }
  | {
      type: "traffic";
      generation: string;
      traffic: ClientTrafficSample;
      point: TrafficPoint | null;
    };
export interface Interest {
  visible: boolean;
  mountPaths: string[];
  docker: DockerInterest;
  clientTraffic: boolean;
}
export interface MonitorApi {
  subscribe: (
    sessionId: string,
    interest: Interest,
    receive: (event: MonitorEvent) => void,
  ) => Promise<string>;
  unsubscribe: (subscriptionId: string) => Promise<void>;
  update: (subscriptionId: string, interest: Interest) => Promise<void>;
  history: (
    subscriptionId: string,
    minutes: number,
  ) => Promise<{
    generation: string;
    points: HistoryPoint[];
    truncated: boolean;
    traffic?: TrafficHistory;
  }>;
  refresh: (
    subscriptionId: string,
    scope: "all" | "metrics" | "mounts" | "docker",
  ) => Promise<void>;
}
export interface MonitorState {
  docker: DockerState;
  traffic: ClientTrafficSample | null;
  trafficPoints: TrafficPoint[];
  trafficHistoryTruncated: boolean;
  metrics: Metrics;
  mounts: MountState;
  points: HistoryPoint[];
  connected: boolean;
  historyLoading: boolean;
  historyError: string;
  historyTruncated: boolean;
  error: string;
}
const emptyMetric = <T>(): Metric<T> => ({
  value: null,
  status: "collecting",
  updatedAt: null,
  error: null,
});
export const emptyMetrics = (): Metrics => ({
  cpu: emptyMetric(),
  memory: emptyMetric(),
  network: emptyMetric(),
});

export function mergeHistory(
  current: HistoryPoint[],
  incoming: HistoryPoint[],
): HistoryPoint[] {
  const values = new Map(current.map((p) => [p.sequence, p]));
  incoming.forEach((p) => values.set(p.sequence, p));
  const result = [...values.values()].sort((a, b) => a.sequence - b.sequence);
  const cutoff = (result.at(-1)?.at ?? 0) - 3_600_000;
  let bytes = 0;
  let start = result.length;
  while (start > 0 && result.length - start < 1801) {
    const point = result[start - 1];
    bytes +=
      128 +
      Object.keys(point.network).reduce(
        (n, key) => n + key.length * 2 + 128,
        0,
      );
    if (point.at < cutoff || bytes > 8 * 1024 * 1024) break;
    start--;
  }
  return result.slice(start);
}
export function selectInterface(
  interfaces: NetworkSample[],
  selected: string,
): NetworkSample | undefined {
  if (selected) return interfaces.find((n) => n.name === selected);
  return (
    interfaces.find((n) => n.isDefault) ??
    interfaces.find((n) => n.name !== "lo" && !n.virtualInterface) ??
    interfaces.find((n) => n.name !== "lo") ??
    interfaces[0]
  );
}
export function percent(value: number | null | undefined) {
  return value == null ? "—" : `${value.toFixed(1)}%`;
}
export function rate(value: number | null | undefined) {
  if (value == null || !Number.isFinite(value) || value < 0) return "—";
  const units = ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s"];
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  return `${value.toFixed(i ? 1 : 0)} ${units[i]}`;
}

export class MonitorStore {
  private state: MonitorState;
  private listeners = new Set<() => void>();
  private mountListeners = new Set<() => void>();
  private subscription: string | null = null;
  private epoch = 0;
  private generation = "";
  private metricsRevision = -1;
  private mountsRevision = -1;
  private dockerRevision = -1;
  private trafficRevision = -1;
  private trafficEpoch = "";
  private trafficPointsEpoch = "";
  private dockerConsumers = new Map<string, DockerInterest>();
  private consumers = new Map<string, string[]>();
  private visible = false;
  private running = false;
  private minutes = 5;
  private interestVersion = 0;
  private updatingEpoch: number | null = null;
  constructor(
    readonly sessionId: string,
    private api: MonitorApi,
  ) {
    this.state = {
      docker: emptyDocker(),
      traffic: null,
      trafficPoints: [],
      trafficHistoryTruncated: false,
      metrics: emptyMetrics(),
      mounts: { sessionId, usage: {}, fetchedAt: 0, loading: false, error: "" },
      points: [],
      connected: false,
      historyLoading: false,
      historyError: "",
      historyTruncated: false,
      error: "",
    };
  }
  getState = () => this.state;
  getMounts = () => this.state.mounts;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  subscribeMounts = (listener: () => void) => {
    this.mountListeners.add(listener);
    return () => {
      this.mountListeners.delete(listener);
    };
  };
  private update(patch: Partial<MonitorState>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((f) => f());
    if (patch.mounts) this.mountListeners.forEach((f) => f());
  }
  private interest(): Interest {
    return {
      visible: this.visible,
      clientTraffic: true,
      docker: {
        summary: [...this.dockerConsumers.values()].some((i) => i.summary),
        containers: [...this.dockerConsumers.values()].some(
          (i) => i.containers,
        ),
        images: [...this.dockerConsumers.values()].some((i) => i.images),
      },
      mountPaths: [...new Set([...this.consumers.values()].flat())].slice(
        0,
        4096,
      ),
    };
  }
  setVisible(visible: boolean) {
    if (this.visible !== visible) {
      this.visible = visible;
      this.queueInterest();
    }
  }
  setDockerInterest(consumer: string, interest: DockerInterest) {
    const old = this.dockerConsumers.get(consumer);
    if (
      old?.summary === interest.summary &&
      old?.containers === interest.containers &&
      old?.images === interest.images
    )
      return;
    this.dockerConsumers.set(consumer, interest);
    this.queueInterest();
  }
  removeDockerInterest(consumer: string) {
    if (this.dockerConsumers.delete(consumer)) this.queueInterest();
  }
  setPaths(consumer: string, paths: string[]) {
    const previous = this.consumers.get(consumer);
    if (
      previous?.length === paths.length &&
      previous.every((p, i) => p === paths[i])
    )
      return;
    this.consumers.set(consumer, paths);
    this.queueInterest();
  }
  removePaths(consumer: string) {
    if (this.consumers.delete(consumer)) this.queueInterest();
  }
  private queueInterest() {
    this.interestVersion++;
    void this.sendInterest();
  }
  private async sendInterest() {
    if (this.updatingEpoch === this.epoch || !this.subscription) return;
    const epoch = this.epoch;
    this.updatingEpoch = epoch;
    try {
      let sent = -1;
      while (
        this.subscription &&
        epoch === this.epoch &&
        sent !== this.interestVersion
      ) {
        sent = this.interestVersion;
        await this.api.update(this.subscription, this.interest());
      }
    } catch (e) {
      if (epoch === this.epoch) this.update({ error: errorOf(e).message });
    } finally {
      if (this.updatingEpoch === epoch) this.updatingEpoch = null;
    }
  }
  async start() {
    if (!this.sessionId || this.running) return;
    this.running = true;
    const epoch = ++this.epoch;
    this.generation = "";
    this.metricsRevision = -1;
    this.mountsRevision = -1;
    this.dockerRevision = -1;
    this.trafficRevision = -1;
    this.trafficEpoch = "";
    this.update({ connected: true, error: "", points: [] });
    try {
      const id = await this.api.subscribe(
        this.sessionId,
        this.interest(),
        (event) => {
          if (epoch === this.epoch) this.receive(event);
        },
      );
      if (typeof id !== "string" || !id)
        throw new Error("无法建立资源监控订阅");
      if (epoch !== this.epoch) {
        await this.api.unsubscribe(id);
        return;
      }
      this.subscription = id;
      await this.sendInterest();
      if (epoch === this.epoch) void this.loadHistory(this.minutes);
    } catch (e) {
      if (epoch === this.epoch) {
        this.running = false;
        this.update({ error: errorOf(e).message });
      }
    }
  }
  stop() {
    ++this.epoch;
    this.running = false;
    const subscription = this.subscription;
    this.subscription = null;
    if (subscription) void this.api.unsubscribe(subscription).catch(() => {});
    this.update({
      connected: false,
      historyLoading: false,
      mounts: {
        ...this.state.mounts,
        loading: false,
        usage: Object.fromEntries(
          Object.entries(this.state.mounts.usage).map(([k, v]) => [
            k,
            { ...v, loading: false, stale: true },
          ]),
        ),
      },
    });
  }
  receive(event: MonitorEvent) {
    if (!this.generation) this.generation = event.generation;
    if (event.generation !== this.generation) return;
    const patch: Partial<MonitorState> = {};
    if (
      (event.type === "snapshot" && event.docker) ||
      event.type === "docker"
    ) {
      const revision =
        event.type === "snapshot"
          ? (event.dockerRevision ?? 0)
          : event.revision;
      if (revision >= this.dockerRevision) {
        this.dockerRevision = revision;
        patch.docker = event.docker;
      }
    }
    if (
      (event.type === "snapshot" && event.traffic) ||
      event.type === "traffic"
    ) {
      const traffic = event.traffic!;
      if (!this.trafficEpoch) {
        this.trafficEpoch = traffic.epoch;
      }
      if (
        traffic.epoch === this.trafficEpoch &&
        traffic.revision >= this.trafficRevision
      ) {
        if (this.trafficPointsEpoch !== traffic.epoch) {
          this.trafficPointsEpoch = traffic.epoch;
          patch.trafficPoints = [];
        }
        this.trafficRevision = traffic.revision;
        patch.traffic = traffic;
        if (event.type === "traffic" && event.point)
          patch.trafficPoints = mergeTrafficHistory(
            patch.trafficPoints ?? this.state.trafficPoints,
            [event.point],
          );
      }
    }
    if (event.type === "snapshot" || event.type === "metrics") {
      const revision =
        event.type === "snapshot" ? event.metricsRevision : event.revision;
      if (revision >= this.metricsRevision) {
        this.metricsRevision = revision;
        patch.metrics = event.metrics;
        patch.error = "";
      }
      if (event.type === "metrics")
        patch.points = mergeHistory(this.state.points, [event.point]);
    }
    if (event.type === "snapshot" || event.type === "mounts") {
      const revision =
        event.type === "snapshot" ? event.mountsRevision : event.revision;
      if (revision >= this.mountsRevision) {
        this.mountsRevision = revision;
        patch.mounts = {
          ...event.mounts,
          sessionId: this.sessionId,
          table: event.mounts.table ?? undefined,
          usage: Object.fromEntries(
            Object.entries(event.mounts.usage).map(([k, v]) => [
              k,
              {
                ...v,
                value: v.value ?? undefined,
                error: v.error ?? undefined,
              },
            ]),
          ),
        };
      }
    }
    this.update(patch);
  }
  async loadHistory(minutes: number) {
    this.minutes = minutes;
    if (!this.subscription) return;
    const epoch = this.epoch;
    this.update({ historyLoading: true, historyError: "" });
    try {
      const result = await this.api.history(this.subscription, minutes);
      if (epoch === this.epoch && !this.generation)
        this.generation = result.generation;
      if (
        epoch !== this.epoch ||
        minutes !== this.minutes ||
        result.generation !== this.generation
      )
        return;
      this.update({
        points: mergeHistory(this.state.points, result.points),
        historyTruncated: result.truncated,
      });
      if (result.traffic) {
        if (!this.trafficEpoch) this.trafficEpoch = result.traffic.epoch;
        if (result.traffic.epoch === this.trafficEpoch) {
          const current =
            this.trafficPointsEpoch === result.traffic.epoch
              ? this.state.trafficPoints
              : [];
          this.trafficPointsEpoch = result.traffic.epoch;
          this.update({
            trafficPoints: mergeTrafficHistory(current, result.traffic.points),
            trafficHistoryTruncated: result.traffic.truncated,
          });
        }
      }
    } catch (e) {
      if (epoch === this.epoch && minutes === this.minutes)
        this.update({ historyError: errorOf(e).message });
    } finally {
      if (epoch === this.epoch && minutes === this.minutes)
        this.update({ historyLoading: false });
    }
  }
  refresh = async (scope: "all" | "metrics" | "mounts" | "docker" = "all") => {
    if (!this.subscription) {
      await this.start();
      return;
    }
    const epoch = this.epoch;
    try {
      await this.api.refresh(this.subscription, scope);
    } catch (e) {
      if (epoch !== this.epoch) return;
      const failure = errorOf(e);
      if (failure.kind === "disconnected" && this.state.connected) {
        this.stop();
        await this.start();
      } else {
        this.update({ error: failure.message });
      }
    }
  };
}
