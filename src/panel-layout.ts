export type PanelSide = "left" | "right";
export interface PanelWidths {
  leftWidth: number;
  rightWidth: number;
  drag?: { side: PanelSide; otherWidth: number };
}
export interface PanelGeometry { left: boolean; right: boolean; leftWidth: number; rightWidth: number }
export function panelGeometry(room: number, widths: PanelWidths, left: boolean, right: boolean, priority: PanelSide): PanelGeometry {
  room = Math.max(0, Math.floor(room));
  if (room < 160) left = right = false;
  else if (left && right && room < 320) { left = priority === "left"; right = !left; }
  const clamp = (value: number, max: number) => Math.max(160, Math.min(480, max, Math.round(value)));
  let leftWidth = left ? clamp(widths.leftWidth, room) : 0;
  let rightWidth = right ? clamp(widths.rightWidth, room) : 0;
  if (left && right) {
    if (widths.drag?.side === "left") { rightWidth = clamp(widths.drag.otherWidth, room - 160); leftWidth = clamp(widths.leftWidth, room - rightWidth); }
    else if (widths.drag?.side === "right") { leftWidth = clamp(widths.drag.otherWidth, room - 160); rightWidth = clamp(widths.rightWidth, room - leftWidth); }
    else if (priority === "left") { leftWidth = clamp(widths.leftWidth, room - 160); rightWidth = clamp(widths.rightWidth, room - leftWidth); }
    else { rightWidth = clamp(widths.rightWidth, room - 160); leftWidth = clamp(widths.leftWidth, room - rightWidth); }
  }
  return { left, right, leftWidth, rightWidth };
}
