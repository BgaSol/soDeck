import { describe, expect, it, vi } from "vitest";
import {
  MonitorStore,
  emptyMetrics,
  mergeHistory,
  rate,
  selectInterface,
  type HistoryPoint,
  type MonitorApi,
  type MonitorEvent,
  type MountUpdate,
  type NetworkSample,
} from "./resourceMonitoring";
import { chartPath } from "./MetricChart";
import { emptyDocker, type ClientTrafficSample } from "./serverMonitoring";

const traffic = (revision: number, epoch = "traffic"): ClientTrafficSample => {
  const value = {
    sentBytes: "100",
    receivedBytes: "200",
    sentPerSecond: 10,
    receivedPerSecond: 20,
  };
  return {
    epoch,
    revision,
    startedAt: 100,
    sampledAt: 2000 * revision,
    activeConnections: 1,
    measurement: "sshStream",
    status: "ready",
    total: value,
    monitoring: value,
    terminalAndLogs: value,
    files: value,
    other: value,
  };
};

const mounts: MountUpdate = {
  table: null,
  usage: {},
  fetchedAt: 0,
  loading: false,
  error: "",
};
const point = (sequence: number): HistoryPoint => ({
  sequence,
  at: sequence * 2000,
  cpu: 20,
  memory: 30,
  network: {},
  gap: false,
});
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function fixture() {
  let receive!: (event: MonitorEvent) => void;
  const api: MonitorApi = {
    subscribe: vi.fn(async (_session, _interest, callback) => {
      receive = callback;
      callback({
        type: "snapshot",
        generation: "g",
        metricsRevision: 0,
        mountsRevision: 0,
        metrics: emptyMetrics(),
        mounts,
      });
      return "sub";
    }),
    unsubscribe: vi.fn(async () => {}),
    update: vi.fn(async () => {}),
    refresh: vi.fn(async () => {}),
    history: vi.fn(async () => ({
      generation: "g",
      points: [],
      truncated: false,
    })),
  };
  return {
    api,
    store: new MonitorStore("session", api),
    send: (event: MonitorEvent) => receive(event),
  };
}
describe("resource subscriptions", () => {
  it("shares Docker demand and keeps Docker and traffic updates independent of failed metrics", async () => {
    const { store, api, send } = fixture();
    store.setDockerInterest("overview", {
      summary: true,
      containers: false,
      images: false,
    });
    store.setDockerInterest("details", {
      summary: true,
      containers: true,
      images: true,
    });
    await store.start();
    expect(vi.mocked(api.update).mock.calls.at(-1)?.[1].docker).toEqual({
      summary: true,
      containers: true,
      images: true,
    });
    const docker = emptyDocker();
    docker.engine.error = "docker permission";
    send({ type: "docker", generation: "g", revision: 5, docker });
    send({
      type: "docker",
      generation: "g",
      revision: 4,
      docker: emptyDocker(),
    });
    send({
      type: "traffic",
      generation: "g",
      traffic: traffic(2),
      point: {
        sequence: 2,
        at: 4000,
        sent: 10,
        received: 20,
        monitorSent: 1,
        monitorReceived: 2,
        gap: false,
      },
    });
    send({
      type: "metrics",
      generation: "g",
      revision: 100,
      metrics: emptyMetrics(),
      point: { ...point(3), gap: true },
    });
    send({
      type: "traffic",
      generation: "g",
      traffic: traffic(1),
      point: null,
    });
    send({
      type: "traffic",
      generation: "g",
      traffic: traffic(200, "wrong-account"),
      point: null,
    });
    expect(store.getState().docker.engine.error).toBe("docker permission");
    expect(store.getState().traffic?.revision).toBe(2);
    expect(store.getState().trafficPoints).toHaveLength(1);
    store.stop();
    expect(store.getState().traffic?.total.sentBytes).toBe("100");
  });
  it("merges traffic history with newer live points and rejects another account's history", async () => {
    const { store, api, send } = fixture();
    const request = deferred<Awaited<ReturnType<MonitorApi["history"]>>>();
    api.history = vi.fn(() => request.promise);
    await store.start();
    const tp = {
      sequence: 2,
      at: 4000,
      sent: 10,
      received: 20,
      monitorSent: 1,
      monitorReceived: 2,
      gap: false,
    };
    send({ type: "traffic", generation: "g", traffic: traffic(2), point: tp });
    request.resolve({
      generation: "g",
      points: [],
      truncated: false,
      traffic: {
        epoch: "traffic",
        points: [{ ...tp, sequence: 1, at: 2000 }],
        truncated: false,
      },
    });
    await request.promise;
    await Promise.resolve();
    expect(store.getState().trafficPoints.map((p) => p.sequence)).toEqual([
      1, 2,
    ]);
    api.history = vi.fn(async () => ({
      generation: "g",
      points: [],
      truncated: false,
      traffic: { epoch: "another", points: [], truncated: true },
    }));
    await store.loadHistory(5);
    expect(store.getState().trafficPoints).toHaveLength(2);
    expect(store.getState().trafficHistoryTruncated).toBe(false);
    store.stop();
  });
  it("replaces a previous account's history when new history arrives before the restart snapshot", async () => {
    const { api, store, send } = fixture();
    const tp = {
      sequence: 9,
      at: 18000,
      sent: 10,
      received: 20,
      monitorSent: 1,
      monitorReceived: 2,
      gap: false,
    };
    await store.start();
    send({
      type: "traffic",
      generation: "g",
      traffic: traffic(9, "old-account"),
      point: tp,
    });
    store.stop();
    let receive!: (event: MonitorEvent) => void;
    api.subscribe = vi.fn(async (_session, _interest, callback) => {
      receive = callback;
      return "new-sub";
    });
    api.history = vi.fn(async () => ({
      generation: "new-generation",
      points: [],
      truncated: false,
      traffic: {
        epoch: "new-account",
        points: [{ ...tp, sequence: 1, at: 2000 }],
        truncated: false,
      },
    }));
    await store.start();
    await Promise.resolve();
    expect(store.getState().trafficPoints.map((p) => p.sequence)).toEqual([1]);
    receive({
      type: "snapshot",
      generation: "new-generation",
      metricsRevision: 0,
      mountsRevision: 0,
      metrics: emptyMetrics(),
      mounts,
      traffic: traffic(1, "new-account"),
    });
    receive({
      type: "traffic",
      generation: "new-generation",
      traffic: traffic(2, "new-account"),
      point: { ...tp, sequence: 2, at: 4000 },
    });
    expect(store.getState().traffic?.epoch).toBe("new-account");
    expect(store.getState().trafficPoints.map((p) => p.sequence)).toEqual([
      1, 2,
    ]);
    store.stop();
  });
  it("accepts history arriving before the initial channel snapshot", async () => {
    const { api, store } = fixture();
    let receive!: (event: MonitorEvent) => void;
    api.subscribe = vi.fn(async (_session, _interest, callback) => {
      receive = callback;
      return "sub";
    });
    api.history = vi.fn(async () => ({
      generation: "g",
      points: [point(1)],
      truncated: false,
    }));
    await store.start();
    await Promise.resolve();
    expect(store.getState().points).toHaveLength(1);
    receive({
      type: "snapshot",
      generation: "g",
      metricsRevision: 1,
      mountsRevision: 0,
      metrics: emptyMetrics(),
      mounts,
    });
    expect(store.getState().points).toHaveLength(1);
    store.stop();
  });
  it("releases subscriptions that finish opening after a window unmounts", async () => {
    const { api, store } = fixture();
    const request = deferred<string>();
    api.subscribe = vi.fn(() => request.promise);
    const opening = store.start();
    store.stop();
    request.resolve("late-sub");
    await opening;
    expect(api.unsubscribe).toHaveBeenCalledWith("late-sub");
    expect(store.getState().connected).toBe(false);
  });
  it("merges racing history and live samples without losing or duplicating points", async () => {
    const { api, store, send } = fixture();
    const request = deferred<{
      generation: string;
      points: HistoryPoint[];
      truncated: boolean;
    }>();
    api.history = vi.fn(() => request.promise);
    await store.start();
    send({
      type: "metrics",
      generation: "g",
      revision: 4,
      metrics: emptyMetrics(),
      point: point(4),
    });
    request.resolve({
      generation: "g",
      points: [point(2), point(3), point(4)],
      truncated: false,
    });
    await request.promise;
    await Promise.resolve();
    expect(store.getState().points.map((p) => p.sequence)).toEqual([2, 3, 4]);
    store.stop();
    send({
      type: "metrics",
      generation: "g",
      revision: 5,
      metrics: emptyMetrics(),
      point: point(5),
    });
    expect(store.getState().points.map((p) => p.sequence)).toEqual([2, 3, 4]);
  });
  it("keeps each data family's revision independent and rejects old generations", async () => {
    const { store, send } = fixture();
    await store.start();
    const metrics = emptyMetrics();
    metrics.cpu.error = "new";
    send({
      type: "metrics",
      generation: "g",
      revision: 6,
      metrics,
      point: point(6),
    });
    send({
      type: "snapshot",
      generation: "g",
      metricsRevision: 0,
      mountsRevision: 5,
      metrics: emptyMetrics(),
      mounts: { ...mounts, error: "disk error" },
    });
    send({ type: "mounts", generation: "g", revision: 3, mounts });
    send({ type: "mounts", generation: "old", revision: 100, mounts });
    expect(store.getState().metrics.cpu.error).toBe("new");
    expect(store.getMounts().error).toBe("disk error");
    store.stop();
  });
  it("combines consumers and does not resubmit identical paths on a disk update", async () => {
    const { store, api } = fixture();
    store.setPaths("tree", ["/", "/data"]);
    store.setPaths("panel", ["/data", "/backup"]);
    store.setVisible(true);
    await store.start();
    expect(api.update).toHaveBeenLastCalledWith("sub", {
      visible: true,
      clientTraffic: true,
      docker: { summary: false, containers: false, images: false },
      mountPaths: ["/", "/data", "/backup"],
    });
    const calls = vi.mocked(api.update).mock.calls.length;
    store.setPaths("panel", ["/data", "/backup"]);
    expect(vi.mocked(api.update).mock.calls).toHaveLength(calls);
    store.removePaths("panel");
    await Promise.resolve();
    expect(api.update).toHaveBeenLastCalledWith("sub", {
      visible: true,
      clientTraffic: true,
      docker: { summary: false, containers: false, images: false },
      mountPaths: ["/", "/data"],
    });
    store.stop();
  });
  it("keeps real time gaps and a bounded hour", () => {
    const points = mergeHistory(
      [],
      Array.from({ length: 2000 }, (_, i) => point(i)),
    );
    expect(points).toHaveLength(1801);
    const samples = [
      point(0),
      point(1),
      { ...point(2), cpu: null },
      point(3),
      { ...point(4), gap: true },
    ];
    expect(
      chartPath(samples, (p) => p.cpu, 0, 10000, 100).match(/M/g),
    ).toHaveLength(3);
    expect(
      chartPath([{ ...point(1), cpu: 0 }], (p) => p.cpu, 0, 10000, 100),
    ).toContain("80.00");
  });
  it("selects one interface and never silently substitutes a missing explicit selection", () => {
    const makeNetwork = (name: string, isDefault: boolean): NetworkSample => ({
      name,
      isDefault,
      index: 1,
      virtualInterface: !isDefault,
      rxBytes: "0",
      txBytes: "0",
      rxPerSecond: 0,
      txPerSecond: 0,
      rxErrors: "0",
      txErrors: "0",
      rxDropped: "0",
      txDropped: "0",
    });
    const networks = [makeNetwork("docker0", false), makeNetwork("eth0", true)];
    expect(selectInterface(networks, "")?.name).toBe("eth0");
    expect(selectInterface(networks, "docker0")?.name).toBe("docker0");
    expect(selectInterface(networks, "eth1")).toBeUndefined();
    expect(rate(null)).toBe("—");
    expect(rate(0)).toBe("0 B/s");
    expect(rate(1024)).toBe("1.0 KiB/s");
  });
});
