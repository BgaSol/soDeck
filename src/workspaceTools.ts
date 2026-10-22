import {
  normalizeResourcePreferences,
  type ResourcePreferences,
  type ResourceMetric,
} from "./resourcePresentation";
import {
  clamp,
  defaultWorkspaceSize,
  type WorkspaceLayoutSize,
  type WorkspaceViewState,
} from "./workspaceSizing";
export type ServiceTool = ResourcePreferences["tab"];
export type ToolSide = "files" | "services";
export interface WorkspaceToolState {
  version: 2;
  filesOpen: boolean;
  servicesOpen: boolean;
  lastSide: ToolSide;
  compactPane: "terminal" | ToolSide;
}
export interface WorkspacePresentationState {
  tools: WorkspaceToolState;
  layout: WorkspaceLayoutSize;
  resources: ResourcePreferences;
}
export const defaultTools: WorkspaceToolState = {
  version: 2,
  filesOpen: true,
  servicesOpen: false,
  lastSide: "files",
  compactPane: "terminal",
};
export function normalizeWorkspacePresentation(initial?: {
  tools?: Partial<WorkspaceToolState>;
  view?: Partial<WorkspaceViewState>;
  layout?: Partial<WorkspaceLayoutSize>;
  resources?: Partial<ResourcePreferences>;
  resourceWidth?: number;
}): WorkspacePresentationState {
  let tools = { ...defaultTools };
  const t = initial?.tools;
  if (t?.version === 2)
    tools = {
      version: 2,
      filesOpen: typeof t.filesOpen === "boolean" ? t.filesOpen : true,
      servicesOpen:
        typeof t.servicesOpen === "boolean" ? t.servicesOpen : false,
      lastSide: t.lastSide === "services" ? "services" : "files",
      compactPane:
        t.compactPane === "files" || t.compactPane === "services"
          ? t.compactPane
          : "terminal",
    };
  else if (
    !t &&
    initial?.view &&
    ["files", "services", "terminal"].includes(initial.view.pane ?? "")
  ) {
    const pane = initial.view.pane!;
    tools = {
      version: 2,
      filesOpen: pane !== "services",
      servicesOpen: pane !== "files",
      lastSide: pane === "files" ? "files" : "services",
      compactPane: pane,
    };
  }
  const size = (
    value: number | undefined,
    fallback: number,
    min: number,
    max: number,
  ) =>
    typeof value === "number" && Number.isFinite(value) && value > 0
      ? clamp(value, min, max)
      : fallback;
  return {
    tools,
    resources: normalizeResourcePreferences(initial?.resources),
    layout: {
      fileWidth: size(
        initial?.layout?.fileWidth ?? initial?.resourceWidth,
        defaultWorkspaceSize.fileWidth,
        240,
        520,
      ),
      serviceWidth: size(
        initial?.layout?.serviceWidth ??
          (initial?.resourceWidth ? 320 : undefined),
        defaultWorkspaceSize.serviceWidth,
        320,
        480,
      ),
    },
  };
}
export type WorkspaceAction =
  | { type: "toggle-files"; visible: boolean }
  | { type: "toggle-service"; tool: ServiceTool; visible: boolean }
  | { type: "close-side"; side: ToolSide }
  | { type: "return-to-terminal" }
  | { type: "open-metric"; metric: ResourceMetric }
  | { type: "reveal-file" }
  | { type: "set-size"; side: keyof WorkspaceLayoutSize; width: number }
  | {
      type: "replace-resource-preferences";
      next:
        ResourcePreferences | ((p: ResourcePreferences) => ResourcePreferences);
    };
export function workspacePresentationReducer(
  state: WorkspacePresentationState,
  action: WorkspaceAction,
): WorkspacePresentationState {
  const open = (side: ToolSide, resources = state.resources) => ({
    ...state,
    resources,
    tools: {
      ...state.tools,
      [side === "files" ? "filesOpen" : "servicesOpen"]: true,
      lastSide: side,
      compactPane: side,
    },
  });
  const close = (side: ToolSide) => ({
    ...state,
    tools: {
      ...state.tools,
      [side === "files" ? "filesOpen" : "servicesOpen"]: false,
      compactPane:
        state.tools.compactPane === side
          ? ("terminal" as const)
          : state.tools.compactPane,
    },
  });
  switch (action.type) {
    case "toggle-files":
      return action.visible ? close("files") : open("files");
    case "toggle-service":
      return action.visible && state.resources.tab === action.tool
        ? close("services")
        : open("services", { ...state.resources, tab: action.tool });
    case "close-side":
      return close(action.side);
    case "return-to-terminal":
      return { ...state, tools: { ...state.tools, compactPane: "terminal" } };
    case "open-metric":
      return open("services", {
        ...state.resources,
        tab: "monitor",
        selectedMetric: action.metric,
        detailOpen: true,
      });
    case "reveal-file":
      return open("files");
    case "set-size": {
      if (!Number.isFinite(action.width) || action.width <= 0) return state;
      return {
        ...state,
        layout: {
          ...state.layout,
          [action.side]: clamp(
            action.width,
            action.side === "fileWidth" ? 240 : 320,
            action.side === "fileWidth" ? 520 : 480,
          ),
        },
      };
    }
    case "replace-resource-preferences":
      return {
        ...state,
        resources:
          typeof action.next === "function"
            ? action.next(state.resources)
            : action.next,
      };
  }
}
