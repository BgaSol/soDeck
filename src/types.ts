export interface Server {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  tags: string[];
  authType: "agent" | "password" | "key";
  privateKey: string;
  hasSavedPassword: boolean;
  jumpServerId: string | null;
  directHost: string | null;
  directPort: number;
}
export interface Credentials {
  password: string;
  passphrase: string;
}
export interface AppError {
  kind: string;
  message: string;
  fingerprint?: string;
  previous?: string;
  hop?: number;
  stage?: string;
}
export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  isSymlink?: boolean;
  size: number;
  modified: number;
}
export interface Directory {
  path: string;
  resolvedPath?: string;
  entries: FileEntry[];
}
export interface DirectoryBatch {
  homePath: string;
  path: string;
  directories: Array<{ path: string; directory?: Directory; error?: string }>;
}
export interface DockerStatus {
  state: string;
  message: string;
  endpoint: string;
}
export interface DockerImage {
  id: string;
  name: string;
  size: string;
}
export interface Container {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
}
export type PasswordChange =
  { type: "keep" } | { type: "set"; value: string } | { type: "clear" };
export interface SaveServerRequest extends Server {
  passwordChange: PasswordChange;
}
export interface HopView {
  index: number;
  role: "jump" | "target";
  server: Server;
}
export interface ConnectionView {
  target: Server;
  hops: HopView[];
  identity: string;
}
export interface AuthChallenge {
  id: string;
  hop: HopView;
  message: string | null;
}
export interface HostKeyChallenge {
  id: string;
  hop: HopView;
  fingerprint: string;
  previous: string | null;
}
export type ConnectionAnswer =
  { type: "trust"; accept: boolean } | ({ type: "credentials" } & Credentials);
export type TerminalEvent =
  | { type: "progress"; hop: HopView; stage: string }
  | { type: "authRequired"; challenge: AuthChallenge }
  | { type: "hostKeyRequired"; challenge: HostKeyChallenge }
  | { type: "ready"; connection: ConnectionView }
  | { type: "data"; data: string }
  | {
      type: "history";
      sessionId: string;
      sourceId: string;
      workspaceId: string;
    }
  | { type: "displayCleared" }
  | {
      type: "closed";
      message: string;
      reason: "failed" | "cancelled" | "ended";
      error: AppError | null;
    }
  | { type: "frozen"; token: string }
  | { type: "cwd"; path: string; home: string | null; sequence: number };
export interface ConnectionRequest {
  serverId: string;
  attempt: number;
}
export const emptyServer = (): Server => ({
  id: "",
  name: "",
  host: "",
  port: 22,
  username: "",
  tags: [],
  authType: "agent",
  privateKey: "",
  hasSavedPassword: false,
  jumpServerId: null,
  directHost: null,
  directPort: 22,
});
export function errorOf(error: unknown): AppError {
  if (error && typeof error === "object" && "message" in error)
    return error as AppError;
  return { kind: "error", message: String(error) };
}
export function addressOf(server: Server) {
  return `${server.host.includes(":") ? "[" + server.host + "]" : server.host}:${server.port}`;
}
export function filterServers(servers: Server[], query: string, tag: string) {
  const text = query.trim().toLocaleLowerCase();
  return servers
    .filter(
      (server) =>
        (!text ||
          `${server.name} ${server.host}`.toLocaleLowerCase().includes(text)) &&
        (!tag || server.tags.includes(tag)),
    )
    .sort(
      (a, b) =>
        a.name.localeCompare(b.name, "zh-CN", { numeric: true }) ||
        a.host.localeCompare(b.host) ||
        a.port - b.port ||
        a.id.localeCompare(b.id),
    );
}
export function addTag(tags: string[], value: string, existing: string[]) {
  const tag = value.trim();
  if (
    !tag ||
    tags.some((t) => t.toLocaleLowerCase() === tag.toLocaleLowerCase())
  )
    return tags;
  return [
    ...tags,
    existing.find((t) => t.toLocaleLowerCase() === tag.toLocaleLowerCase()) ||
      tag,
  ];
}
export function formatSize(size: number) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 ** 2) return `${(size / 1024).toFixed(1)} KB`;
  if (size < 1024 ** 3) return `${(size / 1024 ** 2).toFixed(1)} MB`;
  return `${(size / 1024 ** 3).toFixed(1)} GB`;
}
