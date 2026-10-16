import { useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { Folder, PanelRight, TerminalSquare } from "lucide-react";
import {
  defaultWorkspaceSize,
  defaultWorkspaceView,
  fitWorkspaceLayout,
  workspaceVisibility,
  type WorkspaceLayoutSize,
  type WorkspaceViewState,
  type WorkspaceVisibility,
} from "./workspaceSizing";

export function WorkspaceLayout({
  files,
  terminal,
  services,
  value,
  onChange,
  id,
  labelledBy,
  active = true,
  view = defaultWorkspaceView,
  onViewChange,
  onVisibilityChange,
}: {
  files: ReactNode;
  terminal: ReactNode;
  services: ReactNode;
  value?: WorkspaceLayoutSize;
  onChange?: (value: WorkspaceLayoutSize) => void;
  id?: string;
  labelledBy?: string;
  active?: boolean;
  view?: WorkspaceViewState;
  onViewChange?: (view: WorkspaceViewState) => void;
  onVisibilityChange?: (visible: WorkspaceVisibility) => void;
}) {
  const page = useRef<HTMLElement>(null);
  const [width, setWidth] = useState(0);
  const drag = useRef<{
    side: keyof WorkspaceLayoutSize;
    x: number;
    width: number;
  } | null>(null);
  useLayoutEffect(() => {
    const measure = () => {
      const next = page.current?.clientWidth;
      if (next) setWidth(next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(page.current!);
    return () => observer.disconnect();
  }, []);
  const fitted = fitWorkspaceLayout(width || 1440, value);
  const { fileWidth, serviceWidth, fileMin, serviceMin, budget, mode } = fitted;
  const visible = workspaceVisibility(mode, view);
  useLayoutEffect(() => {
    onVisibilityChange?.({
      files: visible.files,
      terminal: visible.terminal,
      services: visible.services,
    });
  }, [onVisibilityChange, visible.files, visible.terminal, visible.services]);
  function change(side: keyof WorkspaceLayoutSize, next: number) {
    const other =
      mode === "three" ? (side === "fileWidth" ? serviceWidth : fileWidth) : 0;
    const min = side === "fileWidth" ? fileMin : serviceMin;
    const max = Math.min(side === "fileWidth" ? 520 : 480, budget - other);
    onChange?.({
      ...(value ?? defaultWorkspaceSize),
      [side]: Math.max(min, Math.min(max, next)),
    });
  }
  function separator(side: keyof WorkspaceLayoutSize, shown: boolean) {
    const left = side === "fileWidth";
    return (
      <div
        className={`vertical-resizer ${left ? "file-resizer" : "service-resizer"}`}
        hidden={!shown}
        role="separator"
        tabIndex={onChange && shown ? 0 : -1}
        aria-disabled={!onChange}
        aria-label={left ? "调整文件区域宽度" : "调整服务区域宽度"}
        aria-orientation="vertical"
        aria-valuemin={Math.round(left ? fileMin : serviceMin)}
        aria-valuemax={Math.round(
          Math.min(
            left ? 520 : 480,
            budget - (mode === "three" ? (left ? serviceWidth : fileWidth) : 0),
          ),
        )}
        aria-valuenow={Math.round(fitted[side])}
        onDoubleClick={() => change(side, defaultWorkspaceSize[side])}
        onPointerDown={(e) => {
          if (!onChange || e.button !== 0) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          drag.current = { side, x: e.clientX, width: fitted[side] };
        }}
        onPointerMove={(e) => {
          const start = drag.current;
          if (start?.side === side)
            change(side, start.width + (e.clientX - start.x) * (left ? 1 : -1));
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
          if (!onChange || !["ArrowLeft", "ArrowRight"].includes(e.key)) return;
          e.preventDefault();
          change(
            side,
            fitted[side] +
              (e.key === "ArrowRight" ? 10 : -10) * (left ? 1 : -1),
          );
        }}
      />
    );
  }
  return (
    <section
      ref={page}
      className={`session-page responsive-workspace${onChange ? "" : " empty-session"}`}
      role={id ? "tabpanel" : undefined}
      id={id}
      aria-labelledby={labelledBy}
      hidden={!active}
      data-layout={mode}
      data-pane={view.pane}
      style={
        {
          "--file-width": `${fileWidth}px`,
          "--service-width": `${serviceWidth}px`,
        } as CSSProperties
      }
    >
      <nav
        className="workspace-pane-switcher"
        aria-label="工作区域"
        hidden={!onViewChange || mode === "three"}
      >
        {(
          [
            ["files", "文件", Folder],
            ["terminal", "终端", TerminalSquare],
            ["services", "监控与 Docker", PanelRight],
          ] as const
        ).map(([pane, label, Icon]) => (
          <button
            type="button"
            key={pane}
            aria-pressed={view.pane === pane}
            onClick={() => onViewChange?.({ pane })}
          >
            <Icon size={14} />
            {label}
          </button>
        ))}
      </nav>
      <div className="workspace-columns">
        <div
          className="workspace-slot workspace-files"
          hidden={!visible.files}
          inert={!visible.files}
        >
          {files}
        </div>
        {separator("fileWidth", mode !== "single" && visible.files)}
        <div
          className="workspace-slot workspace-terminal"
          hidden={!visible.terminal}
          inert={!visible.terminal}
        >
          {terminal}
        </div>
        {separator("serviceWidth", mode !== "single" && visible.services)}
        <div
          className="workspace-slot workspace-services"
          hidden={!visible.services}
          inert={!visible.services}
        >
          {services}
        </div>
      </div>
    </section>
  );
}
