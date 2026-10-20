import { Channel, invoke } from "@tauri-apps/api/core";
export type ContainerAction =
  | "start"
  | "stop"
  | "restart"
  | "remove"
  | "pause"
  | "unpause"
  | "kill"
  | "rename"
  | "restartPolicy";
export type DockerAction = DockerActionPayload["type"];
export type DockerActionPayload =
  | { type: Exclude<ContainerAction, "rename" | "restartPolicy"> }
  | { type: "rename"; name: string }
  | { type: "restartPolicy"; policy: string; maximumRetryCount: number }
  | {
      type: "compose";
      mode: ComposeMode;
      associationId: string;
      revision: number;
    }
  | { type: "create"; draftId: string }
  | { type: "transfer"; draftId: string; kind: DockerTransferKind }
  | { type: "batch"; entries: DockerBatchEntry[] }
  | {
      type:
        | "volumeCreate"
        | "volumeRemove"
        | "networkCreate"
        | "networkRemove"
        | "cachePrune";
    }
  | { type: "networkConnect" | "networkDisconnect"; containerId: string }
  | { type: "imagePull" }
  | { type: "imageTag"; reference: string }
  | { type: "imageRemove"; reference: string | null };
export interface DockerEngineBinding {
  token: string;
  generation: string;
  engineGeneration: number;
  endpoint: string;
  daemonId: string | null;
}
export interface ContainerDetail {
  id: string;
  name: string;
  image: string;
  state: string;
  health: string | null;
  failingStreak: number | null;
  exitCode: number | null;
  oomKilled: boolean;
  error: string;
  restartCount: number;
  restartPolicy: string;
  restartMaximum: number;
  autoRemove: boolean;
  stopTimeout: number | null;
  created: string;
  startedAt: string;
  finishedAt: string;
  ports: {
    containerPort: string;
    hostIp: string | null;
    hostPort: string | null;
  }[];
  mounts: {
    kind: string;
    name: string | null;
    source: string;
    destination: string;
    readOnly: boolean;
  }[];
  networks: { name: string; id: string; address: string; ipv6: string }[];
  updatedAt: number;
  stats?: {
    cpuPercent: number | null;
    memory: string;
    memoryPercent: number | null;
    networkIo: string;
    blockIo: string;
    processes: string;
  } | null;
  statsError?: string | null;
  statsLoading?: boolean;
}
export interface DockerDetailState {
  containerId: string;
  value: ContainerDetail | null;
  loading: boolean;
  stale: boolean;
  errorKind?: string | null;
  missing: boolean;
  error: string | null;
}
export interface DockerOperation {
  id: string;
  requestId: string;
  engine: DockerEngineBinding;
  targetId: string;
  targetName: string;
  action: DockerActionPayload;
  phase: string;
  commandResult: string;
  message: string;
  observedState: string | null;
  observedHealth: string | null;
  createdAt: number;
  finishedAt: number | null;
  reconciling: boolean;
  createdContainerId?: string | null;
  items?: { targetId: string; name: string; phase: string; message: string }[];
}
export interface DockerPreview {
  id: string;
  engine: DockerEngineBinding;
  targetId: string;
  targetName: string;
  action: DockerActionPayload;
  consequences: string[];
  expiresAt: number;
}
export interface DockerFrame {
  catalogRevision?: number;
  subscriptionId: string;
  revision: number;
  engine: DockerEngineBinding | null;
  operations: DockerOperation[];
  detail: DockerDetailState | null;
  error: string | null;
}
export interface DockerOperationRequest {
  requestId: string;
  engineToken: string;
  targetId: string;
  action: DockerActionPayload;
  previewId: string | null;
}
export const dockerApi = {
  subscribe: (sessionId: string, receive: (frame: DockerFrame) => void) =>
    invoke<string>("subscribe_docker_management", {
      sessionId,
      events: new Channel(receive),
    }),
  unsubscribe: (subscriptionId: string) =>
    invoke<void>("unsubscribe_docker_management", { subscriptionId }),
  interest: (subscriptionId: string, containerId: string | null) =>
    invoke<void>("update_docker_management_interest", {
      subscriptionId,
      containerId,
    }),
  ack: (subscriptionId: string, revision: number) =>
    invoke<void>("ack_docker_management", { subscriptionId, revision }),
  refresh: (sessionId: string) =>
    invoke<void>("refresh_docker_management", { sessionId }),
  history: (sessionId: string, before: string | null) =>
    invoke<{ operations: DockerOperation[]; nextCursor: string | null }>(
      "list_docker_operations",
      { sessionId, before },
    ),
  prepare: (
    sessionId: string,
    engineToken: string,
    targetId: string,
    action: DockerActionPayload,
  ) =>
    invoke<DockerPreview>("prepare_docker_operation", {
      sessionId,
      engineToken,
      targetId,
      action,
    }),
  submit: (sessionId: string, request: DockerOperationRequest) =>
    invoke<string>("submit_docker_operation", { sessionId, request }),
  reconcile: (sessionId: string, operationId: string) =>
    invoke<void>("reconcile_docker_operation", { sessionId, operationId }),
  acknowledge: (sessionId: string, operationId: string) =>
    invoke<void>("acknowledge_docker_operation", { sessionId, operationId }),
  summary: () =>
    invoke<{ active: number; attention: number }>("docker_operation_summary"),
  prepareTransfer: (
    sessionId: string,
    engineToken: string,
    kind: DockerTransferKind,
    targetId: string,
    containerPath: string,
    conflict: string,
  ) =>
    invoke<DockerPreview | null>("prepare_docker_transfer", {
      sessionId,
      engineToken,
      kind,
      targetId,
      containerPath,
      conflict,
    }),
  prepareCreate: (
    sessionId: string,
    engineToken: string,
    specification: CreateContainerSpec,
  ) =>
    invoke<DockerPreview>("prepare_docker_create", {
      sessionId,
      engineToken,
      specification,
    }),
  resources: (sessionId: string, engineToken: string) =>
    invoke<DockerResources>("get_docker_resources", { sessionId, engineToken }),
  projects: (sessionId: string, engineToken: string) =>
    invoke<ComposeOverview>("get_docker_compose_projects", {
      sessionId,
      engineToken,
    }),
  saveAssociation: (
    sessionId: string,
    engineToken: string,
    association: ComposeAssociation,
  ) =>
    invoke<ComposeAssociation>("save_docker_compose_association", {
      sessionId,
      engineToken,
      association,
    }),
  removeAssociation: (
    sessionId: string,
    associationId: string,
    revision: number,
  ) =>
    invoke<void>("remove_docker_compose_association", {
      sessionId,
      associationId,
      revision,
    }),
  hostLink: (
    sessionId: string,
    engineToken: string,
    containerId: string,
    request:
      | { type: "mount"; destination: string }
      | {
          type: "port";
          containerPort: string;
          hostIp: string;
          hostPort: string;
        },
  ) =>
    invoke<{
      path: string | null;
      host: string | null;
      port: number | null;
      name: string;
    }>("get_docker_host_link", {
      sessionId,
      engineToken,
      containerId,
      request,
    }),
  image: (sessionId: string, engineToken: string, imageId: string) =>
    invoke<ImageDetail>("get_docker_image_detail", {
      sessionId,
      engineToken,
      imageId,
    }),
  diagnostics: (
    sessionId: string,
    engineToken: string,
    containerId: string,
    section: "configuration" | "processes",
    reveal: boolean,
  ) =>
    invoke<string>("get_docker_container_diagnostics", {
      sessionId,
      engineToken,
      containerId,
      section,
      reveal,
    }),
};
export const dockerActionNames: Record<DockerAction, string> = {
  volumeCreate: "创建数据卷",
  volumeRemove: "删除数据卷",
  networkCreate: "创建网络",
  networkRemove: "删除网络",
  networkConnect: "连接网络",
  networkDisconnect: "断开网络",
  cachePrune: "清理构建缓存",
  transfer: "Docker 传输",
  batch: "批量维护",
  create: "创建容器",
  compose: "管理项目",
  imagePull: "拉取镜像",
  imageTag: "添加 Tag",
  imageRemove: "删除镜像或引用",
  start: "启动",
  stop: "停止",
  restart: "重启",
  remove: "删除",
  pause: "暂停",
  unpause: "恢复",
  kill: "强制停止",
  rename: "重命名",
  restartPolicy: "修改重启策略",
};
export function operationActive(task: DockerOperation) {
  return (
    ["accepted", "checking", "running", "verifying"].includes(task.phase) ||
    task.reconciling
  );
}
export function operationBlocks(task: DockerOperation) {
  return operationActive(task) || task.phase === "unknown";
}
export function healthName(health: string | null) {
  return health === "healthy"
    ? "健康"
    : health === "unhealthy"
      ? "不健康"
      : health === "starting"
        ? "健康检查中"
        : health
          ? "健康状态未知"
          : "未配置健康检查";
}

