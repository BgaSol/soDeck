import {core,errorMessage,type Group} from '@sodeck/sdk';
import {Button,InputGroup,Notice,SearchSelect,TextField,Stack,Inline} from '@sodeck/ui';
import type {Controller} from './controller';
import type {Draft,Catalog,GroupEdit} from './model';
export function GroupEditorFields({editor:g,controller:c,onSaved,onCancel}:{editor:GroupEdit;controller:Controller;onSaved(group:Group):void;onCancel():void}){
  return <Stack><InputGroup><TextField autoFocus label="分组名称" maxLength={64} value={g.name} disabled={g.busy||g.unknown||c.store.frozen} onChange={e=>c.act(()=>c.store.editGroup(g,e.target.value))}/><Button disabled={!g.name.trim()||c.store.frozen} busy={g.busy} onClick={()=>void c.saves.saveGroup(g)}>{g.unknown?'核对同一次保存':g.group?'保存名称':'创建分组'}</Button><Button disabled={g.busy||g.unknown} onClick={onCancel}>取消</Button></InputGroup>{g.error&&<Notice kind="error">{g.error}</Notice>}<small>分组独立保存，取消服务器配置不会删除已创建的分组。</small></Stack>;
}
export function GroupFields({draft:d,catalog,controller:c,refresh}:{draft:Draft;catalog:Catalog;controller:Controller;refresh():Promise<void>}){
  return <Stack><SearchSelect label="分组" value={d.fields.groupId??''} options={[{value:'',label:'未分组'},...catalog.groups.map(g=>({value:g.id,label:g.name}))]} error={d.errors.groupId} onChange={groupId=>c.act(()=>c.store.edit(d.id,{groupId:groupId||null}))}/>
    {d.groupEdit?<GroupEditorFields editor={d.groupEdit} controller={c} onCancel={()=>{d.groupEdit=undefined;c.store.changed();}} onSaved={group=>{if(c.store.items.has(d.id)){d.groupEdit=undefined;c.store.edit(d.id,{groupId:group.id});}void refresh();}}/>:<Inline><Button variant="quiet" onClick={()=>{d.groupEdit={name:''};c.store.changed();}}>新建分组</Button></Inline>}
  </Stack>;
}
