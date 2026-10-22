import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { desktop } from "./api";
import {
  Plus,
  X,
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  PanelTop,
  Server as ServerIcon,
} from "lucide-react";
import type { WorkspaceTab } from "./workspaces";
import { tabTitle } from "./workspaces";
import { useTabStrip } from "./useTabStrip";
import { useWorkspaceTabDrag } from "./useWorkspaceTabDrag";
import { WorkspaceTabMenu, type TabMenuItem } from "./WorkspaceTabMenu";
import "./workspace-windows.css";

export interface WorkspaceTabsProps {
  tabs: WorkspaceTab[];
  activeId: string;
  detached?: boolean;
  moving: Set<string>;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onMove: (id: string, target: string | null, beforeId?: string | null) => void;
  onReorder: (id: string, beforeId: string | null) => void;
  onNew?: () => void;
  onNewConnection?: (tabId: string) => void;
  onHome?: () => void;
  onError: (error: unknown) => void;
}

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
  onNewConnection,
  onHome,
  onError,
  appendDropRef,
  onAppendDropChange,
}: WorkspaceTabsProps & {
  appendDropRef?: RefObject<HTMLDivElement | null>;
  onAppendDropChange?: (active: boolean) => void;
}) {
  const layoutKey = JSON.stringify(tabs.map((tab) => [tab.id, tabTitle(tab)]));
  const scroll = useTabStrip(activeId, layoutKey);
  const drag = useWorkspaceTabDrag({
    strip: scroll.strip,
    appendDropRef,
    onAppendDropChange,
    onError,
  });
  const [menu, setMenu] = useState<{
    tabId?: string;
    anchor: HTMLElement;
    x: number;
    y: number;
  } | null>(null);
  const menuId = useId();
  const currentId = useRef(activeId);
  currentId.current = activeId;
  function focusTab(id: string) {
    requestAnimationFrame(() => {
      const tab =
        document.getElementById(`tab-${id}`) ??
        document.getElementById(`tab-${currentId.current}`);
      tab?.focus({ preventScroll: true });
      scroll.revealTab(tab?.id.slice(4) ?? currentId.current);
      if (!tab) scroll.action.current?.focus();
    });
  }
  function closeMenu(restoreFocus: boolean) {
    const anchor = menu?.anchor;
    setMenu(null);
    if (restoreFocus)
      requestAnimationFrame(() => {
        if (anchor?.isConnected) anchor.focus({ preventScroll: true });
        else focusTab(currentId.current);
      });
  }
  useEffect(() => {
    if (menu?.tabId && !tabs.some((tab) => tab.id === menu.tabId))
      setMenu(null);
    if (menu && !menu.tabId && !scroll.overflowing) setMenu(null);
  }, [tabs, menu, scroll.overflowing]);
  function select(id: string) {
    onSelect(id);
    focusTab(id);
  }
  const selected = menu?.tabId
    ? tabs.find((tab) => tab.id === menu.tabId)
    : undefined;
  const menuItems: TabMenuItem[] = selected
    ? [
        {
          id: "new-connection",
          label: "新建终端",

          disabled: !onNewConnection || moving.has(selected.id),
          run: () => onNewConnection?.(selected.id),
        },
        {
          id: "move",
          label: detached ? "移回主窗口" : "移到独立窗口",
          disabled: moving.has(selected.id),
          run: () => onMove(selected.id, detached ? "main" : null),
        },
        ...(!detached
          ? [
              {
                id: "left",
                label: "向左移动",
                disabled:
                  moving.has(selected.id) || tabs[0]?.id === selected.id,
                run: () => {
                  const i = tabs.findIndex((t) => t.id === selected.id);
                  onReorder(selected.id, tabs[i - 1]?.id ?? null);
                  focusTab(selected.id);
                },
              },
              {
                id: "right",
                label: "向右移动",
                disabled:
                  moving.has(selected.id) || tabs.at(-1)?.id === selected.id,
                run: () => {
                  const i = tabs.findIndex((t) => t.id === selected.id);
                  onReorder(selected.id, tabs[i + 2]?.id ?? null);
                  focusTab(selected.id);
                },
              },
            ]
          : []),
        {
          id: "close",
          label: "关闭服务器",
          disabled: moving.has(selected.id),
          run: () => {
            onClose(selected.id);
            focusTab(currentId.current);
          },
        },
      ]
    : tabs.map((tab) => ({
        id: tab.id,
        label: tabTitle(tab),
        detail: tab.state,
        selected: tab.id === activeId,
        disabled: moving.has(tab.id),
        run: () => select(tab.id),
      }));
  return (
    <>
      {onHome && (
        <button
          className={`server-home-tab${!activeId ? " active" : ""}`}
          aria-label="服务器首页"
          aria-pressed={!activeId}
          onClick={onHome}
        >
          <ServerIcon size={15} />
          服务器
        </button>
      )}
      <div
        ref={scroll.root}
        className={`session-tabs${drag.over !== undefined ? " receiving-tab" : ""}`}
      >
        {scroll.overflowing && (
          <button
            className="icon-button tab-scroll-button"
            aria-label="向左滚动服务器"
            title="向左滚动服务器"
            disabled={!scroll.canScrollLeft}
            onClick={() => scroll.scrollByPage(-1)}
          >
            <ChevronLeft size={15} />
          </button>
        )}
        <div
          ref={scroll.strip}
          className="tab-scroll"
          role="tablist"
          aria-label="服务器标签"
          {...drag.htmlHandlers}
        >
          <div
            ref={scroll.track}
            className={`tab-track${drag.over === null ? " insert-at-end" : ""}`}
          >
            {tabs.map((tab, index) => (
              <div
                key={tab.id}
                data-tab-id={tab.id}
                className={`session-tab${activeId === tab.id ? " active" : ""}${drag.over === tab.id ? " insert-before" : ""}${moving.has(tab.id) ? " moving" : ""}`}
              >
                <button
                  className="tab-select"
                  role="tab"
                  id={`tab-${tab.id}`}
                  aria-controls={`panel-${tab.id}`}
                  aria-selected={activeId === tab.id}
                  tabIndex={
                    activeId === tab.id || (!activeId && index === 0) ? 0 : -1
                  }
                  aria-label={`${tabTitle(tab)}，${tab.state}`}
                  title={`${tabTitle(tab)} · ${tab.server.username}@${tab.server.host}:${tab.server.port} · ${tab.state} · 拖动标签可排序或移到独立窗口`}
                  disabled={moving.has(tab.id)}
                  draggable={!desktop && !moving.has(tab.id)}
                  onPointerDown={(event) => {
                    if (!moving.has(tab.id))
                      drag.native.down(event, () => drag.beginNative(tab.id));
                  }}
                  onClick={() => {
                    if (!drag.native.suppressClick()) onSelect(tab.id);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    const bounds = event.currentTarget.getBoundingClientRect();
                    setMenu({
                      tabId: tab.id,
                      anchor: event.currentTarget,
                      x: event.clientX || bounds.left,
                      y: event.clientY || bounds.bottom,
                    });
                  }}
                  onDragStart={(event) =>
                    drag.beginHtml(event, tab.id, tabTitle(tab))
                  }
                  onDragEnd={drag.endHtml}
                  onKeyDown={(event) => {
                    let next = index;
                    if (
                      event.altKey &&
                      (event.key === "ArrowLeft" || event.key === "ArrowRight")
                    ) {
                      event.preventDefault();
                      const to =
                        event.key === "ArrowLeft" ? index - 1 : index + 2;
                      if (to >= 0) onReorder(tab.id, tabs[to]?.id ?? null);
                      return;
                    }
                    if (event.key === "ArrowRight")
                      next = (index + 1) % tabs.length;
                    else if (event.key === "ArrowLeft")
                      next = (index - 1 + tabs.length) % tabs.length;
                    else if (event.key === "Home") next = 0;
                    else if (event.key === "End") next = tabs.length - 1;
                    else if (event.key === "Delete") {
                      event.preventDefault();
                      onClose(tab.id);
                      focusTab(currentId.current);
                      return;
                    } else return;
                    event.preventDefault();
                    select(tabs[next].id);
                  }}
                >
                  <span
                    className={`tab-state ${tab.state === "已连接" ? "connected" : ["连接中", "等待重连", "正在重连", "等待验证"].includes(tab.state) ? "connecting" : tab.state === "连接失败" ? "failed" : ""}`}
                    aria-hidden="true"
                  />
                  <span className="tab-name">{tabTitle(tab)}</span>
                  <span className="sr-only">{tab.state}</span>
                </button>
                <button
                  className="tab-close"
                  aria-label={`关闭 ${tabTitle(tab)} 服务器`}
                  title="关闭服务器"
                  disabled={moving.has(tab.id)}
                  onClick={() => {
                    onClose(tab.id);
                    focusTab(currentId.current);
                  }}
                >
                  <X size={13} />
                </button>
              </div>
            ))}
          </div>
        </div>
        {scroll.overflowing && (
          <>
            <button
              className="icon-button tab-scroll-button"
              aria-label="向右滚动服务器"
              title="向右滚动服务器"
              disabled={!scroll.canScrollRight}
              onClick={() => scroll.scrollByPage(1)}
            >
              <ChevronRight size={15} />
            </button>
            <button
              className="icon-button tab-list-button"
              aria-label="全部服务器"
              title="全部服务器"
              aria-haspopup="menu"
              aria-expanded={!!menu && !menu.tabId}
              aria-controls={menu && !menu.tabId ? menuId : undefined}
              onClick={(event) => {
                if (menu && !menu.tabId) {
                  closeMenu(true);
                  return;
                }
                const bounds = event.currentTarget.getBoundingClientRect();
                setMenu({
                  anchor: event.currentTarget,
                  x: bounds.right - 280,
                  y: bounds.bottom + 6,
                });
              }}
            >
              <ChevronDown size={15} />
            </button>
          </>
        )}
        {detached ? (
          <button
            ref={scroll.action}
            className="icon-button tab-return"
            aria-label="移回主窗口"
            title="移回主窗口"
            disabled={!tabs.length || moving.size > 0}
            onClick={() => onMove(tabs[0].id, "main")}
          >
            <PanelTop size={16} />
          </button>
        ) : (
          <button
            ref={scroll.action}
            className="icon-button new-session"
            aria-label="新建连接"
            title="选择服务器并新建连接"
            disabled={!onNew}
            onClick={onNew}
          >
            <Plus size={17} />
          </button>
        )}
      </div>
      {menu && (
        <WorkspaceTabMenu
          id={menuId}
          title={menu.tabId ? "服务器操作" : "全部服务器"}
          anchor={menu.anchor}
          x={menu.x}
          y={menu.y}
          items={menuItems}
          onClose={closeMenu}
        />
      )}
    </>
  );
}
