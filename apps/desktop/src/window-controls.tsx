import {WindowControlButton} from '@sodeck/ui';
import { Minus, Square, X } from "lucide-react";
export function WindowControls({ run }: { run(method: string, params?: unknown): Promise<unknown> }) {
  return <div className="window-controls" aria-label="窗口操作">
    <WindowControlButton label="最小化" onClick={() => void run("windows.control", { action: "minimize" })}><Minus size={15} /></WindowControlButton>
    <WindowControlButton data-chrome-maximize label="最大化或还原" onClick={() => void run("windows.control", { action: "toggleMaximize" })}><Square size={12} /></WindowControlButton>
    <WindowControlButton close label="关闭窗口" onClick={() => void run("windows.close")}><X size={17} /></WindowControlButton>
  </div>;
}
