import { useState } from "react";
import { ArrowUpRight, Plus, Search, Server as ServerIcon, Settings2, Trash2 } from "lucide-react";
import { Dialog } from "./dialog";
import type { Server, Workspace } from "./types";

export function ServerPicker({ servers, workspaces, onClose, onOpen, onEdit, onDelete }: {
  servers: Server[]; workspaces: Workspace[]; onClose(): void;
  onOpen(server: Server, newWindow: boolean): void; onEdit(server: Server | "new"): void; onDelete(server: Server): void;
}) {
  const [search, setSearch] = useState("");
  const results = servers.filter(server => `${server.name} ${server.host} ${server.username}`.toLocaleLowerCase().includes(search.toLocaleLowerCase()));
  return <Dialog className="server-picker-dialog" title="打开服务器" subtitle="选择一台服务器，继续你的工作。" onClose={onClose}>
    <div className="server-picker">
      <label className="server-search"><Search size={18} aria-hidden="true" /><input autoFocus aria-label="搜索服务器" placeholder="搜索名称、地址或用户名" value={search} onChange={event => setSearch(event.target.value)} /></label>
      <div className="picker-list">{results.map(server => {
        const workspace = workspaces.find(item => item.serverId === server.id);
        return <div className="picker-row" key={server.id}>
          <button className="picker-open" onClick={() => onOpen(server, false)}>
            <span className="picker-avatar" aria-hidden="true"><ServerIcon size={19} /></span>
            <span className="picker-details"><strong title={server.name}>{server.name}</strong><small title={`${server.username}@${server.host}:${server.port}`}>{server.username}@{server.host}:{server.port}</small>{workspace && <span className="picker-status">已打开 · {workspace.ownerWindowId === "main" ? "主窗口" : "其他窗口"}</span>}</span>
          </button>
          <div className="picker-actions">
          <button className="icon-button" aria-label={`在新窗口打开 ${server.name}`} title="在新窗口打开" onClick={() => onOpen(server, true)}><ArrowUpRight size={16} /></button>
          <button className="icon-button" aria-label={`编辑 ${server.name}`} title="编辑服务器" onClick={() => onEdit(server)}><Settings2 size={16} /></button>
          <button className="icon-button picker-delete" aria-label={`删除 ${server.name}`} title="删除服务器" onClick={() => onDelete(server)}><Trash2 size={16} /></button>
          </div>
        </div>;
      })}{!results.length && <p className="picker-empty">{servers.length ? "没有匹配的服务器" : "还没有保存服务器"}</p>}</div>
      <footer className="picker-footer"><span role="status">{search ? `${results.length} 个匹配 · 共 ${servers.length} 台` : `${servers.length} 台服务器`}</span><button className="primary" onClick={() => onEdit("new")}><Plus size={16} />添加服务器</button></footer>
    </div>
  </Dialog>;
}
