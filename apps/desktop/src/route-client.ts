import {core,type RouteMeta,type RouteSummary,type ServerPage} from '@sodeck/sdk';
import type {Server} from './types';

export type DisplayRoute=RouteMeta|RouteSummary;
export const routeLabel=(route:DisplayRoute)=>'nodes' in route
  ?['本机',...route.nodes.map(n=>`${n.name} (${n.username}@${n.host}:${n.port})`)].join(' → ')
  :`${route.targetName} · ${route.nodeCount>1?`经由 ${route.nodeCount-1} 台跳板`:'直接连接'}`;

export async function loadServers(version:0|1|2):Promise<{servers:Server[]}> {
  if(version!==2)return core.call(version===1?'servers.routes.list':'servers.list');
  const servers:Server[]=[];let cursor:string|null=null;
  do {const page:ServerPage=await core.call('servers.routes.v2.list',{cursor});servers.push(...page.servers);cursor=page.nextCursor;}while(cursor);
  return {servers};
}

/** References point towards the entry host; an invalid candidate stays unavailable. */
export function serverPath(servers:readonly Server[],id:string,excluded?:string):Server[]|null {
  const byId=new Map(servers.map(s=>[s.id,s]));const visited=new Set<string>();const path:Server[]=[];
  let next:string|undefined=id;
  while(next){
    if(next===excluded||visited.has(next))return null;
    visited.add(next);const node:Server|undefined=byId.get(next);if(!node)return null;
    path.push(node);next=node.connectionRoute?.kind==='sshJump'?node.connectionRoute.jumpServerId:undefined;
  }
  return path.reverse();
}