export interface ImageDetail {
  id: string;
  tags: string[];
  digests: string[];
  created: string;
  platform: string;
  size: number;
  containers: { id: string; name: string; state: string }[];
}

export type ComposeMode =
  "start" | "stop" | "restart" | "pull" | "apply" | "down";
export const composeActionNames: Record<ComposeMode, string> = {
  start: "启动项目",
  stop: "停止项目",
  restart: "重启项目",
  pull: "更新项目镜像",
  apply: "应用配置",
  down: "停止并移除项目",
};
export function actionName(action: DockerActionPayload) {
  if (action.type === "transfer") return transferActionNames[action.kind];
  return action.type === "compose"
    ? composeActionNames[action.mode]
    : dockerActionNames[action.type];
}
export interface ComposeAssociation {
  id: string;
  revision: number;
  serverId: string;
  endpoint: string;
  daemonId: string | null;
  project: string;
  directory: string;
  files: string[];
  envFiles: string[];
  profiles: string[];
}
export interface ComposeProject {
  name: string;
  directory: string;
  files: string[];
  containers: { id: string; name: string; service: string; state: string }[];
}
export interface ComposeOverview {
  available: boolean;
  message: string;
  projects: ComposeProject[];
  associations: ComposeAssociation[];
}

export interface CreateContainerSpec {
  imageId: string;
  name: string;
  start: boolean;
  ports: {
    hostIp: string;
    hostPort: number;
    containerPort: number;
    protocol: string;
  }[];
  environment: { key: string; value: string }[];
  mounts: {
    kind: string;
    source: string;
    destination: string;
    readOnly: boolean;
  }[];
  network: string | null;
  restartPolicy: string;
  maximumRetryCount: number;
  command: string[];
  workingDirectory: string;
}

