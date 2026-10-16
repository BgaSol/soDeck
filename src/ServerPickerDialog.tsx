import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ChevronDown,
  Pencil,
  Plug,
  Plus,
  Search,
  Server as ServerIcon,
  Trash2,
} from "lucide-react";
import { addressOf, filterServers, type Server } from "./types";
import { DialogHeader } from "./DialogHeader";
import "./server-picker.css";
import { forwardApi } from "./portForwardApi";
import { Network } from "lucide-react";

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
  const addButton = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<{
    server: Server;
    x: number;
    y: number;
  } | null>(null);
  const visible = filterServers(servers, query, tag);
  const selected = visible.find((s) => s.id === selectedId);
  const tags = [...new Set(servers.flatMap((s) => s.tags))].sort();
  const empty = !loading && !servers.length && !error;
  useLayoutEffect(() => {
    const element = dialog.current!;
    if (suspended) {
      element.close();
      setMenu(null);
    } else {
      if (!element.open) element.showModal();
      (search.current || addButton.current)?.focus();
    }
  }, [suspended, empty]);
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
      <DialogHeader
        id="server-picker-title"
        title="服务器"
        closeLabel="关闭服务器窗口"
        onClose={onClose}
      />
      {!empty && (
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
          <div className="server-picker-tag">
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
            <ChevronDown size={13} aria-hidden="true" />
          </div>
          <button disabled={busy || loading} onClick={onNew}>
            <Plus size={15} />
            新增
          </button>
        </div>
      )}
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
          <div className="server-picker-empty">
            <span className="server-picker-empty-symbol" aria-hidden="true">
              {!empty && (query || tag) ? (
                <Search size={28} />
              ) : (
                <ServerIcon size={32} />
              )}
            </span>
            <strong>
              {loading
                ? "正在读取服务器…"
                : error && !servers.length
                  ? "无法读取服务器"
                  : servers.length
                    ? "没有匹配的服务器"
                    : "还没有服务器"}
            </strong>
            {empty && (
              <>
                <p>添加服务器，开始你的第一个会话。</p>
                <button
                  ref={addButton}
                  className="primary"
                  disabled={busy}
                  onClick={onNew}
                >
                  <Plus size={15} />
                  添加服务器
                </button>
              </>
            )}
            {!loading && !!servers.length && !visible.length && (
              <p>试试其他名称、地址或标签。</p>
            )}
            {!empty && (query || tag) && (
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
      {!empty && (
        <footer className="server-footer">
          <div className="server-actions">
            <button
              className="quiet"
              disabled={busy || !selected}
              onClick={() => {
                void forwardApi
                  .open(selected?.id)
                  .then(onClose)
                  .catch((e) => window.alert(String(e?.message || e)));
              }}
            >
              <Network size={14} />
              配置端口转发
            </button>
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
          </div>
          <span className="server-picker-count">
            {query || tag
              ? `${visible.length} / ${servers.length}`
              : servers.length}{" "}
            台服务器
          </span>
          <div className="server-picker-confirm">
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
      )}
      {menu && (
        <div
          className="context-menu"
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
