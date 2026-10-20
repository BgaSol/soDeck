export interface WorkspaceLayoutSize {
  fileWidth: number;
  serviceWidth: number;
}
export type WorkspacePane = "files" | "terminal" | "services";
export interface WorkspaceViewState {
  expanded?: boolean; // Legacy snapshots only.
  pane: WorkspacePane;
}
export const defaultWorkspaceView: WorkspaceViewState = {
  pane: "terminal",
};
export const defaultWorkspaceSize: WorkspaceLayoutSize = {
  fileWidth: 280,
  serviceWidth: 360,
};
export function normalizeWorkspaceView(
  value?: Partial<WorkspaceViewState>,
): WorkspaceViewState {
  return {
    pane:
      value?.pane === "files" || value?.pane === "services"
        ? value.pane
        : "terminal",
  };
}
const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(max, value));
export function fitWorkspaceLayout(
  width: number,
  preferred?: WorkspaceLayoutSize,
) {
  const mode = width >= 1200 ? "three" : width >= 760 ? "two" : "single";
  const terminalMin = Math.min(320, width);
  const fileMin = Math.min(280, width);
  const serviceMin = Math.min(320, width);
  const budget = Math.max(
    0,
    width - terminalMin - (mode === "three" ? 20 : 10),
  );
  let fileWidth = clamp(
    preferred?.fileWidth || defaultWorkspaceSize.fileWidth,
    fileMin,
    520,
  );
  let serviceWidth = clamp(preferred?.serviceWidth || 360, serviceMin, 480);
  if (mode === "three") {
    fileWidth = Math.max(
      fileMin,
      fileWidth - Math.max(0, fileWidth + serviceWidth - budget),
    );
    serviceWidth = Math.max(
      serviceMin,
      Math.min(serviceWidth, budget - fileWidth),
    );
  } else if (mode === "two") {
    fileWidth = Math.min(fileWidth, budget);
    serviceWidth = Math.min(serviceWidth, budget);
  }
  return {
    fileWidth,
    serviceWidth,
    fileMin,
    serviceMin,
    terminalMin,
    budget,
    mode,
  };
}
export function workspaceVisibility(
  mode: ReturnType<typeof fitWorkspaceLayout>["mode"],
  view: WorkspaceViewState,
) {
  if (mode === "three") return { files: true, terminal: true, services: true };
  if (mode === "two")
    return {
      files: view.pane === "files",
      terminal: true,
      services: view.pane !== "files",
    };
  return {
    files: view.pane === "files",
    terminal: view.pane === "terminal",
    services: view.pane === "services",
  };
}
export type WorkspaceVisibility = ReturnType<typeof workspaceVisibility>;
