import { useState, type ReactNode, type Ref } from "react";
import { Minus, Square, Copy, X } from "lucide-react";
import { useDesktopWindow } from "./useDesktopWindow";
import { ThemeSwitcher } from "./ThemeSwitcher";
import { SystemSettingsButton } from "./SystemSettingsDialog";
import { errorOf } from "./types";
import "./window-chrome.css";

function WindowControls({
  maximized,
  run,
}: Pick<ReturnType<typeof useDesktopWindow>, "maximized" | "run">) {
  return (
    <div className="window-controls" role="group" aria-label="窗口控制">
      <button
        aria-label="最小化窗口"
        title="最小化"
        onClick={() => void run("minimize")}
      >
        <Minus aria-hidden="true" size={14} />
      </button>
      <button
        aria-label={maximized ? "还原窗口" : "最大化窗口"}
        title={maximized ? "还原" : "最大化"}
        onClick={() => void run("toggleMaximize")}
      >
        {maximized ? (
          <Copy aria-hidden="true" size={12} />
        ) : (
          <Square aria-hidden="true" size={12} />
        )}
      </button>
      <button
        className="window-close"
        aria-label="关闭窗口"
        title="关闭窗口"
        onClick={() => void run("close")}
      >
        <X aria-hidden="true" size={16} />
      </button>
    </div>
  );
}

export function WindowTitlebar({
  leading,
  title,
  subtitle,
  tabs,
  actions,
  dragRegionRef,
  appendDropActive = false,
  tone = "default",
  onError,
}: {
  leading?: ReactNode;
  title?: string;
  subtitle?: string;
  tabs?: ReactNode;
  actions?: ReactNode;
  dragRegionRef?: Ref<HTMLDivElement>;
  appendDropActive?: boolean;
  tone?: "default" | "terminal";
  onError?: (error: unknown) => void;
}) {
  const [error, setError] = useState("");
  const window = useDesktopWindow(
    onError ?? ((e) => setError(errorOf(e).message)),
  );
  const draggable = window.chrome === "custom" || window.chrome === "overlay";
  return (
    <>
      <header
        className={`window-titlebar window-titlebar-${tone}${tabs ? " window-titlebar-tabs" : ""}`}
        data-chrome={window.chrome}
        data-focused={window.focused}
        data-maximized={window.maximized}
        data-fullscreen={window.fullscreen}
      >
        {leading && <div className="window-leading">{leading}</div>}
        {tabs && <div className="window-tabs-slot">{tabs}</div>}
        <div
          ref={dragRegionRef}
          className="window-drag-region"
          data-tab-drop={appendDropActive || undefined}
          data-tauri-drag-region={draggable || undefined}
          title={subtitle || title}
        >
          {appendDropActive ? (
            <span className="tab-drop-hint">松开以放入会话</span>
          ) : (
            <>
              {!tabs && title && <h1>{title}</h1>}
              {!tabs && subtitle && <p>{subtitle}</p>}
            </>
          )}
        </div>
        <div className="window-actions">
          {actions}
          <ThemeSwitcher />
          <SystemSettingsButton />
        </div>
        {window.chrome === "custom" && (
          <WindowControls maximized={window.maximized} run={window.run} />
        )}
      </header>
      {error && (
        <div className="connection-error" role="alert">
          <span>{error}</span>
          <button
            className="icon-button"
            aria-label="关闭窗口错误提示"
            onClick={() => setError("")}
          >
            <X size={14} />
          </button>
        </div>
      )}
    </>
  );
}

export function WindowLoading({ title }: { title: string }) {
  return (
    <div className="window-loading">
      <WindowTitlebar title={title} />
      <p role="status">正在打开{title}…</p>
    </div>
  );
}
