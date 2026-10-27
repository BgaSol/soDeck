import {Input,Select,Button} from '@sodeck/ui';
import {useServerGroups} from './server-catalog-client';
import { useState } from "react";
import { ArrowUpRight, Plus, Search, Server as ServerIcon, Settings2, Trash2 } from "lucide-react";
import { Dialog } from "./dialog";
import type { Server, Workspace } from "./types";

export function ServerPicker({ servers, workspaces, onClose, onOpen, onEdit, onDelete,catalogSupported=false,onManageGroups }: {
  catalogSupported?:boolean;onManageGroups?():void;
  servers: Server[]; workspaces: Workspace[]; onClose(): void;
  onOpen(server: Server, newWindow: boolean): void; onEdit(server: Server | "new"): void; onDelete(server: Server): void;
}) {
  const [search, setSearch] = useState("");
  const [groupId,setGroupId]=useState('*');
  const {groups,error}=useServerGroups(catalogSupported);
  const groupName=(server:Server)=>groups.find(g=>g.id===server.groupId)?.name??'未分组';
  const results = servers.filter(server => (groupId==='*'||(server.groupId??'')===groupId)&&`${server.name} ${server.host} ${server.username} ${groupName(server)}`.toLocaleLowerCase().includes(search.toLocaleLowerCase()));
  return <Dialog className="server-picker-dialog" title="打开服务器" subtitle="选择一台服务器，继续你的工作。" onClose={onClose}>
    <div className="server-picker">
      <label className="server-search"><Search size={18} aria-hidden="true" /><Input autoFocus aria-label="搜索服务器" placeholder="搜索名称、地址或用户名" value={search} onChange={event => setSearch(event.target.value)} /></label>
      {catalogSupported&&<div className="group-picker-tools"><Select aria-label="按分组筛选" value={groupId} onChange={e=>setGroupId(e.target.value)}><option value="*">全部分组</option><option value="">未分组</option>{groups.map(g=><option key={g.id} value={g.id}>{g.name} · {g.serverCount}</option>)}</Select><Button onClick={onManageGroups}>管理分组</Button></div>}
      {error&&<p role="alert" className="inline-error">{error}</p>}
      <div className="picker-list">{results.map(server => {
        const workspace = workspaces.find(item => item.serverId === server.id);
        return <div className="picker-row" key={server.id}>
          <Button className="picker-open" onClick={() => onOpen(server, false)}>
            <span className="picker-avatar" aria-hidden="true"><ServerIcon size={19} /></span>
            <span className="picker-details"><strong title={server.name}>{server.name}</strong><small title={`${server.username}@${server.host}:${server.port}`}>{server.username}@{server.host}:{server.port}</small>{server.connectionRoute?.kind==="sshJump"&&<small>经由 {servers.find(s=>s.id===(server.connectionRoute as {jumpServerId:string}).jumpServerId)?.name??"跳板服务器"}</small>}{catalogSupported&&<small>{groupName(server)}</small>}{workspace && <span className="picker-status">已打开 · {workspace.ownerWindowId === "main" ? "主窗口" : "其他窗口"}</span>}</span>
          </Button>
          <div className="picker-actions">
          <Button variant="quiet" size="compact" className="icon-button" aria-label={`在新窗口打开 ${server.name}`} title="在新窗口打开" onClick={() => onOpen(server, true)}><ArrowUpRight size={16} /></Button>
          <Button variant="quiet" size="compact" className="icon-button" aria-label={`编辑 ${server.name}`} title="编辑服务器" onClick={() => onEdit(server)}><Settings2 size={16} /></Button>
          <Button variant="quiet" size="compact" className="icon-button picker-delete" aria-label={`删除 ${server.name}`} title="删除服务器" onClick={() => onDelete(server)}><Trash2 size={16} /></Button>
          </div>
        </div>;
      })}{!results.length && <p className="picker-empty">{servers.length ? "没有匹配的服务器" : "还没有保存服务器"}</p>}</div>
      <footer className="picker-footer"><span role="status">{search||groupId!=='*' ? `${results.length} 个匹配 · 共 ${servers.length} 台` : `${servers.length} 台服务器`}</span><Button variant="primary" className="primary" onClick={() => onEdit("new")}><Plus size={16} />添加服务器</Button></footer>
    </div>
  </Dialog>;
}