export interface DockerResource {
  id: string;
  name: string;
  kind: "volume" | "network";
  driver: string;
  created: string;
  protected: boolean;
  mountpoint: string;
  containers: { id: string; name: string; state: string }[];
  configuration: Record<string, unknown>;
}
export interface DockerResources {
  candidates: {
    id: string;
    name: string;
    kind: "container" | "image" | "volume" | "network";
  }[];
  usage: {
    Type: string;
    TotalCount: string | number;
    Active: string | number;
    Size: string;
    Reclaimable: string;
  }[];
  volumes: DockerResource[];
  networks: DockerResource[];
}

export interface DockerBatchEntry {
  targetId: string;
  action:
    | "start"
    | "stop"
    | "restart"
    | "removeContainer"
    | "removeImage"
    | "removeVolume"
    | "removeNetwork";
}

export type DockerTransferKind =
  "imageSave" | "imageLoad" | "copyOut" | "copyIn";
export const transferActionNames: Record<DockerTransferKind, string> = {
  imageSave: "导出镜像",
  imageLoad: "导入镜像",
  copyOut: "导出容器文件",
  copyIn: "复制文件到容器",
};
export function isContainerAction(action: DockerActionPayload) {
  return [
    "start",
    "stop",
    "restart",
    "pause",
    "unpause",
    "remove",
    "kill",
    "rename",
    "restartPolicy",
  ].includes(action.type);
}
