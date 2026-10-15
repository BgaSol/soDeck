export interface WorkspaceLayoutSize {
  fileWidth: number;
  serviceWidth: number;
}

// Always reserve a usable terminal, including in the 650px detached window.
export function fitWorkspaceLayout(
  width: number,
  preferred?: WorkspaceLayoutSize,
) {
  const available = Math.max(0, width - 20);
  const terminalMin = Math.min(280, available * 0.4);
  const fileMin = Math.min(180, available * 0.28);
  const serviceMin = Math.min(200, available * 0.3);
  const budget = available - terminalMin;
  let fileWidth = Math.max(
    fileMin,
    Math.min(520, preferred?.fileWidth || available * 0.23),
  );
  let serviceWidth = Math.max(
    serviceMin,
    Math.min(560, preferred?.serviceWidth || available * 0.26),
  );
  const excess = fileWidth + serviceWidth - budget;
  if (excess > 0) {
    const room = fileWidth - fileMin + serviceWidth - serviceMin;
    fileWidth -= (excess * (fileWidth - fileMin)) / Math.max(1, room);
    serviceWidth = budget - fileWidth;
  }
  return { fileWidth, serviceWidth, fileMin, serviceMin, budget };
}
