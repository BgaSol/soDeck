import { useLayoutEffect, useRef, useState } from "react";
import {
  resolveWorkspaceLayout,
  workspaceMetrics,
  type WorkspaceLayoutSize,
} from "./workspaceSizing";
import type { WorkspaceToolState } from "./workspaceTools";
export function useWorkspaceLayout(
  tools: WorkspaceToolState,
  preferred: WorkspaceLayoutSize,
) {
  const root = useRef<HTMLElement>(null);
  const ruler = useRef<HTMLSpanElement>(null);
  const [measurement, setMeasurement] = useState({ width: 0, scale: 1 });
  useLayoutEffect(() => {
    const measure = () => {
      const width = root.current?.clientWidth ?? 0;
      const font = ruler.current?.getBoundingClientRect().width ?? 14;
      if (width > 0)
        setMeasurement((p) =>
          p.width === width && p.scale === font / 14
            ? p
            : { width, scale: font / 14 },
        );
    };
    const observer = new ResizeObserver(measure);
    if (root.current) observer.observe(root.current);
    if (ruler.current) observer.observe(ruler.current);
    measure();
    return () => observer.disconnect();
  }, []);
  const metrics = workspaceMetrics(measurement.scale);
  return {
    root,
    ruler,
    metrics,
    resolved: resolveWorkspaceLayout({
      width: measurement.width,
      metrics,
      tools,
      preferred,
    }),
  };
}
