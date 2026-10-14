export interface Server {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  tags: string[];
  authType: "agent" | "password" | "key";
  privateKey: string;
  password: string;
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
export type TerminalEvent =
  | { type: "data"; data: string }
  | { type: "closed"; message: string }
  | { type: "frozen"; token: string }
  | { type: "cwd"; path: string; home: string | null; sequence: number };
export interface ConnectionRequest {
  server: Server;
  credentials: Credentials;
  trustFingerprint?: string;
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
  password: "",
});
export function automaticCredentials(server: Server): Credentials | undefined {
  if (server.authType === "agent") return { password: "", passphrase: "" };
  if (server.authType === "password" && server.password)
    return { password: server.password, passphrase: "" };
  return undefined;
}
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
