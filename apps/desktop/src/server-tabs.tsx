import {TabItem} from '@sodeck/ui';
import {Button} from '@sodeck/ui';
import { useEffect, useRef, useState } from "react";
import type { PointerEvent } from "react";
import { ChevronDown, ChevronLeft, ChevronRight, Plus, Server as ServerIcon, X } from "lucide-react";
import { onCoreNotification,type HomePage } from "@sodeck/sdk";
import { useOverflow } from "./use-overflow";
import type { Server, Workbench } from "./types";

export function ServerTabs({ workbench, servers, run, onPicker, onMenu, onList, onDragging,home,onHomeClose }: {
  home:HomePage;onHomeClose():void;
  workbench: Workbench; servers: Server[]; run(method: string, params?: unknown): Promise<unknown>;
  onList():void; onPicker(): void; onMenu(id: string, x: number, y: number): void; onDragging(value: boolean): void;
}) {
  const row = useRef<HTMLDivElement>(null);
  const overflow=useOverflow(row, workbench.workspaceIds.join(",")+servers.map(s=>s.name).join(","));
  const candidate = useRef<{ id: string; x: number; y: number; offset: number; offsetY:number; operationId?:string } | null>(null);
  const suppressed = useRef<string | null>(null);
  const [drag, setDrag] = useState<{ id: string; x: number; y:number; offset: number; offsetY:number } | null>(null);
  const [destination,setDestination]=useState("");
  const [indicator, setIndicator] = useState<number | null>(null);
  useEffect(() => onCoreNotification(n => { if (n.type === "workbench.dragPreview") { const p = n.payload as { index?: number; windowId?: string; newWindow?:boolean } | null; setIndicator(p?.windowId === workbench.windowId ? p.index ?? null : null); setDestination(p?.newWindow?"移到新窗口":p?.newWindow===false?"此处不能放置":""); } }), [workbench.windowId]);
  useEffect(() => { document.getElementById(`server-tab-${workbench.activeWorkspaceId}`)?.scrollIntoView({ block: "nearest", inline: "nearest" }); }, [workbench.activeWorkspaceId]);
  const begin = (event: PointerEvent, id: string) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
    const rect=event.currentTarget.getBoundingClientRect();
    suppressed.current = null; candidate.current = { id, x: event.clientX, y: event.clientY, offset: event.clientX - rect.left,offsetY:event.clientY-rect.top };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const move = (event: PointerEvent) => {
    const current = candidate.current; if (!current) return;
    if (!drag && Math.hypot(event.clientX - current.x, event.clientY - current.y) < 8) return;
    setDrag({ id: current.id, x: event.clientX,y:event.clientY, offset: current.offset,offsetY:current.offsetY });
    if (suppressed.current === current.id) return;
    suppressed.current = current.id; onDragging(true);
    current.operationId=crypto.randomUUID();
    void run("workspaces.drag", { workspaceId: current.id, operationId: current.operationId }).finally(() => { candidate.current = null; setDrag(null); onDragging(false); });
  };
  const navigateKey=(event:import('react').KeyboardEvent,id:string)=>{
    if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;
    event.preventDefault();const ids=[...(home.open?['@home']:[]),...workbench.workspaceIds],index=ids.indexOf(id);
    const next=ids[event.key==='Home'?0:event.key==='End'?ids.length-1:(index+(event.key==='ArrowRight'?1:-1)+ids.length)%ids.length];
    document.getElementById(`server-tab-${next}`)?.focus();if(next==='@home')onPicker();else void run('workspaces.activate',{workspaceId:next});
  };
  return <><div ref={row} className="server-tabs" data-server-tabs role="tablist" aria-label="工作台页面">
    {home.open&&<TabItem id="server-tab-@home" className="server-tab" selected={home.selected} tabIndex={home.selected?0:-1} aria-label="新建连接" onClick={onPicker} onKeyDown={e=>{if(e.target!==e.currentTarget)return;navigateKey(e,'@home');if(e.key==='Enter'||e.key===' '){e.preventDefault();onPicker();}if(e.key==='Delete'){e.preventDefault();onHomeClose();}}}><Plus size={14}/><span>新建连接</span><Button variant="quiet" size="compact" className="icon-button" aria-label="关闭新建连接页" onClick={e=>{e.stopPropagation();onHomeClose();}}><X size={13}/></Button></TabItem>}
    {workbench.workspaceIds.map((id, index) => {
      const server = servers.find(s => s.id === id), active = workbench.activeWorkspaceId === id;
      const name = server?.name ?? "服务器不可用";
      const address = server ? `${server.username}@${server.host}:${server.port}` : id;
      return <TabItem key={id} id={`server-tab-${id}`} data-workspace-id={id} className={`server-tab ${active ? "active" : ""} ${indicator === index ? "insert-before" : ""}`} role="tab" tabIndex={active ? 0 : -1} selected={active} aria-label={name} title={`${name}\n${address}`} onPointerDown={event => begin(event, id)} onPointerMove={move} onPointerCancel={() => { const id=candidate.current?.operationId;candidate.current = null;if(id)void run("workbench.operation.cancel",{operationId:id}); }} onClick={() => { if (suppressed.current === id) { suppressed.current = null; return; } void run("workspaces.activate", { workspaceId: id }); }} onContextMenu={event => { event.preventDefault(); onMenu(id, event.clientX, event.clientY); }} onKeyDown={event => {
        if (event.target !== event.currentTarget) return;
        if (event.key === "Delete") { event.preventDefault(); void run("workspaces.close", { workspaceId: id }); }
        if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
          navigateKey(event,id);
        }
        if (event.key === "Enter" || event.key === " ") { event.preventDefault(); void run("workspaces.activate", { workspaceId: id }); }
        if ((event.key === "F10" && event.shiftKey)||event.key === "ContextMenu") { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); onMenu(id, rect.left, rect.bottom); }
      }}><ServerIcon size={14} /><span>{name}</span><Button variant="quiet" size="compact" className="icon-button" aria-label={`关闭服务器工作区 ${name}`} title="关闭工作区并结束其会话" onClick={event => { event.stopPropagation(); void run("workspaces.close", { workspaceId: id }); }}><X size={13} /></Button></TabItem>;
    })}{indicator === workbench.workspaceIds.length && <i className="insert-end" />}
  </div><Button variant="quiet" size="compact" className="icon-button open-server" aria-label="打开服务器" title="打开服务器 (Ctrl+O)" onClick={onPicker}><Plus size={18} /></Button>
  {overflow && <><Button variant="quiet" size="compact" className="icon-button tab-scroll-button" aria-label="向左滚动服务器页" onClick={()=>row.current?.scrollBy({left:-260})}><ChevronLeft size={14}/></Button><Button variant="quiet" size="compact" className="icon-button tab-scroll-button" aria-label="向右滚动服务器页" onClick={()=>row.current?.scrollBy({left:260})}><ChevronRight size={14}/></Button><Button variant="quiet" size="compact" className="icon-button" aria-label="已打开服务器" onClick={onList}><ChevronDown size={14}/></Button></>}
  {drag && <div className="server-drag-preview" style={{ left: drag.x - drag.offset, top: drag.y-drag.offsetY }}>{servers.find(s => s.id === drag.id)?.name}{destination && <span role="status"> · {destination}</span>}</div>}</>;
}
