import { contextReference } from "../toolContext";
import { Channel } from "@tauri-apps/api/core";
import { invokeDiagnosed as invoke } from "../diagnostics";
export type Action = "up" | "down" | "serviceStart" | "serviceRestart";
export type Interest = "none" | "background" | "visible";
export interface Binding {
  token: string;
  cliPath: string;
  daemonAddress: string;
  clientVersion: string | null;
  daemonVersion: string | null;
  profile: string | null;
  capabilities: Record<Action | "json", boolean> & { reason: string | null };
}
export interface Component {
  state: string;
  address: string | null;
  message: string | null;
}
export interface Peer {
  id: string;
  name: string;
  fqdn: string | null;
  ipv4: string | null;
  ipv6: string | null;
  state: string;
  rawState: string | null;
  connectionMode: string;
  latencyMs: number | null;
  latencyMeasuredAt: number | null;
  lastHandshakeAt: number | null;
  stateChangedAt: number | null;
  receivedBytes: string | null;
  sentBytes: string | null;
  networks: string[];
}
export interface Snapshot {
  binding: Binding | null;
  collectionStatus: string;
  updatedAt: number | null;
  error: string | null;
  installation: string;
  service: string;
  authentication: string;
  connection: string;
  localPeer: Peer | null;
  management: Component;
  signal: Component;
  relay: Component;
  dns: Component;
  peers: Peer[];
  peersComplete: boolean;
  reportedTotal: number | null;
}
export interface Operation {
  id: string;
  requestId: string;
  action: Action;
  phase: string;
  message: string;
  createdAt: number;
  finishedAt: number | null;
  acknowledged: boolean;
}
export interface Frame {
  subscriptionId: string;
  revision: number;
  connectionIdentity: string;
  snapshot: Snapshot;
  operations: Operation[];
  refreshing: boolean;
}
export interface Preview {
  id: string;
  bindingToken: string;
  action: Action;
  expiresAt: number;
  allowed: boolean;
  path: { result: string; checkedAt: number; reasons: string[] };
  consequences: string[];
}
export interface OperationRequest {
  requestId: string;
  bindingToken: string;
  action: Action;
  previewId: string;
}
export interface Diagnostic {
  text: string;
  source: string;
  checkedAt: number;
  truncated: boolean;
}
export interface ProbeResult {
  state: string;
  message: string;
  address: string;
  source: string;
  checkedAt: number;
}
export const netbirdApi = {
  subscribe(
    contextId: string,
    level: Interest,
    receive: (frame: Frame) => void,
  ) {
    const events = new Channel<Frame>();
    events.onmessage = receive;
    return invoke<string>("subscribe_netbird", {
      context: contextReference(contextId),
      interest: { level },
      events,
    });
  },
  interest: (subscriptionId: string, level: Interest) =>
    invoke<void>("update_netbird_interest", {
      subscriptionId,
      interest: { level },
    }),
  unsubscribe: (subscriptionId: string) =>
    invoke<void>("unsubscribe_netbird", { subscriptionId }),
  ack: (subscriptionId: string, revision: number) =>
    invoke<void>("ack_netbird_frame", { subscriptionId, revision }),
  refresh: (subscriptionId: string) =>
    invoke<void>("refresh_netbird", { subscriptionId }),
  raw: (contextId: string, bindingToken: string) =>
    invoke<Diagnostic>("get_netbird_raw_status", {
      context: contextReference(contextId),
      bindingToken,
    }),
  logs: (contextId: string, bindingToken: string) =>
    invoke<Diagnostic>("get_netbird_logs", {
      context: contextReference(contextId),
      bindingToken,
    }),
  probe: (
    contextId: string,
    bindingToken: string,
    request: { peerId: string; kind: "icmp" | "tcp"; port: number | null },
  ) =>
    invoke<ProbeResult>("probe_netbird_peer", {
      context: contextReference(contextId),
      bindingToken,
      request,
    }),
  prepare: (contextId: string, bindingToken: string, action: Action) =>
    invoke<Preview>("prepare_netbird_operation", {
      context: contextReference(contextId),
      bindingToken,
      action,
    }),
  release: (contextId: string, previewId: string) =>
    invoke<void>("release_netbird_preview", {
      context: contextReference(contextId),
      previewId,
    }),
  submit: (contextId: string, request: OperationRequest) =>
    invoke<string>("submit_netbird_operation", {
      context: contextReference(contextId),
      request,
    }),
  reconcile: (contextId: string, operationId: string) =>
    invoke<void>("reconcile_netbird_operation", {
      context: contextReference(contextId),
      operationId,
    }),
  acknowledge: (contextId: string, operationId: string) =>
    invoke<void>("acknowledge_netbird_operation", {
      context: contextReference(contextId),
      operationId,
    }),
  path: (contextId: string, path: string | null) =>
    invoke<void>("set_netbird_cli_path", {
      context: contextReference(contextId),
      path,
    }),
};
