import { contextReference } from "./toolContext";
import { isTauri } from "@tauri-apps/api/core";
import { invokeDiagnosed as invoke } from "./diagnostics";
import type {
  Server,
  SaveServerRequest,
  DuplicateServerRequest,
  Directory,
  DirectoryBatch,
  DockerStatus,
  DockerImage,
  Container,
} from "./types";
export const desktop = isTauri();
export const api = {
  listServers: () => invoke<Server[]>("list_servers"),
  resetIncompatibleDatabase: () =>
    invoke<string>("reset_incompatible_database"),
  saveServer: (server: SaveServerRequest) =>
    invoke<Server>("save_server", { server }),
  duplicateServer: (request: DuplicateServerRequest) =>
    invoke<Server>("duplicate_server", { request }),
  copySavedServerPassword: (id: string) =>
    invoke<void>("copy_saved_server_password", { id }),
  deleteServer: (id: string) => invoke<void>("delete_server", { id }),
  directory: (contextId: string, path?: string) =>
    invoke<Directory>("list_directory", {
      context: contextReference(contextId),
      path: path || null,
    }),
  directoryChain: (
    contextId: string,
    path?: string,
    knownPaths: string[] = [],
  ) =>
    invoke<DirectoryBatch>("list_directory_chain", {
      context: contextReference(contextId),
      path: path || null,
      knownPaths,
    }),
  docker: (contextId: string) =>
    invoke<DockerStatus>("detect_docker", {
      context: contextReference(contextId),
    }),
  images: (contextId: string, endpoint: string) =>
    invoke<DockerImage[]>("docker_images", {
      context: contextReference(contextId),
      endpoint,
    }),
  containers: (contextId: string, endpoint: string) =>
    invoke<Container[]>("docker_containers", {
      context: contextReference(contextId),
      endpoint,
    }),
  openContainer: (contextId: string, container: Container, endpoint: string) =>
    invoke<void>("open_container", {
      context: contextReference(contextId),
      target: { id: container.id, name: container.name, endpoint },
    }),
  openContainerLogs: (
    contextId: string,
    container: Container,
    endpoint: string,
  ) =>
    invoke<void>("open_container_logs", {
      context: contextReference(contextId),
      target: { id: container.id, name: container.name, endpoint },
    }),
};
