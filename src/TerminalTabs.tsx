import { useEffect, useLayoutEffect, useId, useRef, useState } from "react";
import {
  Plus,
  X,
  ChevronDown,
  History,
  Ellipsis,
  CircleAlert,
} from "lucide-react";
import type { TerminalHeaderState } from "./TerminalSession";
import { WorkspaceTabMenu, type TabMenuItem } from "./WorkspaceTabMenu";
import {
  terminalStateLabel,
  terminalTitle,
  type TerminalTab,
} from "./workspaces";
import "./terminal-tabs.css";

export type TerminalHeaderAction =
  | "history"
  | "copy"
  | "paste"
  | "clear"
  | "search"
  | "disconnect"
  | "reconnect"
  | "challenge"
  | "details";

export function TerminalTabs({
  tabs,
  activeId,
  disabled,
  onSelect,
  onCreate,
  onClose,
  onRename,
  onReorder,
  header,
  onAction,
  active = true,
}: {
  tabs: TerminalTab[];
  activeId: string | null;
  disabled: boolean;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onClose: (id: string) => void;
  onRename: (id: string, title: string | null) => void;
  onReorder: (ids: string[]) => void;
  header?: TerminalHeaderState;
  onAction?: (id: string, action: TerminalHeaderAction) => void;
  active?: boolean;
}) {
  const track = useRef<HTMLDivElement>(null);
  const actionGroup = useRef<HTMLDivElement>(null);
  const bar = useRef<HTMLDivElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  const menuId = useId();
  const [menu, setMenu] = useState<{
    id?: string;
    all?: boolean;
    anchor: HTMLElement;
    x: number;
    y: number;
  } | null>(null);
  const closeMenu = (restore: boolean) => {
    const anchor = menu?.anchor;
    setMenu(null);
    if (restore)
      requestAnimationFrame(() => anchor?.isConnected && anchor.focus());
  };
  const openMenu = (
    anchor: HTMLElement,
    id?: string,
    all = false,
    x?: number,
    y?: number,
  ) => {
    if (menu?.anchor === anchor) {
      closeMenu(true);
      return;
    }
    const bounds = anchor.getBoundingClientRect();
    setMenu({
      id,
      all,
      anchor,
      x: x ?? bounds.right - 280,
      y: y ?? bounds.bottom + 4,
    });
  };
  const dragged = useRef<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState("");
  useEffect(() => {
    setMenu(null);
  }, [activeId, active, disabled]);
  useEffect(() => {
    if (menu?.id && !tabs.some((t) => t.id === menu.id)) setMenu(null);
  }, [tabs, menu?.id]);
  useLayoutEffect(() => {
    const viewport = track.current;
    if (!viewport) return;
    let frame = 0;
    const measure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        // Capacity before adding the overflow control avoids oscillation.
        const extra =
          actionGroup.current?.querySelector<HTMLElement>(
            ".terminal-tabs-overflow",
          )?.offsetWidth ?? 0;
        const capacity = viewport.clientWidth + (extra ? extra + 4 : 0);
        setOverflowing(viewport.scrollWidth > capacity + 1);
        viewport
          .querySelector('[aria-selected="true"]')
          ?.scrollIntoView({ block: "nearest", inline: "nearest" });
      });
    };
    const observer = new ResizeObserver(measure);
    [viewport, bar.current, actionGroup.current, ...viewport.children].forEach(
      (node) => node && observer.observe(node),
    );
    measure();
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [tabs, renaming]);
  useEffect(() => {
    track.current
      ?.querySelector<HTMLButtonElement>('[aria-selected="true"]')
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeId, tabs, overflowing, active]);
  const selected = tabs.find((t) => t.id === menu?.id);
  const isCurrent = selected?.id === activeId;
  const chrome = header?.chrome;
  const action = (id: string, kind: TerminalHeaderAction) =>
    onAction?.(id, kind);
  const rename = (tab: TerminalTab) => {
    setName(tab.title || "");
    setRenaming(tab.id);
  };
  const menuItems: TabMenuItem[] = menu?.all
    ? tabs.map((t) => ({
        id: t.id,
        label: terminalTitle(t),
        detail: terminalStateLabel(t.state),
        selected: t.id === activeId,
        run: () => {
          onSelect(t.id);
          requestAnimationFrame(() =>
            document.getElementById(`terminal-tab-${t.id}`)?.focus(),
          );
        },
      }))
    : selected
      ? [
          ...(isCurrent
            ? [
                {
                  id: "copy",
                  label: "复制选区",
                  disabled:
                    chrome?.mode === "history" ||
                    !chrome?.clipboard.hasSelection,
                  detail:
                    chrome?.mode === "history"
                      ? "请使用历史视图的复制功能"
                      : !chrome?.clipboard.hasSelection
                        ? "未选择终端文字"
                        : undefined,
                  run: () => action(selected.id, "copy"),
                },
                {
                  id: "paste",
                  label: "粘贴",
                  disabled:
                    chrome?.mode === "history" ||
                    !chrome?.clipboard.canPaste ||
                    chrome?.clipboard.busy,
                  detail:
                    chrome?.mode === "history"
                      ? "历史视图只读"
                      : chrome?.clipboard.unavailableReason,
                  run: () => action(selected.id, "paste"),
                },
                {
                  id: "clear",
                  label: "清理显示",
                  disabled:
                    !header?.sessionId ||
                    chrome?.mode === "history" ||
                    chrome?.alternate,
                  detail: chrome?.alternate
                    ? "屏幕程序运行期间由程序处理清屏"
                    : "保留当前输入行和全部历史",
                  run: () => action(selected.id, "clear"),
                },
                {
                  id: "search",
                  label: "搜索全部终端历史",
                  groupBefore: true,
                  run: () => action(selected.id, "search"),
                },
                {
                  id: "history-status",
                  label: chrome?.recordLabel || "历史记录状态尚未就绪",
                  info: true,
                  run: () => {},
                },
                {
                  id: "details",
                  label: "连接详情",
                  groupBefore: true,
                  run: () => action(selected.id, "details"),
                },
              ]
            : []),
          { id: "rename", label: "重命名", run: () => rename(selected) },
          {
            id: "connection",
            label: ["disconnected", "failed"].includes(selected.state)
              ? "重新连接"
              : isCurrent && header?.automatic
                ? "停止重连"
                : "断开当前终端",
            danger: !["disconnected", "failed"].includes(selected.state),
            run: () =>
              action(
                selected.id,
                ["disconnected", "failed"].includes(selected.state)
                  ? "reconnect"
                  : "disconnect",
              ),
          },
          {
            id: "close",
            label: "关闭终端",
            groupBefore: true,
            run: () => onClose(selected.id),
          },
        ]
      : [];
  const recoveryAction = header?.prompt
    ? "challenge"
    : header?.phase === "waiting"
      ? "reconnect"
      : header?.status === "已断开" || header?.status === "连接失败"
        ? "reconnect"
        : "disconnect";
  const recoveryLabel = header?.prompt
    ? "继续验证"
    : header?.phase === "waiting"
      ? "立即重连"
      : header?.status === "连接失败"
        ? "重试连接"
        : header?.status === "已断开"
          ? "重新连接"
          : "取消连接";
  return (
    <div className="terminal-tabs" ref={bar} aria-label="终端工作条">
      <div
        ref={track}
        role="tablist"
        aria-label="终端标签"
        className="terminal-tabs-track"
        onKeyDown={(e) => {
          if (
            (e.key === "Delete" || e.key === "Backspace") &&
            !disabled &&
            activeId
          ) {
            e.preventDefault();
            onClose(activeId);
            return;
          }
          const index = tabs.findIndex((t) => t.id === activeId);
          if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
            e.preventDefault();
            const to = index + (e.key === "ArrowLeft" ? -1 : 1);
            if (!disabled && index >= 0 && to >= 0 && to < tabs.length) {
              const ids = tabs.map((t) => t.id);
              [ids[index], ids[to]] = [ids[to], ids[index]];
              onReorder(ids);
            }
            return;
          }
          const next =
            e.key === "ArrowRight"
              ? (index + 1) % tabs.length
              : e.key === "ArrowLeft"
                ? (index - 1 + tabs.length) % tabs.length
                : e.key === "Home"
                  ? 0
                  : e.key === "End"
                    ? tabs.length - 1
                    : -1;
          if (next < 0 || disabled) return;
          e.preventDefault();
          onSelect(tabs[next].id);
          track.current
            ?.querySelector<HTMLButtonElement>(
              `[data-terminal-id="${tabs[next].id}"]`,
            )
            ?.focus();
        }}
      >
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={`terminal-tab ${activeId === tab.id ? "active" : ""}`}
            draggable={!disabled && renaming !== tab.id}
            onContextMenu={(e) => {
              e.preventDefault();
              if (!disabled)
                openMenu(
                  e.currentTarget.querySelector<HTMLElement>('[role="tab"]') ??
                    e.currentTarget,
                  tab.id,
                  false,
                  e.clientX,
                  e.clientY,
                );
            }}
            onDragStart={(e) => {
              e.stopPropagation();
              dragged.current = tab.id;
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("application/x-sodeck-terminal", tab.id);
            }}
            onDragEnd={() => {
              dragged.current = null;
            }}
            onDragOver={(e) => {
              if (dragged.current) {
                e.preventDefault();
                e.stopPropagation();
              }
            }}
            onDrop={(e) => {
              if (!dragged.current || disabled) return;
              e.preventDefault();
              e.stopPropagation();
              if (dragged.current === tab.id) {
                dragged.current = null;
                return;
              }
              const ids = tabs
                .filter((t) => t.id !== dragged.current)
                .map((t) => t.id);
              ids.splice(Math.max(0, ids.indexOf(tab.id)), 0, dragged.current);
              dragged.current = null;
              onReorder(ids);
            }}
          >
            {renaming === tab.id ? (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  onRename(tab.id, name.trim() || null);
                  setRenaming(null);
                  requestAnimationFrame(() =>
                    document.getElementById(`terminal-tab-${tab.id}`)?.focus(),
                  );
                }}
              >
                <input
                  autoFocus
                  aria-label="终端名称"
                  value={name}
                  maxLength={80}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Escape") {
                      setRenaming(null);
                      requestAnimationFrame(() =>
                        document
                          .getElementById(`terminal-tab-${tab.id}`)
                          ?.focus(),
                      );
                    }
                  }}
                  onBlur={() => setRenaming(null)}
                />
              </form>
            ) : (
              <button
                type="button"
                role="tab"
                id={`terminal-tab-${tab.id}`}
                data-terminal-id={tab.id}
                aria-controls={`terminal-panel-${tab.id}`}
                aria-selected={activeId === tab.id}
                tabIndex={activeId === tab.id ? 0 : -1}
                disabled={disabled}
                title={`${terminalTitle(tab)} · ${terminalStateLabel(tab.state)}；双击重命名`}
                onClick={(e) => {
                  onSelect(tab.id);
                  e.currentTarget.scrollIntoView({
                    block: "nearest",
                    inline: "nearest",
                  });
                }}
                onDoubleClick={() => {
                  rename(tab);
                }}
                onKeyDown={(e) => {
                  if (e.key === "F2") {
                    e.preventDefault();
                    setName(tab.title || "");
                    setRenaming(tab.id);
                  }
                }}
              >
                <span
                  className={`terminal-tab-dot ${tab.state}`}
                  aria-hidden="true"
                />
                <span className="terminal-tab-name">{terminalTitle(tab)}</span>
              </button>
            )}
            <button
              className="terminal-tab-close"
              aria-label={`关闭${terminalTitle(tab)}`}
              disabled={disabled}
              onClick={() => onClose(tab.id)}
            >
              <X size={12} />
            </button>
          </div>
        ))}
      </div>
      <div className="terminal-header-actions" ref={actionGroup}>
        {overflowing && (
          <button
            className="icon-button terminal-tabs-overflow"
            aria-label="全部终端"
            title="全部终端"
            aria-haspopup="menu"
            aria-expanded={!!menu?.all}
            disabled={disabled}
            onClick={(e) => openMenu(e.currentTarget, undefined, true)}
          >
            <ChevronDown size={15} />
          </button>
        )}
        <button
          className="icon-button"
          aria-label="新建终端"
          title="新建终端"
          disabled={disabled}
          onClick={onCreate}
        >
          <Plus size={16} />
        </button>
        {activeId && header && header.status !== "已连接" && (
          <button
            className="quiet terminal-recovery-action"
            disabled={disabled}
            onClick={() => action(activeId, recoveryAction)}
          >
            {recoveryLabel}
          </button>
        )}
        <button
          className="quiet terminal-header-history"
          aria-label={chrome?.mode === "history" ? "返回实时终端" : "历史"}
          aria-pressed={chrome?.mode === "history"}
          title={chrome?.recordLabel || "查看当前终端历史"}
          disabled={disabled || !activeId}
          onClick={() => activeId && action(activeId, "history")}
        >
          <History size={15} />
          <span className="terminal-header-label">历史</span>
          {chrome?.recordError && (
            <CircleAlert size={13} aria-label="历史保存异常" />
          )}
        </button>
        <button
          className="icon-button"
          aria-label="当前终端更多操作"
          title="当前终端更多操作"
          aria-haspopup="menu"
          aria-expanded={!!menu?.id && !menu.all}
          disabled={disabled || !activeId}
          onClick={(e) => activeId && openMenu(e.currentTarget, activeId)}
        >
          <Ellipsis size={16} />
        </button>
      </div>
      {menu && (menu.all || selected) && (
        <WorkspaceTabMenu
          id={menuId}
          title={menu.all ? "全部终端" : terminalTitle(selected!)}
          anchor={menu.anchor}
          x={menu.x}
          y={menu.y}
          items={menuItems}
          onClose={closeMenu}
        />
      )}
    </div>
  );
}
