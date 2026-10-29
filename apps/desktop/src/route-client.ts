import {core,type RouteMeta,type ServerPage} from '@sodeck/sdk';
import type {Server} from './types';
export type DisplayRoute=RouteMeta;
export const routeLabel=(route:DisplayRoute)=>`${route.targetName} · ${route.stepCount>1?`经由 ${route.sshCount-1} 个 SSH · ${route.proxyCount} 个 SOCKS5`:'直接连接'}`;
export async function loadServers():Promise<{servers:Server[]}> {
  const servers:Server[]=[];let cursor:string|null=null;
  do {const page:ServerPage=await core.call('servers.routes.list',{cursor});servers.push(...page.servers);cursor=page.nextCursor;}while(cursor);
  return {servers};
}
