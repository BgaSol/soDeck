import {connectionDiagnosticsSchema as wire,type DiagnosticRef,type DiagnosticSummary,type DiagnosticList,type DiagnosticPhase,type DiagnosticError} from './connection-diagnostics.generated';
import {validate,type Shape} from './schema';
export type {DiagnosticRef,DiagnosticSummary,DiagnosticList,DiagnosticPhase,DiagnosticError,DiagnosticPhases} from './connection-diagnostics.generated';
const schema=wire as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
export function validateConnectionDiagnostics(kind:'x-methods'|'x-results',method:string,value:unknown){const name=schema[kind][method];return name?validate(schema,schema.$defs[name],value,method):undefined;}
export function decodeDiagnosticList(value:unknown):DiagnosticList {const e=validateConnectionDiagnostics('x-results','connections.diagnostics.list',value);if(e)throw Error(e);return value as DiagnosticList;}
export function diagnosticReference(error:unknown):DiagnosticRef|undefined {
  const value=(error as {details?:{diagnosticRef?:unknown}})?.details?.diagnosticRef;
  return value&&!validate(schema,schema.$defs.DiagnosticRef,value,'diagnosticRef')?value as DiagnosticRef:undefined;
}
export function openConnectionDiagnostics(bridge:{call<T=unknown>(method:string,params:unknown):Promise<T>},reference:DiagnosticRef){return bridge.call('ui.connectionDiagnostics.open',{reference});}
export const diagnosticStage:Record<string,string>={prepare:'准备连接路径',queue:'等待连接调度',admit:'接纳连接资源',resolveCached:'使用本次地址候选缓存',resolve:'解析地址',connect:'连接端点',forward:'建立 SSH 转发通道',handshake:'SSH 握手',verify:'核验主机身份',credentials:'选择登录凭据',authenticate:'SSH 登录',proxyCredentials:'选择代理凭据',proxyNegotiate:'SOCKS5 方法协商',proxyAuthenticate:'代理账号认证',proxyConnect:'代理连接下一站',deliver:'交付连接',candidate:'地址候选'};
export const diagnosticState:Record<string,string>={running:'进行中',waiting:'等待确认',succeeded:'已完成',failed:'失败',canceled:'已取消',superseded:'已继续至下一次尝试',pending:'等待执行',notRun:'未执行',notApplicable:'无需执行'};
export function diagnosticCause(error:DiagnosticError|null):string {
  if(!error)return '';
  const names:Record<string,string>={JUMP_FORWARD_FAILED:'SSH 端无法打开转发通道',CONNECTION_INTERRUPTED:'连接执行中断',PROXY_DISCONNECTED:'收到完整 SOCKS5 回复前通道关闭',PROXY_NETWORK_ERROR:'SOCKS5 通道中断',PROXY_AUTH_METHOD:'代理不接受所选认证方式',PROXY_PROTOCOL_ERROR:'SOCKS5 回复格式无效',PROXY_AUTH_FAILED:'代理账号认证失败',AUTH_FAILED:'SSH 登录失败',HOST_KEY_CHANGED:'主机密钥发生变化，等待核对',HOST_CHALLENGE_EXPIRED:'身份与登录确认已过期，请重新连接',HOST_KEY_REQUIRED:'等待确认主机身份',AUTH_REQUIRED:'等待输入登录凭据',CREDENTIAL_CONFIRM_REQUIRED:'等待确认已保存凭据',PROXY_AUTH_REQUIRED:'等待代理凭据',CONNECT_TIMEOUT:'连接超时',PROXY_TIMEOUT:'SOCKS5 当前阶段超时',CANCELED:'操作已取消',REVISION_CONFLICT:'连接配置已变化',ROUTE_CHANGED:'连接路径已变化',LIMIT_EXCEEDED:'资源已达到上限',NETWORK_ERROR:'网络连接未能建立',SSH_ERROR:'SSH 协议操作未完成'};
  const replies=['成功','一般失败','规则拒绝','网络不可达','主机不可达','连接被拒绝','TTL 超时','命令不支持','地址类型不支持'];
  if(error.code==='PROXY_CONNECT_FAILED'&&error.reply!==undefined)return `代理返回：${replies[error.reply]??'未知错误'}（REP ${error.reply}）`;
  return names[error.code]??`操作未完成（${error.code}）`;
}
export function diagnosticSummary(value:DiagnosticSummary):string {
  if(value.reused&&value.state==='succeeded')return '复用已有连接';
  if(value.state==='superseded')return '确认后已继续至下一次尝试';
  const e=value.error;
  if(!e)return diagnosticState[value.state]??value.state;
  const node=e.stepIndex===undefined?'':`第 ${e.stepIndex+1}${value.route?`/${value.route.stepCount}`:''} 站 · `;
  return `${node}${e.stage?`${diagnosticStage[e.stage]??e.stage}：`:''}${diagnosticCause(e)}`;
}
export function diagnosticFaultMessage(error:unknown):string|undefined {
  if(!diagnosticReference(error))return;
  const e=error as {code:string;details?:DiagnosticError&{stepCount?:number}};
  const d=e.details;return `${d?.stepIndex===undefined?'':`第 ${d.stepIndex+1}${d.stepCount?`/${d.stepCount}`:''} 站 · `}${d?.stage?`${diagnosticStage[d.stage]??d.stage}：`:''}${diagnosticCause({code:e.code,...d})}`;
}
export function diagnosticPhaseText(phase:DiagnosticPhase){return `${diagnosticStage[phase.stage]??phase.stage}：${diagnosticState[phase.state]??phase.state}${phase.error?' · '+diagnosticCause(phase.error):''}`;}

export function sameDiagnostic(a?:DiagnosticRef,b?:DiagnosticRef){return !!a&&!!b&&a.bootId===b.bootId&&a.attemptId===b.attemptId&&a.attemptGeneration===b.attemptGeneration;}
