import type { Server, ServerDraft } from "./types";

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

export function updateDuplicateHost(
  draft: Server,
  host: string,
  source: Server,
  directEdited: boolean,
): Server {
  return {
    ...draft,
    host,
    ...(!directEdited && source.directHost && host.trim() !== source.host.trim()
      ? { directHost: null, directPort: 22 }
      : {}),
  };
}
