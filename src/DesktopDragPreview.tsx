import { useState } from "react";
import { createPortal } from "react-dom";
import { Copy, Move } from "lucide-react";
import { useDesktopDrag } from "./nativeDrag";
import type { DesktopDragEvent } from "./nativeDrag";

export function DesktopDragPreview() {
  const [drag, setDrag] = useState<DesktopDragEvent | null>(null);
  useDesktopDrag((event) =>
    setDrag(
      event.phase === "over" && event.kind !== "localFiles" ? event : null,
    ),
  );
  if (!drag) return null;
  const Icon = drag.kind === "remoteFiles" ? Copy : Move;
  return createPortal(
    <div
      className="native-drag-preview"
      aria-hidden="true"
      style={{
        left: Math.max(0, Math.min(drag.x + 14, window.innerWidth - 220)),
        top: Math.max(0, Math.min(drag.y + 14, window.innerHeight - 40)),
      }}
    >
      <Icon size={14} />
      <span>{drag.label}</span>
    </div>,
    document.body,
  );
}
