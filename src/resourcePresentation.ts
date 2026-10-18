import type { Metric } from "./resourceMonitoring";
import type { RemoteMount } from "./remoteMounts";
import { mountKind } from "./remoteMounts";
export type ResourceMetric = "cpu" | "memory" | "network";
export type MountGroup = "storage" | "system" | "runtime" | "memory";
export interface ResourcePreferences {
  tab: "monitor" | "docker" | "netbird";
  minutes: 5 | 15 | 60;
  networkInterface: string;
  collapsed?: string[]; // Legacy snapshots only.
  selectedMetric?: ResourceMetric;
  detailOpen?: boolean;
  pinnedMounts?: string[];
  trafficScope?: "total" | "monitoring"; // Read once into connection preferences.
  scrollTop?: { compact: number; expanded?: number };
  mounts?: {
    all: boolean;
    query: string;
    onlyPinned: boolean;
    groups: MountGroup[];
    page: number;
  };
}
export const defaultResourcePreferences: ResourcePreferences = {
  tab: "monitor",
  minutes: 5,
  networkInterface: "",
  selectedMetric: "cpu",
  pinnedMounts: ["/"],
};
export function normalizeResourcePreferences(
  value?: Partial<ResourcePreferences>,
): ResourcePreferences {
  const {
    collapsed: _collapsed,
    trafficScope: _traffic,
    ...current
  } = value ?? {};
  return {
    ...defaultResourcePreferences,
    ...current,
    tab:
      value?.tab === "docker" || value?.tab === "netbird"
        ? value.tab
        : "monitor",
    minutes: value?.minutes === 15 || value?.minutes === 60 ? value.minutes : 5,
    networkInterface:
      typeof value?.networkInterface === "string" ? value.networkInterface : "",
    detailOpen: value?.detailOpen === true,
    selectedMetric:
      value?.selectedMetric === "memory" || value?.selectedMetric === "network"
        ? value.selectedMetric
        : "cpu",
    pinnedMounts: [
      ...new Set(
        value?.pinnedMounts?.filter(
          (p) => typeof p === "string" && p.startsWith("/"),
        ) ?? ["/"],
      ),
    ].slice(0, 3),
  };
}
export function sampleTime(at: number | null | undefined) {
  return at == null
    ? "尚无采样"
    : `更新于 ${new Date(at).toLocaleTimeString()}`;
}
export function metricMessage(metric: Metric<unknown>, connected: boolean) {
  const label = !connected
    ? "已断开"
    : metric.status === "collecting"
      ? "正在采集"
      : metric.status === "unsupported"
        ? "当前环境不支持"
        : metric.status === "ready"
          ? ""
          : "暂不可用";
  return (
    [
      label,
      metric.updatedAt == null
        ? ""
        : `${label ? "上次采样 " : "更新于 "}${new Date(metric.updatedAt).toLocaleTimeString()}`,
    ]
      .filter(Boolean)
      .join(" · ") || "尚无采样"
  );
}
export function hasOldValue(metric: Metric<unknown>, connected: boolean) {
  return metric.value != null && (!connected || metric.status !== "ready");
}
const pseudo = new Set([
  "proc",
  "sysfs",
  "cgroup",
  "cgroup2",
  "devpts",
  "securityfs",
  "debugfs",
  "tracefs",
  "configfs",
  "pstore",
  "mqueue",
  "hugetlbfs",
  "autofs",
  "fusectl",
  "binfmt_misc",
  "rpc_pipefs",
]);
export function mountGroup(mount: RemoteMount): MountGroup {
  if (mount.target === "/") return "storage";
  if (mountKind(mount) === "memory") return "memory";
  if (
    pseudo.has(mount.fsType) ||
    (mount.fsType === "squashfs" && mount.target.startsWith("/snap/"))
  )
    return "system";
  if (
    mount.fsType === "nsfs" ||
    (mount.fsType === "overlay" &&
      /^\/(?:var\/lib\/(?:docker|containerd|containers)|run\/(?:docker|containerd|containers))(?:\/|$)/.test(
        mount.target,
      ))
  )
    return "runtime";
  return "storage";
}
