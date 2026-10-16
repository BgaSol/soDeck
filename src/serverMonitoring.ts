import type { Metric } from "./resourceMonitoring";
import type { Container, DockerImage, DockerStatus } from "./types";

export interface DockerInterest {
  summary: boolean;
  containers: boolean;
  images: boolean;
}
export interface DockerSummary {
  total: number;
  byState: Record<string, number>;
  unhealthy: number | null;
  healthError: string | null;
}
export interface DockerState {
  engineGeneration: number;
  engine: Metric<DockerStatus>;
  containers: Metric<Container[]>;
  summary: Metric<DockerSummary>;
  images: Metric<DockerImage[]>;
  refreshing: boolean;
}
export function emptyDocker(): DockerState {
  const metric = <T>(): Metric<T> => ({
    value: null,
    status: "collecting",
    updatedAt: null,
    error: null,
  });
  return {
    engineGeneration: 0,
    engine: metric(),
    containers: metric(),
    summary: metric(),
    images: metric(),
    refreshing: false,
  };
}
export interface TrafficValues {
  sentBytes: string;
  receivedBytes: string;
  sentPerSecond: number | null;
  receivedPerSecond: number | null;
}
export interface ClientTrafficSample {
  epoch: string;
  revision: number;
  startedAt: number;
  sampledAt: number;
  activeConnections: number;
  measurement: "sshStream";
  status: "collecting" | "ready" | "idle" | "unavailable";
  total: TrafficValues;
  monitoring: TrafficValues;
  terminalAndLogs: TrafficValues;
  files: TrafficValues;
  other: TrafficValues;
}
export interface TrafficPoint {
  sequence: number;
  at: number;
  sent: number | null;
  received: number | null;
  monitorSent: number | null;
  monitorReceived: number | null;
  gap: boolean;
}
export interface TrafficHistory {
  epoch: string;
  points: TrafficPoint[];
  truncated: boolean;
}
export function mergeTrafficHistory(
  current: TrafficPoint[],
  incoming: TrafficPoint[],
) {
  const values = new Map(current.map((p) => [p.sequence, p]));
  incoming.forEach((p) => values.set(p.sequence, p));
  const points = [...values.values()].sort((a, b) => a.sequence - b.sequence);
  const cutoff = (points.at(-1)?.at ?? 0) - 3_600_000;
  return points.filter((p) => p.at >= cutoff).slice(-1801);
}
