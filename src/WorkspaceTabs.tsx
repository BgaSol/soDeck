import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Plus, X } from "lucide-react";
import type { WorkspaceTab } from "./workspaces";
import { tabTitle } from "./workspaces";
import "./workspace-windows.css";

const dragType = "application/x-ssh-workspace-tab";
export function WorkspaceTabs({
  tabs,
  activeId,
  detached = false,
  moving,
  onSelect,
  onClose,
  onMove,
  onReorder,
  onNew,
}: {
  tabs: WorkspaceTab[];
  activeId: string;
  detached?: boolean;
  moving: Set<string>;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onMove: (id: string, target: string | null, beforeId?: string | null) => void;
  onReorder: (id: string, beforeId: string | null) => void;
  onNew?: () => void;
}) {
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(
    null,
  );
  const [over, setOver] = useState<string | null | undefined>(undefined);
  const drag = useRef<{
    token: string;
    ready: Promise<unknown>;
    cancelled: boolean;
  } | null>(null);
  const strip = useRef<HTMLDivElement>(null);
  useEffect(() => {
    strip.current
      ?.querySelector('[aria-selected="true"]')
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeId]);
  useEffect(() => {
    const clear = () => setMenu(null);
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        clear();
        if (drag.current) drag.current.cancelled = true;
      }
    };
    window.addEventListener("click", clear);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("click", clear);
      window.removeEventListener("keydown", key);
    };
  }, []);
  function beforeAt(x: number) {
    const items = Array.from(
      strip.current?.querySelectorAll<HTMLElement>(".session-tab") || [],
    );
    return (
      items.find((el) => {
        const r = el.getBoundingClientRect();
        return x < r.x + r.width / 2;
      })?.dataset.tabId || null
    );
  }
  return (
    <>
      <div
        className={`session-tabs${over !== undefined ? " receiving-tab" : ""}`}
      >
        <div
          className="tab-scroll"
          role="tablist"
          aria-label="SSH 会话"
          ref={strip}
          onDragOver={(e) => {
            if (!e.dataTransfer.types.includes(dragType)) return;
            e.preventDefault();
            e.dataTransfer.dropEffect = "move";
            setOver(beforeAt(e.clientX));
            if (strip.current) {
              const r = strip.current.getBoundingClientRect();
              if (e.clientX < r.left + 30) strip.current.scrollLeft -= 16;
              if (e.clientX > r.right - 30) strip.current.scrollLeft += 16;
            }
          }}
          onDragLeave={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node))
              setOver(undefined);
          }}
          onDrop={(e) => {
            const token = e.dataTransfer.getData(dragType);
            if (!token) return;
            e.preventDefault();
            const beforeId = beforeAt(e.clientX);
            setOver(undefined);
            void invoke("drop_tab", { token, beforeId });
          }}
        >
          {tabs.map((tab, index) => (
            <div
              className={`session-tab${activeId === tab.id ? " active" : ""}${over === tab.id ? " insert-before" : ""}${moving.has(tab.id) ? " moving" : ""}`}
              key={tab.id}
              data-tab-id={tab.id}
            >
              <button
                className="tab-select"
                role="tab"
                id={`tab-${tab.id}`}
                aria-controls={`panel-${tab.id}`}
                aria-selected={activeId === tab.id}
                tabIndex={activeId === tab.id ? 0 : -1}
                aria-label={`${tabTitle(tab)}，${tab.state}`}
                title={`${tab.server.username}@${tab.server.host} · 拖动标签可排序或移到独立窗口`}
                disabled={moving.has(tab.id)}
                draggable={!moving.has(tab.id)}
                onClick={() => onSelect(tab.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({
                    id: tab.id,
                    x: Math.min(e.clientX, innerWidth - 190),
                    y: Math.min(e.clientY, innerHeight - 180),
                  });
                }}
                onDragStart={(e) => {
                  const token = crypto.randomUUID();
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData(dragType, token);
                  e.dataTransfer.setData("text/plain", tabTitle(tab));
                  drag.current = {
                    token,
                    ready: invoke("begin_tab_drag", { token, tabId: tab.id }),
                    cancelled: false,
                  };
                }}
                onDragEnd={() => {
                  setOver(undefined);
                  const d = drag.current;
                  drag.current = null;
                  if (d)
                    void d.ready
                      .then(() => new Promise((r) => setTimeout(r, 80)))
                      .then(() =>
                        invoke("finish_tab_drag", {
                          token: d.token,
                          cancelled: d.cancelled,
                        }),
                      );
                }}
                onKeyDown={(e) => {
                  let next = index;
                  if (
                    e.altKey &&
                    (e.key === "ArrowLeft" || e.key === "ArrowRight")
                  ) {
                    e.preventDefault();
                    const to = e.key === "ArrowLeft" ? index - 1 : index + 2;
                    if (to >= 0) onReorder(tab.id, tabs[to]?.id || null);
                    return;
                  }
                  if (e.key === "ArrowRight") next = (index + 1) % tabs.length;
                  else if (e.key === "ArrowLeft")
                    next = (index - 1 + tabs.length) % tabs.length;
                  else if (e.key === "Home") next = 0;
                  else if (e.key === "End") next = tabs.length - 1;
                  else if (e.key === "Delete") {
                    e.preventDefault();
                    onClose(tab.id);
                    return;
                  } else return;
                  e.preventDefault();
                  onSelect(tabs[next].id);
                  document.getElementById(`tab-${tabs[next].id}`)?.focus();
                }}
              >
                <span
                  className={`tab-state ${tab.state === "已连接" ? "connected" : tab.state === "连接中" ? "connecting" : tab.state === "连接失败" ? "failed" : ""}`}
                  aria-hidden="true"
                />
                <span className="tab-name">{tabTitle(tab)}</span>
                <span className="sr-only">{tab.state}</span>
              </button>
              <button
                className="tab-close"
                aria-label={`关闭 ${tabTitle(tab)} 会话`}
                disabled={moving.has(tab.id)}
                onClick={() => onClose(tab.id)}
              >
                <X size={13} />
              </button>
            </div>
          ))}
          {!tabs.length && (
            <span className="tabs-placeholder">
              {over !== undefined ? "松开以放入会话" : "SSH 会话"}
            </span>
          )}
          {over === null && <span className="tab-end-marker" />}
        </div>
        {detached ? (
          <button
            className="quiet"
            disabled={!tabs.length || moving.size > 0}
            onClick={() => onMove(tabs[0].id, "main")}
          >
            移回主窗口
          </button>
        ) : (
          <button
            className="icon-button new-session"
            aria-label="新建连接"
            title="新建连接"
            disabled={!onNew}
            onClick={onNew}
          >
            <Plus size={17} />
          </button>
        )}
      </div>
      {menu && (
        <div
          className="server-context-menu"
          role="menu"
          style={{ left: menu.x, top: menu.y }}
        >
          <button
            role="menuitem"
            disabled={moving.has(menu.id)}
            onClick={() => onMove(menu.id, detached ? "main" : null)}
          >
            {detached ? "移回主窗口" : "移到独立窗口"}
          </button>
          {!detached && (
            <>
              <button
                role="menuitem"
                disabled={tabs[0]?.id === menu.id}
                onClick={() => {
                  const i = tabs.findIndex((t) => t.id === menu.id);
                  onReorder(menu.id, tabs[i - 1]?.id || null);
                }}
              >
                向左移动
              </button>
              <button
                role="menuitem"
                disabled={tabs.at(-1)?.id === menu.id}
                onClick={() => {
                  const i = tabs.findIndex((t) => t.id === menu.id);
                  onReorder(menu.id, tabs[i + 2]?.id || null);
                }}
              >
                向右移动
              </button>
            </>
          )}
          <button
            role="menuitem"
            disabled={moving.has(menu.id)}
            onClick={() => onClose(menu.id)}
          >
            关闭会话
          </button>
        </div>
      )}
    </>
  );
}
