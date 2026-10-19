import { invoke, isTauri } from "@tauri-apps/api/core";
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
  deleteServer: (id: string) => invoke<void>("delete_server", { id }),
  directory: (sessionId: string, path?: string) =>
    invoke<Directory>("list_directory", { sessionId, path: path || null }),
  directoryChain: (
    sessionId: string,
    path?: string,
    knownPaths: string[] = [],
  ) =>
    invoke<DirectoryBatch>("list_directory_chain", {
      sessionId,
      path: path || null,
      knownPaths,
    }),
  docker: (sessionId: string) =>
    invoke<DockerStatus>("detect_docker", { sessionId }),
  images: (sessionId: string, endpoint: string) =>
    invoke<DockerImage[]>("docker_images", { sessionId, endpoint }),
  containers: (sessionId: string, endpoint: string) =>
    invoke<Container[]>("docker_containers", { sessionId, endpoint }),
  openContainer: (sessionId: string, container: Container, endpoint: string) =>
    invoke<void>("open_container", {
      sessionId,
      target: { id: container.id, name: container.name, endpoint },
    }),
  openContainerLogs: (
    sessionId: string,
    container: Container,
    endpoint: string,
  ) =>
    invoke<void>("open_container_logs", {
      sessionId,
      target: { id: container.id, name: container.name, endpoint },
    }),
};
