import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import {
  fitWorkspaceLayout,
  type WorkspaceLayoutSize,
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
}: {
  files: ReactNode;
  terminal: ReactNode;
  services: ReactNode;
  value?: WorkspaceLayoutSize;
  onChange?: (value: WorkspaceLayoutSize) => void;
  id?: string;
  labelledBy?: string;
  active?: boolean;
}) {
  const page = useRef<HTMLElement>(null);
  const [width, setWidth] = useState(0);
  const drag = useRef<{
    side: "fileWidth" | "serviceWidth";
    x: number;
    width: number;
  } | null>(null);
  useLayoutEffect(() => {
    const measure = () => {
      const next = page.current?.clientWidth;
      if (next) setWidth(next); // Hidden tabs retain their last useful dimensions.
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(page.current!);
    return () => observer.disconnect();
  }, []);
  const fitted = fitWorkspaceLayout(width, value);
  const { fileWidth, serviceWidth, fileMin, serviceMin, budget } = fitted;
  useEffect(() => {
    if (width > 0 && !value) onChange?.({ fileWidth, serviceWidth });
  }, [width, value, onChange, fileWidth, serviceWidth]);
  function change(side: "fileWidth" | "serviceWidth", next: number) {
    const other = side === "fileWidth" ? serviceWidth : fileWidth;
    const min = side === "fileWidth" ? fileMin : serviceMin;
    const max = Math.min(side === "fileWidth" ? 520 : 560, budget - other);
    onChange?.({
      fileWidth,
      serviceWidth,
      [side]: Math.max(min, Math.min(max, next)),
    });
  }
  function separator(side: "fileWidth" | "serviceWidth") {
    const left = side === "fileWidth";
    return (
      <div
        className="vertical-resizer"
        role="separator"
        tabIndex={onChange ? 0 : -1}
        aria-disabled={!onChange}
        aria-label={left ? "调整文件区域宽度" : "调整服务区域宽度"}
        aria-orientation="vertical"
        aria-valuemin={Math.round(left ? fileMin : serviceMin)}
        aria-valuemax={Math.round(
          Math.min(
            left ? 520 : 560,
            budget - (left ? serviceWidth : fileWidth),
          ),
        )}
        aria-valuenow={Math.round(fitted[side])}
        onPointerDown={(event) => {
          if (!onChange || event.button !== 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          drag.current = { side, x: event.clientX, width: fitted[side] };
        }}
        onPointerMove={(event) => {
          const start = drag.current;
          if (start?.side === side)
            change(
              side,
              start.width + (event.clientX - start.x) * (left ? 1 : -1),
            );
        }}
        onPointerUp={(event) => {
          drag.current = null;
          if (event.currentTarget.hasPointerCapture(event.pointerId))
            event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
        onLostPointerCapture={() => {
          drag.current = null;
        }}
        onKeyDown={(event) => {
          if (!onChange || !["ArrowLeft", "ArrowRight"].includes(event.key))
            return;
          event.preventDefault();
          change(
            side,
            fitted[side] +
              (event.key === "ArrowRight" ? 10 : -10) * (left ? 1 : -1),
          );
        }}
      />
    );
  }
  return (
    <section
      ref={page}
      className={`session-page${onChange ? "" : " empty-session"}`}
      role={id ? "tabpanel" : undefined}
      id={id}
      aria-labelledby={labelledBy}
      hidden={!active}
      style={
        width
          ? ({
              "--file-width": `${fileWidth}px`,
              "--service-width": `${serviceWidth}px`,
            } as CSSProperties)
          : undefined
      }
    >
      {files}
      {separator("fileWidth")}
      {terminal}
      {separator("serviceWidth")}
      {services}
    </section>
  );
}
