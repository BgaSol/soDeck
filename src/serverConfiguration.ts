import type {
  Server,
  ServerDraft,
  PasswordChange,
  DuplicatePasswordPolicy,
} from "./types";

export function duplicateDraft(source: Server, servers: Server[]): Server {
  const suffix = " - 副本";
  const names = new Set(servers.map((s) => s.name.toLocaleLowerCase()));
  let number = 1;
  let name: string;
  do {
    const tail = suffix + (number === 1 ? "" : ` ${number}`);
    name = source.name.slice(0, 120 - tail.length) + tail;
    number++;
  } while (names.has(name.toLocaleLowerCase()));
  return { ...structuredClone(source), id: "", name, hasSavedPassword: false };
}

export function configurationFields(server: Server): ServerDraft {
  return {
    name: server.name,
    host: server.host,
    port: server.port,
    username: server.username,
    tags: [...server.tags],
    authType: server.authType,
    privateKey: server.privateKey,
    jumpServerId: server.jumpServerId,
    directHost: server.directHost,
    directPort: server.directPort,
  };
}

export function createServerName(
  draft: Server,
  source: Server,
  servers: Server[],
): string {
  return source.id && draft.name.trim() === source.name.trim()
    ? duplicateDraft(source, servers).name
    : draft.name;
}

export function duplicatePasswordPolicy(
  source: Server,
  draft: Server,
  change: PasswordChange,
): DuplicatePasswordPolicy {
  if (draft.authType !== "password" || change.type === "clear")
    return { type: "clear" };
  if (change.type === "set") return change;
  return source.authType === "password" && source.hasSavedPassword
    ? { type: "inherit" }
    : { type: "clear" };
}

export function needsDirectAddressReview(
  draft: Server,
  source: Server,
): boolean {
  return (
    !!source.id &&
    !!source.directHost &&
    draft.host.trim() !== source.host.trim() &&
    draft.directHost === source.directHost &&
    draft.directPort === source.directPort
  );
}
