import type {RouteStatus} from './connection-route.generated';

/** Allow the core's complete connection attempt plus command completion time. */
export async function connectionCommandTimeout(
  bridge: {call<T = unknown>(method:string,params?:unknown):Promise<T>},
  serverId?:string,
  workspaceId=serverId,
):Promise<number> {
  if(!serverId||!workspaceId)throw Object.assign(Error('缺少连接目标或工作区，无法读取连接预算'),{code:'INVALID_PARAMS'});
  const capabilities=await bridge.call<{capabilities:string[]}>('capabilities.list');
  if(!capabilities.capabilities.includes('connections.route.v2'))throw Object.assign(Error('核心不支持 API6 连接路径预算'),{code:'UNSUPPORTED_API'});
  const status=await bridge.call<RouteStatus>('connections.route.get',{serverId,workspaceId});
  const budget=status.saved?.attemptTimeoutMs;
  if(!Number.isSafeInteger(budget)||budget<120_000||budget>900_000)throw Object.assign(Error('核心返回的连接预算无效'),{code:'PROTOCOL_ERROR'});
  return budget+60_000;
}
