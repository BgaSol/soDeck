import type {ConnectionView, ConnectionsUi} from './connections';
import {routeTo, type ConnectionNode, type ConnectionTree} from './connection-tree.ts';

export type ConnectionActions=Pick<ConnectionsUi,'connect'|'cancelAttempt'|'continueAttempt'|'focusWorkspace'>;
export type ServerCommand=
  | {kind:'new-server'|'new-proxy'|'reload'|'share-all'|'import-share'}
  | {kind:'new-via';path:string}
  | {kind:'edit'|'copy'|'delete'|'locate-target'|'share';id:string;path?:string}
  | {kind:'connect';id:string;path:string}
  | {kind:'focus';id:string;attempt:string}
  | {kind:'cancel-attempt';attempt:string}
  | {kind:'continue-attempt';attempt:string;question:string};
export interface ServerAction {command:ServerCommand;label:string;group:'connection'|'create'|'record'|'delete';disabled?:string;danger?:boolean}
export function serverActions(node:ConnectionNode|undefined,tree:ConnectionTree,connection:ConnectionView|undefined,unavailable=''):ServerAction[]{
  if(!node)return [
    {command:{kind:'new-server'},label:'新增 SSH 服务器',group:'create',disabled:unavailable},
    {command:{kind:'new-proxy'},label:'新增 SOCKS5 代理',group:'create',disabled:unavailable},
    {command:{kind:'reload'},label:'刷新目录',group:'record'},
    {command:{kind:'share-all'},label:'分享全部服务器',group:'record',disabled:unavailable},
  ];
  if(!node.record)return [{command:{kind:'reload'},label:'刷新目录',group:'record'}];
  const id=node.recordId,path=node.key,actions:ServerAction[]=[];
  if(node.kind==='ssh'){
    if(!node.target)actions.push({command:{kind:'locate-target',id,path},label:'查看服务器目标',group:'connection'});
    else if(connection?.state==='connecting'){
      if(connection.question)actions.push({command:{kind:'continue-attempt',attempt:connection.attemptId,question:connection.question.id},label:'继续确认',group:'connection'});
      actions.push({command:{kind:'cancel-attempt',attempt:connection.attemptId},label:'取消连接',group:'connection'});
    }else if(connection?.state==='connected')actions.push({command:{kind:'focus',id,attempt:connection.attemptId},label:'前往工作台',group:'connection'});
    else actions.push({command:{kind:'connect',id,path},label:connection?'重新连接':'连接服务器',group:'connection',disabled:unavailable});
  }
  let invalid=unavailable;try{routeTo(tree,path);}catch(error){invalid=String(error).replace(/^Error: /,'');}
  actions.push({command:{kind:'new-via',path},label:'经此节点新增服务器',group:'create',disabled:invalid},
    {command:{kind:'edit',id,path},label:connection?.state==='failed'?'编辑连接资料':'编辑资料',group:'record',disabled:unavailable},
    {command:{kind:'copy',id,path},label:'复制资料',group:'record',disabled:unavailable},
    {command:{kind:'delete',id,path},label:'删除资料',group:'delete',danger:true,disabled:unavailable||(connection?.state==='connecting'?'请先取消连接并等待结束':'')});
  if(node.kind==='ssh')actions.splice(actions.length-1,0,{command:{kind:'share',id,path},label:'分享连接',group:'record',disabled:unavailable});
  return actions;
}
export const actionKey=(command:ServerCommand)=>command.kind;
