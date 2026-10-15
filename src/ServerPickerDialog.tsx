import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  Pencil,
  Plug,
  Plus,
  Search,
  Server as ServerIcon,
  Trash2,
  X,
} from "lucide-react";
import { addressOf, filterServers, type Server } from "./types";

export function ServerPickerDialog({
  servers,
  selectedId,
  query,
  tag,
  loading,
  busy,
  suspended,
  error,
  connectedIds,
  onSelect,
  onQuery,
  onTag,
  onConnect,
  onEdit,
  onDelete,
  onNew,
  onClose,
  onResetDatabase,
}: {
  servers: Server[];
  selectedId: string;
  query: string;
  tag: string;
  loading: boolean;
  busy: boolean;
  suspended: boolean;
  error?: string;
  connectedIds: Set<string>;
  onSelect: (id: string) => void;
  onQuery: (value: string) => void;
  onTag: (value: string) => void;
  onConnect: (server: Server) => void;
  onEdit: (server: Server) => void;
  onDelete: (server: Server) => void;
  onNew: () => void;
  onClose: () => void;
  onResetDatabase?: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<{
    server: Server;
    x: number;
    y: number;
  } | null>(null);
  const visible = filterServers(servers, query, tag);
  const selected = visible.find((s) => s.id === selectedId);
  const tags = [...new Set(servers.flatMap((s) => s.tags))].sort();
  useLayoutEffect(() => {
    const element = dialog.current!;
    if (suspended) {
      element.close();
      setMenu(null);
    } else {
      if (!element.open) element.showModal();
      search.current?.focus();
    }
  }, [suspended]);
  useEffect(() => {
    if (!visible.some((s) => s.id === selectedId))
      onSelect(visible[0]?.id || "");
  }, [servers, query, tag, selectedId, onSelect]);
  useEffect(() => {
    if (!menu) return;
    menuRef.current?.querySelector("button")?.focus();
    const outside = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenu(null);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [menu]);
  function action(callback: (server: Server) => void) {
    if (menu && !busy) callback(menu.server);
    setMenu(null);
  }
  return (
    <dialog
      ref={dialog}
      className="server-picker-dialog"
      aria-labelledby="server-picker-title"
      onCancel={(event) => {
        event.preventDefault();
        if (menu) {
          setMenu(null);
          search.current?.focus();
        } else onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id="server-picker-title">
          服务器 <span className="inventory-count">{servers.length}</span>
        </h2>
        <button
          className="icon-button"
          aria-label="关闭服务器窗口"
          onClick={onClose}
        >
          <X size={18} />
        </button>
      </header>
      <div className="server-picker-filters">
        <label className="search-box">
          <Search size={16} />
          <input
            ref={search}
            aria-label="搜索服务器"
            placeholder="搜索名称或地址"
            value={query}
            disabled={busy}
            onChange={(event) => onQuery(event.target.value)}
          />
        </label>
        <select
          aria-label="按标签筛选"
          value={tag}
          disabled={busy}
          onChange={(event) => onTag(event.target.value)}
        >
          <option value="">全部标签</option>
          {tags.map((t) => (
            <option key={t}>{t}</option>
          ))}
        </select>
        <button className="primary" disabled={busy || loading} onClick={onNew}>
          <Plus size={15} />
          新增
        </button>
      </div>
      <div className="server-list" aria-label="服务器列表" aria-busy={loading}>
        {visible.map((server) => (
          <button
            key={server.id}
            className={`server-row${selectedId === server.id ? " selected" : ""}`}
            aria-pressed={selectedId === server.id}
            disabled={busy}
            onClick={() => onSelect(server.id)}
            onDoubleClick={() => {
              if (!busy) onConnect(server);
            }}
            onContextMenu={(event) => {
              event.preventDefault();
              if (busy) return;
              onSelect(server.id);
              const bounds = dialog.current!.getBoundingClientRect();
              setMenu({
                server,
                x: Math.max(
                  0,
                  Math.min(event.clientX - bounds.left, bounds.width - 190),
                ),
                y: Math.max(
                  0,
                  Math.min(event.clientY - bounds.top, bounds.height - 150),
                ),
              });
            }}
          >
            <span className="server-symbol" aria-hidden="true">
              <ServerIcon size={17} />
            </span>
            <span className="server-row-body">
              <span className="server-name">{server.name}</span>
              <span className="server-address">{addressOf(server)}</span>
              {!!server.tags.length && (
                <span className="chips">
                  {server.tags.map((t) => (
                    <span className="chip" key={t}>
                      {t}
                    </span>
                  ))}
                </span>
              )}
            </span>
            {connectedIds.has(server.id) && (
              <span className="server-connected">已连接</span>
            )}
          </button>
        ))}
        {!visible.length && (
          <div className="panel-empty">
            <ServerIcon size={26} aria-hidden="true" />
            <strong>
              {loading
                ? "正在读取服务器…"
                : servers.length
                  ? "没有匹配的服务器"
                  : "还没有服务器"}
            </strong>
            {!loading && !servers.length && (
              <p>添加服务器，开始你的第一个会话。</p>
            )}
            {(query || tag) && (
              <button
                onClick={() => {
                  onQuery("");
                  onTag("");
                }}
              >
                清除筛选
              </button>
            )}
          </div>
        )}
      </div>
      {(busy || error) && (
        <div className="server-picker-status" role={error ? "alert" : "status"}>
          {error || "正在建立连接，请完成指纹确认或登录认证…"}
          {onResetDatabase && (
            <button onClick={onResetDatabase}>备份旧库并重建</button>
          )}
        </div>
      )}
      <footer className="server-footer">
        <span>
          {visible.length} / {servers.length} 台服务器
        </span>
        <div className="server-actions">
          <button
            className="quiet"
            disabled={busy || !selected}
            onClick={() => onEdit(selected!)}
          >
            <Pencil size={14} />
            编辑
          </button>
          <button
            className="quiet danger"
            disabled={busy || !selected}
            onClick={() => onDelete(selected!)}
          >
            <Trash2 size={14} />
            删除
          </button>
          <button onClick={onClose}>{busy ? "取消连接" : "取消"}</button>
          <button
            className="primary"
            disabled={busy || !selected || loading}
            onClick={() => onConnect(selected!)}
          >
            <Plug size={14} />
            {busy ? "连接中…" : "连接"}
          </button>
        </div>
      </footer>
      {menu && (
        <div
          className="server-context-menu"
          role="menu"
          aria-label={`${menu.server.name} 操作`}
          ref={menuRef}
          style={{ left: menu.x, top: menu.y }}
        >
          <button role="menuitem" onClick={() => action(onConnect)}>
            <Plus size={14} />
            新建连接
          </button>
          <button role="menuitem" onClick={() => action(onEdit)}>
            <Pencil size={14} />
            编辑
          </button>
          <button
            role="menuitem"
            className="danger"
            onClick={() => action(onDelete)}
          >
            <Trash2 size={14} />
            删除服务器
          </button>
        </div>
      )}
    </dialog>
  );
}
