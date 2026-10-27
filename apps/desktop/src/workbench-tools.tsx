import {Button} from '@sodeck/ui';
import {ArrowLeftRight,Folder,LayoutPanelTop,Terminal} from 'lucide-react';
import type {WorkbenchContributionsV2} from '@sodeck/sdk';
import type {Command,View} from './types';

export type Tool = WorkbenchContributionsV2['plugins'][number]['presentation']['tools'][number] & {pluginId:string;title:string};
export function contributionActions(contributions:WorkbenchContributionsV2|null,legacy:Command[]) {
  const tools:Tool[]=[],create:Command[]=[],fallback:Command[]=[];
  if(!contributions)return {tools,create,fallback:legacy.filter(c=>c.primary)};
  for(const plugin of contributions.plugins){
    for(const tool of plugin.presentation.tools)tools.push({...tool,pluginId:plugin.pluginId,title:plugin.views.find(v=>v.id===tool.viewType)?.title??plugin.displayName});
    for(const action of plugin.presentation.tabCreate){const command=plugin.commands.find(c=>c.id===action.commandId);if(command)create.push({...command,pluginId:plugin.pluginId});}
    const mapped=new Set([...plugin.presentation.tools.map(t=>t.commandId),...plugin.presentation.tabCreate.map(c=>c.commandId)]);
    fallback.push(...plugin.commands.filter(c=>c.primary&&!mapped.has(c.id)).map(c=>({...c,pluginId:plugin.pluginId})));
  }
  return {tools,create,fallback};
}
export function ToolButtons({edge,tools,views,visible,badges,disabled,onToggle,onOpen}:{edge:'left'|'right';tools:Tool[];views:View[];visible:Set<string>;badges:Record<string,number>;disabled:boolean;onToggle(view:View):void;onOpen(tool:Tool):void}){
  const declared=new Set(tools.map(t=>t.pluginId+':'+t.viewType));
  return <>{tools.filter(t=>{
    const view=views.find(v=>v.pluginId===t.pluginId&&v.viewType===t.viewType);
    return (view&&['left','right'].includes(view.placement)?view.placement:t.edge)===edge;
  }).map(tool=>{
    const view=views.find(v=>v.pluginId===tool.pluginId&&v.viewType===tool.viewType),count=view?badges[view.viewId]??0:0;
    const Icon=tool.icon==='folder'?Folder:tool.icon==='arrow-left-right'?ArrowLeftRight:tool.icon==='terminal'?Terminal:LayoutPanelTop;
    return <Button key={tool.pluginId+':'+tool.viewType} variant="quiet" size="compact" className="rail-button" disabled={disabled} aria-label={`显示或收起 ${tool.title}`} title={tool.title} aria-pressed={!!view&&visible.has(view.viewId)} onClick={()=>view?onToggle(view):onOpen(tool)}><Icon size={19}/>{count>0&&<span className="tool-badge" aria-label={`${count} 个运行中`}>{count>99?'99+':count}</span>}</Button>;
  })}{views.filter(v=>v.placement===edge&&!declared.has(v.pluginId+':'+v.viewType)).map(v=><Button variant="quiet" size="compact" key={v.viewId} className="rail-button" aria-label={`显示或收起 ${v.title}`} title={v.title} aria-pressed={visible.has(v.viewId)} onClick={()=>onToggle(v)}><LayoutPanelTop size={19}/></Button>)}</>;
}
