import {useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import {Button,Input,Notice} from '@sodeck/ui';
import {core,CoreError,errorMessage,type Group} from '@sodeck/sdk';
import {Dialog} from './dialog';
import {useServerGroups} from './server-catalog-client';
import type {Server} from './types';

export function GroupEditor({group,onClose,onSaved}:{group?:Group;onClose():void;onSaved(group:Group):void}) {
  const [name,setName]=useState(group?.name??''),[error,setError]=useState(''),[busy,setBusy]=useState(false),[pending,setPending]=useState(false);
  const operation=useRef<{name:string;id:string}|undefined>(undefined);
  const save=async()=>{if(busy||!name.trim())return;setBusy(true);setError('');
    if(operation.current?.name!==name)operation.current={name,id:crypto.randomUUID()};
    try {const result=await core.call<{group:Group}>('serverGroups.'+(group?'update':'create'),{operationId:operation.current.id,name,...(group?{groupId:group.id,expectedRevision:group.revision}:{})});onSaved(result.group);}
    catch(e){const unknown=e instanceof CoreError&&['RESULT_UNKNOWN','TIMEOUT'].includes(e.code);setPending(unknown);setError((unknown?'保存结果待核实，请重试同一次保存。':'')+errorMessage(e));}finally{setBusy(false);}
  };
  return createPortal(<div onKeyDown={e=>e.stopPropagation()}><Dialog title={group?'重命名分组':'新建分组'} subtitle="分组独立保存，取消服务器表单也会保留。" onClose={()=>{if(!busy)onClose();}}><form onSubmit={e=>{e.preventDefault();void save();}}><label>分组名称<Input autoFocus value={name} maxLength={64} disabled={busy||pending} onChange={e=>setName(e.target.value)} required/></label>{error&&<Notice kind="error">{error}</Notice>}<footer><Button disabled={busy} onClick={onClose}>取消</Button><Button type="submit" variant="primary" disabled={busy||!name.trim()}>保存分组</Button></footer></form></Dialog></div>,document.body);
}

export function ServerGroups({servers,onClose,onEdit}:{servers:Server[];onClose():void;onEdit(server:Server):void}) {
  const {groups,revision,error,refresh}=useServerGroups(true);
  const [editor,setEditor]=useState<Group|'new'|null>(null),[remove,setRemove]=useState<Group|null>(null),[busy,setBusy]=useState(false),[fault,setFault]=useState(''),[selected,setSelected]=useState<string|null>(null);
  const deleteOperation=useRef<string|null>(null);
  const deleteRevision=useRef('0');
  const deleteGroup=async(group:Group,operation:string=crypto.randomUUID(),expectedCatalogRevision=revision)=>{
    if(busy)return;setBusy(true);setFault('');try{await core.call('serverGroups.delete',{operationId:operation,groupId:group.id,expectedRevision:group.revision,expectedCatalogRevision});setRemove(null);if(selected===group.id)setSelected(null);await refresh();}catch(e){setFault(errorMessage(e));}finally{setBusy(false);}
  };
  return <><Dialog title="管理服务器分组" subtitle="删除分组后，服务器会移至未分组，连接保持不变。" onClose={()=>{if(!busy)onClose();}}><div className="management-body">
    <div className="management-tools"><Button onClick={onClose}>返回服务器列表</Button><Button variant="primary" onClick={()=>setEditor('new')}>新建分组</Button></div>
    {(fault||error)&&<Notice kind="error">{fault||error}</Notice>}
    <ul className="resource-list">{groups.map(g=><li key={g.id}><Button variant="quiet" onClick={()=>setSelected(selected===g.id?null:g.id)}>{g.name} · {g.serverCount} 台</Button><div className="group-actions"><Button disabled={busy} onClick={()=>setEditor(g)}>改名</Button><Button variant="danger" disabled={busy} onClick={()=>{if(!g.serverCount)void deleteGroup(g);else{deleteOperation.current=crypto.randomUUID();deleteRevision.current=revision;setRemove(g);}}}>删除</Button></div></li>)}</ul>
    {!groups.length&&<p className="empty-text">还没有分组，服务器默认显示在未分组中。</p>}
    {selected&&<section aria-label="组内服务器"><h3>{groups.find(g=>g.id===selected)?.name}</h3>{servers.filter(s=>s.groupId===selected).map(s=><div key={s.id} className="group-member"><span>{s.name}</span><Button onClick={()=>onEdit(s)}>编辑服务器</Button></div>)}</section>}
  </div></Dialog>
  {editor&&<GroupEditor group={editor==='new'?undefined:editor} onClose={()=>setEditor(null)} onSaved={()=>{setEditor(null);void refresh();}}/>}
  {remove&&<Dialog title="删除分组" onClose={()=>{if(!busy)setRemove(null);}}><div className="confirm-body"><p>删除“{remove.name}”，{remove.serverCount} 台服务器移至未分组。</p>{fault&&<Notice kind="error">{fault}</Notice>}<footer><Button disabled={busy} onClick={()=>setRemove(null)}>取消</Button><Button variant="danger" disabled={busy} onClick={()=>void deleteGroup(remove,deleteOperation.current!,deleteRevision.current)}>删除分组</Button></footer></div></Dialog>}
  </>;
}
