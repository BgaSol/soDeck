import { useEffect, useRef, useState } from "react";
import { core, context, CoreError, errorMessage } from "@sodeck/sdk";
import type { Command, View } from "./types";
import { SurfaceQueue } from "./surface-queue";

const layouts = new SurfaceQueue(layout => core.call("workbench.surface", layout));

/** Only the native host creates plugin pages; this element supplies measured bounds. */
export function NativeSurface({ view, visible, commands = [] }: { view: View; visible: boolean; commands?: Command[] }) {
  const element = useRef<HTMLDivElement>(null);
  const [error, setError] = useState("");
  const [actionError, setActionError] = useState("");
  const [acting, setActing] = useState(false);
  const windowId = context().windowId ?? (context().detachedViewId ? `detached-${context().detachedViewId}` : "main");
  const failureReason = view.failureReason || "可重建显示界面，运行资源的状态请在核心资源中查看。重新加载不会自动连接服务器。";
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
        const value = { windowId, mountGeneration: view.mountGeneration, viewId: view.viewId, bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height }, visible: visible && !["loading", "ended", "viewFailed"].includes(view.status) && rect.width >= 1 && rect.height >= 1 };
        const encoded = JSON.stringify(value);
        if (encoded === previous) return;
        previous = encoded;
        void layouts.submit(value).then(() => { if (!disposed) setError(""); }).catch(e => { previous = ""; if (!disposed) setError(errorMessage(e)); });
      });
    };
    const observer = new ResizeObserver(measure); observer.observe(element.current);
    window.addEventListener("resize", measure); window.addEventListener("scroll", measure, true);
    measure();
    return () => { disposed = true; cancelAnimationFrame(frame); observer.disconnect(); window.removeEventListener("resize", measure); window.removeEventListener("scroll", measure, true); void layouts.submit({ windowId, mountGeneration: view.mountGeneration, viewId: view.viewId, bounds: { x: 0, y: 0, width: 0, height: 0 }, visible: false }).catch(() => {}); };
  }, [windowId, view.viewId, view.mountGeneration, view.status, visible]);
  return <div ref={element} className="native-surface" data-view-id={view.viewId} data-testid="native-surface">{view.status === "loading" ? <div className="surface-error" role="status">正在准备视图…</div> : view.status === "ended" ? <div className="surface-error"><strong>会话已结束</strong><p>工作台保留了这个位置。</p>{commands.filter(command => view.actions?.includes(command.id) && !command.input).map(command => <button key={command.id} disabled={acting} onClick={() => void act("commands.execute", { commandId: command.id, workspaceId: view.workspaceId, viewId: view.viewId, operationId: crypto.randomUUID() }, 130_000)}>{command.title}</button>)}{(actionError || error) && <p role="alert">{actionError || error}</p>}</div> : (error || actionError || view.status === "viewFailed") && <div className="surface-error" role="alert"><strong>视图暂时不可用</strong><p>{error || failureReason}</p>{actionError && <p>{actionError}</p>}<button disabled={acting} onClick={() => void act("views.recover", { viewId: view.viewId })}>重新加载视图</button></div>}</div>;
}
