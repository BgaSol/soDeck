import type { Container, DockerImage, DockerStatus } from "./types";

export type DockerPage = "containers" | "images";
export type DockerFilter = "all" | "running" | "stopped" | "other";
export type DockerSort = "name" | "attention";
export interface DockerSelection {
  kind: DockerPage;
  id: string;
  name: string;
  engineGeneration?: number;
  endpoint?: string;
}
export interface DockerPanelSnapshot {
  page?: DockerPage;
  queries?: { containers: string; images: string };
  filter?: DockerFilter;
  sort?: DockerSort;
  selection?: DockerSelection | null;
  scrollTop?: { containers: number; images: number };
  expandedScrollTop?: { containers: number; images: number }; // Legacy snapshots only.
  detailOpen?: boolean;
  // Only read these fields from older window snapshots, never write live data.
  rootOpen?: boolean;
  imagesOpen?: boolean;
  containersOpen?: boolean;
  status?: DockerStatus;
  containers?: Container[];
  images?: DockerImage[];
}
export function normalizeDockerSnapshot(snapshot?: DockerPanelSnapshot) {
  const page: DockerPage =
    snapshot?.page === "images" ||
    (!snapshot?.page && snapshot?.imagesOpen && !snapshot.containersOpen)
      ? "images"
      : "containers";
  return {
    page,
    queries: {
      containers: snapshot?.queries?.containers ?? "",
      images: snapshot?.queries?.images ?? "",
    },
    filter: (["running", "stopped", "other"].includes(snapshot?.filter ?? "")
      ? snapshot!.filter
      : "all") as DockerFilter,
    sort:
      snapshot?.sort === "attention"
        ? ("attention" as const)
        : ("name" as const),
    selection: snapshot?.selection ?? null,
    scrollTop: {
      containers: Math.max(0, snapshot?.scrollTop?.containers ?? 0),
      images: Math.max(0, snapshot?.scrollTop?.images ?? 0),
    },
    detailOpen: snapshot?.detailOpen ?? false,
  };
}
export function dockerKey(item: Container | DockerImage) {
  return "state" in item ? item.id : `${item.id}/${item.name}`;
}
export function dockerMatches(
  item: Container | DockerImage,
  query: string,
  filter: DockerFilter = "all",
) {
  const term = query.trim().toLocaleLowerCase();
  if (
    term &&
    ![item.name, item.id, "image" in item ? item.image : ""].some((value) =>
      value.toLocaleLowerCase().includes(term),
    )
  )
    return false;
  if (!("state" in item) || filter === "all") return true;
  return filter === "running"
    ? item.state === "running"
    : filter === "stopped"
      ? item.state === "exited"
      : item.state !== "running" && item.state !== "exited";
}
export function orderDockerItems<T extends Container | DockerImage>(
  items: T[],
  sort: DockerSort,
  previous: string[] = [],
) {
  const positions = new Map(previous.map((key, index) => [key, index]));
  const rank = (item: T) =>
    sort === "attention" &&
    "state" in item &&
    ["dead", "restarting"].includes(item.state)
      ? 0
      : 1;
  return [...items].sort((a, b) => {
    const ai = positions.get(dockerKey(a)),
      bi = positions.get(dockerKey(b));
    if (ai != null || bi != null) return (ai ?? Infinity) - (bi ?? Infinity);
    return (
      rank(a) - rank(b) ||
      a.name.localeCompare(b.name, undefined, { numeric: true }) ||
      dockerKey(a).localeCompare(dockerKey(b))
    );
  });
}
export const containerStateNames: Record<string, string> = {
  running: "运行中",
  exited: "已停止",
  paused: "已暂停",
  restarting: "重启中",
  created: "已创建",
  dead: "已结束",
  removing: "删除中",
};

export const dockerStatusNames: Record<string, string> = {
  running: "可访问",
  missing: "未找到 Docker 命令",
  permission: "权限不足",
  unreachable: "服务不可达",
  error: "检测失败",
  disconnected: "未连接",
  detecting: "检测中",
};
