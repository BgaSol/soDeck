import { Channel } from "@tauri-apps/api/core";
import { invokeDiagnosed as invoke } from "./diagnostics";
import type {
  AppError,
  ConnectionAnswer,
  ConnectionView,
  Server,
  TerminalEvent,
} from "./types";

export type ForwardService = { kind: "portForward"; targetHost: string; targetPort: number } | { kind: "networkProxy" };
export type ForwardKind = ForwardService["kind"];
export interface ForwardSettings {
  serverId: string;
  listenAddress: string;
  listenPort: number | null;
  service: ForwardService;
  autoReconnect: boolean;
}
export interface ForwardRule {
  id: string;
  revision: number;
  name: string;
  webScheme: "http" | "https" | null;
  settings: ForwardSettings;
}
export interface ForwardNote {
  at: number;
  message: string;
  count?: number;
}
export type ForwardPrompt = {
  attemptId: string;
  event: Extract<TerminalEvent, { type: "authRequired" | "hostKeyRequired" }>;
};
export interface ForwardRuntime {
  runId: string;
  phase: string;
  actualAddress: string | null;
  settings: ForwardSettings;
  connection: ConnectionView | null;
  startedAt: number;
  activeConnections: number;
  sentBytes: string;
  receivedBytes: string;
  sentPerSecond: number;
  receivedPerSecond: number;
  attempts: number;
  retryAt: number | null;
  error: AppError | null;
  prompt: ForwardPrompt | null;
  targetResult: {
    at: number;
    success: boolean;
    source: "probe" | "connection";
    message: string;
  } | null;
  probeId: string | null;
  proxy: ProxyRuntime | null;
  notes: ForwardNote[];
}
export interface ProxyProbeRequest {
  protocol: "httpConnect" | "socks5";
  target: { host: string; port: number };
}
export interface ProxyProbeResult extends ProxyProbeRequest {
  probeId: string; runId: string; startedAt: number; finishedAt: number;
  success: boolean; stage: string; message: string;
}
export interface ProxyRuntime {
  negotiatingConnections: number; waitingChannels: number; idleClients: number; activeTargetChannels: number;
  lastProbe: ProxyProbeResult | null;
  lastConnection: { at: number; host: string; port: number; protocol: string; stage: string; success: boolean; message: string } | null;
}
export interface ForwardView {
  rule: ForwardRule;
  runtime: ForwardRuntime | null;
  pendingChanges: boolean;
  serverError: string | null;
}
export interface ForwardSummary {
  ready: number;
  active: number;
  connections: number;
  attention: number;
}
export interface ForwardSeed {
  name: string;
  targetHost: string;
  targetPort: number;
}
export interface ForwardOpenRequest {
  id: string;
  serverId: string | null;
  seed?: ForwardSeed | null;
}
export interface ForwardFrame {
  subscriptionId: string;
  revision: number;
  rules: ForwardView[];
  servers: Server[];
  openRequest: ForwardOpenRequest | null;
  error: AppError | null;
}
export const forwardApi = {
  open: (serverId?: string, seed?: ForwardSeed) =>
    invoke<void>("open_port_forward_window", {
      serverId: serverId || null,
      seed: seed ?? null,
    }),
  summary: () => invoke<ForwardSummary>("port_forward_summary"),
  bootstrap: () =>
    invoke<{ generation: number; servers: Server[] }>("port_forward_bootstrap"),
  subscribe: (generation: number, output: Channel<ForwardFrame>) =>
    invoke<string>("subscribe_port_forwards", { generation, output }),
  ack: (generation: number, subscriptionId: string, revision: number) =>
    invoke<void>("ack_port_forward_frame", {
      generation,
      subscriptionId,
      revision,
    }),
  unsubscribe: (generation: number, subscriptionId: string) =>
    invoke<void>("unsubscribe_port_forwards", { generation, subscriptionId }),
  save: (generation: number, rule: ForwardRule) =>
    invoke<ForwardRule>("save_port_forward", { generation, rule }),
  start: (generation: number, rule: ForwardRule) =>
    invoke<string>("start_port_forward", {
      generation,
      id: rule.id,
      revision: rule.revision,
    }),
  stop: (generation: number, view: ForwardView) =>
    invoke<void>("stop_port_forward", {
      generation,
      id: view.rule.id,
      runId: view.runtime?.runId,
    }),
  apply: (generation: number, view: ForwardView, remove = false) =>
    invoke<void>("apply_port_forward", {
      generation,
      id: view.rule.id,
      revision: view.rule.revision,
      runId: view.runtime?.runId,
      delete: remove,
    }),
  remove: (generation: number, rule: ForwardRule) =>
    invoke<void>("delete_port_forward", {
      generation,
      id: rule.id,
      revision: rule.revision,
    }),
  resume: (generation: number, view: ForwardView) =>
    invoke<void>("resume_port_forward", {
      generation,
      id: view.rule.id,
      runId: view.runtime?.runId,
    }),
  answer: (
    generation: number,
    view: ForwardView,
    prompt: ForwardPrompt,
    answer: ConnectionAnswer | null,
  ) =>
    invoke<void>("answer_port_forward", {
      generation,
      id: view.rule.id,
      runId: view.runtime?.runId,
      attemptId: prompt.attemptId,
      challengeId: prompt.event.challenge.id,
      answer,
    }),
  probe: (
    generation: number,
    view: ForwardView,
    cancelProbeId: string | null = null,
  ) =>
    invoke<string>("test_port_forward", {
      generation,
      id: view.rule.id,
      runId: view.runtime?.runId,
      cancelProbeId,
    }),
  proxyProbe: (generation: number, view: ForwardView, request: ProxyProbeRequest) =>
    invoke<string>("test_network_proxy", { generation, id: view.rule.id, runId: view.runtime?.runId, request }),
  cancelProxyProbe: (generation: number, view: ForwardView, probeId: string) =>
    invoke<void>("cancel_network_proxy_test", { generation, id: view.rule.id, runId: view.runtime?.runId, probeId }),
  openWeb: (generation: number, view: ForwardView) =>
    invoke<void>("open_port_forward_web", {
      generation,
      id: view.rule.id,
      runId: view.runtime?.runId,
    }),
};

export function emptyForward(serverId = ""): ForwardRule & { settings: ForwardSettings & { service: Extract<ForwardService, {kind: "portForward"}> } } {
  return {
    id: "",
    revision: 0,
    name: "",
    webScheme: null,
    settings: {
      serverId,
      listenAddress: "127.0.0.1",
      listenPort: null,
      service: { kind: "portForward", targetHost: "127.0.0.1", targetPort: 0 },
      autoReconnect: true,
    },
  };
}
export function emptyNetworkProxy(serverId = ""): ForwardRule {
  return { ...emptyForward(serverId), settings: { serverId, listenAddress: "127.0.0.1", listenPort: 1080, autoReconnect: true, service: {kind: "networkProxy"} } };
}
export const forwardPhases: Record<string, string> = {
  starting: "启动中",
  authenticating: "等待认证／核对指纹",
  ready: "已启动",
  reconnecting: "正在重连",
  attention: "需处理",
  stopping: "停止中",
  stopped: "已停止",
  failed: "启动失败",
};
export function forwardActive(view: ForwardView) {
  return !!view.runtime && !["failed", "stopped"].includes(view.runtime.phase);
}
export function endpoint(host: string, port: number | null) {
  return (
    (host.includes(":") ? "[" + host + "]" : host) + ":" + (port ?? "自动")
  );
}
