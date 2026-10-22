import type { WorkspaceToolState } from "./workspaceTools";

export interface WorkspaceLayoutSize {
  fileWidth: number;
  serviceWidth: number;
}
export type WorkspacePane = "files" | "terminal" | "services";
/** Read compatibility for snapshots made before tool windows. */
export interface WorkspaceViewState {
  expanded?: boolean;
  pane: WorkspacePane;
}
export interface WorkspaceVisibility {
  files: boolean;
  terminal: boolean;
  services: boolean;
}
export const defaultWorkspaceSize: WorkspaceLayoutSize = {
  fileWidth: 280,
  serviceWidth: 360,
};
export const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(max, value));
export function workspaceMetrics(scale = 1) {
  const s = Number.isFinite(scale) && scale > 0 ? scale : 1;
  return {
    rail: 44 * s,
    separator: 10 * s,
    terminal: 560 * s,
    single: 760 * s,
    fileMin: 240 * s,
    fileMax: 520 * s,
    serviceMin: 320 * s,
    serviceMax: 480 * s,
    scale: s,
  };
}
export type WorkspaceMetrics = ReturnType<typeof workspaceMetrics>;
export type PanelPlacement = "hidden" | "docked" | "overlay" | "full";
export interface ResolvedWorkspaceLayout {
  mode: "docked" | "overlay" | "single";
  files: { placement: PanelPlacement; width: number };
  services: { placement: PanelPlacement; width: number };
  terminalVisible: boolean;
  visible: WorkspaceVisibility;
  resizeBounds: Record<
    keyof WorkspaceLayoutSize,
    { min: number; max: number } | null
  >;
}
export function resolveWorkspaceLayout({
  width,
  metrics: m = workspaceMetrics(),
  tools,
  preferred = defaultWorkspaceSize,
}: {
  width: number;
  metrics?: WorkspaceMetrics;
  tools: WorkspaceToolState;
  preferred?: WorkspaceLayoutSize;
}): ResolvedWorkspaceLayout {
  width = Number.isFinite(width) ? Math.max(0, width) : 0;
  const center = Math.max(0, width - 2 * m.rail);
  const finite = (n: number, fallback: number) =>
    Number.isFinite(n) && n > 0 ? n : fallback;
  const files = {
    placement: "hidden" as PanelPlacement,
    width: clamp(
      finite(preferred.fileWidth, 280) * m.scale,
      m.fileMin,
      m.fileMax,
    ),
  };
  const services = {
    placement: "hidden" as PanelPlacement,
    width: clamp(
      finite(preferred.serviceWidth, 360) * m.scale,
      m.serviceMin,
      m.serviceMax,
    ),
  };
  const resizeBounds: ResolvedWorkspaceLayout["resizeBounds"] = {
    fileWidth: null,
    serviceWidth: null,
  };
  let mode: ResolvedWorkspaceLayout["mode"] = "docked",
    terminalVisible = true;
  const requested = { files: tools.filesOpen, services: tools.servicesOpen };
  if (width < m.single) {
    mode = "single";
    if (tools.compactPane !== "terminal" && requested[tools.compactPane]) {
      const panel = tools.compactPane === "files" ? files : services;
      panel.placement = "full";
      panel.width = center;
      terminalVisible = false;
    }
  } else {
    const budget = center - m.terminal;
    const total =
      (tools.filesOpen ? files.width + m.separator : 0) +
      (tools.servicesOpen ? services.width + m.separator : 0);
    if (total <= budget) {
      if (tools.filesOpen) files.placement = "docked";
      if (tools.servicesOpen) services.placement = "docked";
    } else if (tools.filesOpen || tools.servicesOpen) {
      const side = requested[tools.lastSide]
        ? tools.lastSide
        : tools.filesOpen
          ? "files"
          : "services";
      const panel = side === "files" ? files : services;
      const min = side === "files" ? m.fileMin : m.serviceMin;
      if (min + m.separator <= budget) {
        panel.placement = "docked";
        panel.width = Math.min(panel.width, budget - m.separator);
      } else {
        mode = "overlay";
        panel.placement = "overlay";
        panel.width = Math.min(panel.width, Math.max(0, center - 16 * m.scale));
      }
    }
    if (files.placement === "docked")
      resizeBounds.fileWidth = {
        min: m.fileMin,
        max: Math.min(
          m.fileMax,
          budget -
            m.separator -
            (services.placement === "docked"
              ? services.width + m.separator
              : 0),
        ),
      };
    if (services.placement === "docked")
      resizeBounds.serviceWidth = {
        min: m.serviceMin,
        max: Math.min(
          m.serviceMax,
          budget -
            m.separator -
            (files.placement === "docked" ? files.width + m.separator : 0),
        ),
      };
  }
  return {
    mode,
    files,
    services,
    terminalVisible,
    resizeBounds,
    visible: {
      files: files.placement !== "hidden",
      services: services.placement !== "hidden",
      terminal: terminalVisible,
    },
  };
}
