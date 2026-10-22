import {
  useEffect,
  useRef,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import { ArrowLeft } from "lucide-react";
import { WorkspaceToolRail } from "./WorkspaceToolRail";
import {
  clamp,
  defaultWorkspaceSize,
  type WorkspaceLayoutSize,
  type WorkspaceMetrics,
  type ResolvedWorkspaceLayout,
} from "./workspaceSizing";
import type { ServiceTool, ToolSide } from "./workspaceTools";
export function WorkspaceLayout({
  files,
  terminal,
  services,
  id,
  labelledBy,
  active,
  root,
  ruler,
  resolved,
  metrics,
  selected,
  onFiles,
  onService,
  onClose,
  onReturn,
  onResize,
}: {
  files: ReactNode;
  terminal: ReactNode;
  services: ReactNode;
  id: string;
  labelledBy: string;
  active: boolean;
  root: RefObject<HTMLElement | null>;
  ruler: RefObject<HTMLSpanElement | null>;
  resolved: ResolvedWorkspaceLayout;
  metrics: WorkspaceMetrics;
  selected: ServiceTool;
  onFiles: () => void;
  onService: (tool: ServiceTool) => void;
  onClose: (side: ToolSide) => void;
  onReturn: () => void;
  onResize: (side: keyof WorkspaceLayoutSize, width: number) => void;
}) {
  const drag = useRef<{
    side: keyof WorkspaceLayoutSize;
    x: number;
    width: number;
    min: number;
    max: number;
  } | null>(null);
  useEffect(() => {
    if (!active) drag.current = null;
  }, [active]);
  const { visible } = resolved;
  function separator(side: keyof WorkspaceLayoutSize) {
    const left = side === "fileWidth",
      panel = left ? resolved.files : resolved.services,
      bounds = resolved.resizeBounds[side];
    function change(width: number) {
      if (bounds)
        onResize(side, clamp(width, bounds.min, bounds.max) / metrics.scale);
    }
    return (
      <div
        className={`vertical-resizer ${left ? "file-resizer" : "service-resizer"}`}
        hidden={!bounds}
        role="separator"
        tabIndex={bounds ? 0 : -1}
        aria-label={left ? "调整文件区域宽度" : "调整服务区域宽度"}
        aria-orientation="vertical"
        aria-valuemin={Math.round(bounds?.min ?? 0)}
        aria-valuemax={Math.round(bounds?.max ?? 0)}
        aria-valuenow={Math.round(panel.width)}
        onDoubleClick={() => change(defaultWorkspaceSize[side] * metrics.scale)}
        onPointerDown={(e) => {
          if (!bounds || e.button !== 0) return;
          e.preventDefault();
          e.currentTarget.setPointerCapture(e.pointerId);
          drag.current = { side, x: e.clientX, width: panel.width, ...bounds };
        }}
        onPointerMove={(e) => {
          const start = drag.current;
          if (start?.side === side)
            onResize(
              side,
              clamp(
                start.width + (e.clientX - start.x) * (left ? 1 : -1),
                start.min,
                start.max,
              ) / metrics.scale,
            );
        }}
        onPointerUp={(e) => {
          drag.current = null;
          if (e.currentTarget.hasPointerCapture(e.pointerId))
            e.currentTarget.releasePointerCapture(e.pointerId);
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
        onLostPointerCapture={() => {
          drag.current = null;
        }}
        onKeyDown={(e) => {
          if (!["ArrowLeft", "ArrowRight"].includes(e.key)) return;
          e.preventDefault();
          change(
            panel.width +
              (e.key === "ArrowRight" ? 10 : -10) *
                (left ? 1 : -1) *
                metrics.scale,
          );
        }}
      />
    );
  }
  function escape(side: ToolSide, e: React.KeyboardEvent) {
    if (
      resolved[side].placement === "overlay" &&
      e.key === "Escape" &&
      !e.defaultPrevented &&
      !document.querySelector('dialog[open], [role="menu"], details[open]')
    ) {
      e.preventDefault();
      onClose(side);
    }
  }
  return (
    <section
      ref={root}
      className="session-page responsive-workspace"
      role="tabpanel"
      id={id}
      aria-labelledby={labelledBy}
      hidden={!active}
      data-layout={resolved.mode}
      style={
        {
          "--workspace-rail-width": `${metrics.rail}px`,
          "--workspace-separator-width": `${metrics.separator}px`,
          "--file-width": `${resolved.files.width}px`,
          "--service-width": `${resolved.services.width}px`,
          "--file-column":
            resolved.files.placement === "docked"
              ? `${resolved.files.width}px`
              : "0px",
          "--service-column":
            resolved.services.placement === "docked"
              ? `${resolved.services.width}px`
              : "0px",
          "--file-separator": resolved.resizeBounds.fileWidth
            ? `${metrics.separator}px`
            : "0px",
          "--service-separator": resolved.resizeBounds.serviceWidth
            ? `${metrics.separator}px`
            : "0px",
        } as CSSProperties
      }
    >
      <span ref={ruler} className="workspace-rem-ruler" aria-hidden="true" />
      <div className="workspace-columns">
        <WorkspaceToolRail
          side="left"
          id={id}
          filesOpen={visible.files}
          servicesOpen={visible.services}
          selected={selected}
          onFiles={onFiles}
          onService={onService}
        />
        <div
          id={`${id}-files`}
          className="workspace-slot workspace-files"
          data-placement={resolved.files.placement}
          hidden={!visible.files}
          inert={!visible.files}
          onKeyDown={(e) => escape("files", e)}
        >
          {resolved.mode === "single" && (
            <button className="workspace-return" onClick={onReturn}>
              <ArrowLeft size={14} />
              返回终端
            </button>
          )}
          {files}
        </div>
        {separator("fileWidth")}
        <div
          className="workspace-slot workspace-terminal"
          hidden={!visible.terminal}
          inert={!visible.terminal}
        >
          {terminal}
        </div>
        {separator("serviceWidth")}
        <div
          id={`${id}-services`}
          className="workspace-slot workspace-services"
          data-placement={resolved.services.placement}
          hidden={!visible.services}
          inert={!visible.services}
          onKeyDown={(e) => escape("services", e)}
        >
          {resolved.mode === "single" && (
            <button className="workspace-return" onClick={onReturn}>
              <ArrowLeft size={14} />
              返回终端
            </button>
          )}
          {services}
        </div>
        <WorkspaceToolRail
          side="right"
          id={id}
          filesOpen={visible.files}
          servicesOpen={visible.services}
          selected={selected}
          onFiles={onFiles}
          onService={onService}
        />
      </div>
    </section>
  );
}
