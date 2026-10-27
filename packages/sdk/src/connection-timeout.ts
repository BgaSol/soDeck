import type {RouteStatusV2} from './connection-route-v2.generated';

/** Allow the core's complete connection attempt plus command completion time. */
export async function connectionCommandTimeout(
  bridge: {call<T = unknown>(method:string,params?:unknown):Promise<T>},
  serverId?:string,
  workspaceId=serverId,
):Promise<number> {
  if(!serverId||!workspaceId)return 180_000;
  const capabilities=await bridge.call<{capabilities:string[]}>('capabilities.list');
  if(!capabilities.capabilities.includes('connections.route.v2'))return 180_000;
  const status=await bridge.call<RouteStatusV2>('connections.route.v2.get',{serverId,workspaceId});
  return 180_000+75_000*Math.max(0,status.saved.nodeCount-2);
}
