import { Minus, Square, X } from "lucide-react";
export function WindowControls({ run }: { run(method: string, params?: unknown): Promise<unknown> }) {
  return <div className="window-controls" aria-label="窗口操作">
    <button aria-label="最小化" title="最小化" onClick={() => void run("windows.control", { action: "minimize" })}><Minus size={15} /></button>
    <button data-chrome-maximize aria-label="最大化或还原" title="最大化或还原" onClick={() => void run("windows.control", { action: "toggleMaximize" })}><Square size={12} /></button>
    <button className="window-close" aria-label="关闭窗口" title="关闭窗口" onClick={() => void run("windows.close")}><X size={17} /></button>
  </div>;
}
