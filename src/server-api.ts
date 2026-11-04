import { invoke } from "@tauri-apps/api/core";

export type RouteStep = { id: string; kind: "ssh"; serverId: string } | { id: string; kind: "socks5"; proxyId: string };
export interface Authentication {
  type: "password" | "privateKey" | "none" | "usernamePassword";
  privateKeyPath?: string | null;
  username?: string;
  hasSavedSecret: boolean;
  hasSessionSecret: boolean;
}
export interface Endpoint {
  id: string; name: string; host: string; port: number;
  authentication: Authentication; createdAt: string; updatedAt: string;
}
export interface Server extends Endpoint { protocol: "ssh"; username: string; groupId: string | null; route: RouteStep[] }
export interface Proxy extends Endpoint { protocol: "socks5" }
export interface Catalog {
  schemaVersion: 1; revision: number; groups: { id: string; name: string }[];
  servers: Server[]; proxies: Proxy[]; error: string | null; canRecover: boolean;
}
export type SecretChange = { action: "keep"; remember: boolean } | { action: "replace"; value: string; remember: boolean } | { action: "clear" };
export interface Draft {
  id: string | null; name: string; host: string; port: string; username: string;
  groupId: string; authType: Authentication["type"]; privateKeyPath: string;
  password: string; remember: boolean; clearSecret: boolean;
  route: RouteStep[]; routed: boolean;
}
export type Change =
  | { op: "saveServer" | "saveProxy"; draft: Record<string, unknown> }
  | { op: "deleteServer" | "deleteProxy" | "copyServer" | "copyProxy" | "deleteGroup"; id: string }
  | { op: "saveGroup"; id: string | null; name: string };
export const serverApi = {
  read: () => invoke<Catalog>("server_catalog"),
  change: (revision: number, change: Change) => invoke<{ catalog: Catalog; selectedId: string | null }>("change_catalog", { revision, change }),
  recover: () => invoke<Catalog>("recover_catalog"),
};
export type ServerApi = typeof serverApi;
export function newDraft(record?: Server | Proxy): Draft {
  return {
    id: record?.id ?? null, name: record?.name ?? "", host: record?.host ?? "",
    port: String(record?.port ?? 22), username: record?.protocol === "ssh" ? record.username : record?.authentication.username ?? "",
    groupId: record?.protocol === "ssh" ? record.groupId ?? "" : "", authType: record?.authentication.type ?? "password",
    privateKeyPath: record?.authentication.privateKeyPath ?? "", password: "",
    remember: record?.authentication.hasSavedSecret ?? false, clearSecret: false,
    route: record?.protocol === "ssh" ? structuredClone(record.route) : [], routed: record?.protocol === "ssh" && record.route.length > 0,
  };
}
export function toInput(d: Draft, proxy: boolean) {
  const secret: SecretChange = d.clearSecret ? { action: "clear" } : d.password ? { action: "replace", value: d.password, remember: d.remember } : { action: "keep", remember: d.remember };
  const common = { id: d.id, name: d.name, host: d.host, port: Number(d.port), authentication: { type: d.authType, username: d.username, privateKeyPath: d.privateKeyPath || null, secret } };
  return proxy ? common : { ...common, username: d.username, groupId: d.groupId || null, route: d.routed ? d.route : [] };
}
export const address = (r: Pick<Endpoint, "host" | "port">) => `${r.host.includes(":") ? `[${r.host}]` : r.host}:${r.port}`;
export const referenced = (catalog: Catalog, id: string) => catalog.servers.filter(s => s.route.some(r => r.kind === "ssh" ? r.serverId === id : r.proxyId === id));
