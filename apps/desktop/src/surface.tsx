import { useEffect, useRef, useState } from "react";
import { core, CoreError, errorMessage } from "@sodeck/sdk";
import type { View } from "./types";
import { SurfaceQueue } from "./surface-queue";

const layouts = new SurfaceQueue(layout => core.call("workbench.surface", layout));

/** Only the native host creates plugin pages; this element supplies measured bounds. */
export function NativeSurface({ view, visible }: { view: View; visible: boolean }) {
  const element = useRef<HTMLDivElement>(null);
  const [error, setError] = useState("");
  const [actionError, setActionError] = useState("");
  const [acting, setActing] = useState(false);
  const hasPty = "ptyId" in view && typeof view.ptyId === "string" && view.ptyId.length > 0;
  const failureReason = view.failureReason || (hasPty ? "显示进程已结束，可尝试重建视图。连接状态请在核心资源中查看；已丢失的屏幕内容无法恢复。" : "此位置没有恢复运行会话。可检查插件状态后重试加载；重建界面不会自动连接服务器。");
  useEffect(() => { setActionError(""); }, [view.viewId]);
  const act = async (method: string, params: unknown, timeout?: number) => {
    if (acting) return; setActing(true); setActionError("");
    try { await core.call(method, params, timeout); }
    catch (e) { setActionError(e instanceof CoreError ? `${e.code}：${errorMessage(e)}` : errorMessage(e)); }
    finally { setActing(false); }
  };
  useEffect(() => {
    if (!element.current) return;
    let frame = 0, previous = "", disposed = false;
    const measure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (disposed || !element.current) return;
        const rect = element.current.getBoundingClientRect();
        const value = { viewId: view.viewId, bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height }, visible: visible && !["ended", "viewFailed"].includes(view.status) && rect.width >= 1 && rect.height >= 1 };
        const encoded = JSON.stringify(value);
        if (encoded === previous) return;
        previous = encoded;
        void layouts.submit(value).then(() => { if (!disposed) setError(""); }).catch(e => { previous = ""; if (!disposed) setError(errorMessage(e)); });
      });
    };
    const observer = new ResizeObserver(measure); observer.observe(element.current);
    window.addEventListener("resize", measure); window.addEventListener("scroll", measure, true);
    measure();
    return () => { disposed = true; cancelAnimationFrame(frame); observer.disconnect(); window.removeEventListener("resize", measure); window.removeEventListener("scroll", measure, true); void layouts.submit({ viewId: view.viewId, bounds: { x: 0, y: 0, width: 0, height: 0 }, visible: false }).catch(() => {}); };
  }, [view.viewId, view.mountGeneration, view.status, visible]);
  return <div ref={element} className="native-surface" data-view-id={view.viewId} data-testid="native-surface">{view.status === "ended" ? <div className="surface-error"><strong>会话已结束</strong><p>工作台保留了这个位置。新建终端会建立新的 SSH 会话。</p><button disabled={acting} onClick={() => void act("workbench.openTerminal", { serverId: view.serverId }, 130_000)}>新建终端</button>{(actionError || error) && <p role="alert">{actionError || error}</p>}</div> : (error || actionError || view.status === "viewFailed") && <div className="surface-error" role="alert"><strong>视图暂时不可用</strong><p>{error || failureReason}</p>{actionError && <p>{actionError}</p>}<button disabled={acting} onClick={() => void act("workbench.recover", { viewId: view.viewId })}>重新加载视图</button></div>}</div>;
}
