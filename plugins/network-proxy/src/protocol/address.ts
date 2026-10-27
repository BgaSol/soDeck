export const fault=(code:string,message:string)=>Object.assign(new Error(message),{code});
export interface Target {host:string;port:number}
export function validHost(host:string):boolean {
  if(!host||host.length>255||/[^\x21-\x7e]/.test(host)||/[\/\\@?#\[\]%]/.test(host))return false;
  if(host.includes(':')) {
    if(host.includes('.') ) {
      const at=host.lastIndexOf(':'),v4=host.slice(at+1).split('.');
      if(v4.length!==4||!v4.every(p=>/^\d{1,3}$/.test(p)&&Number(p)<=255))return false;
      host=host.slice(0,at+1)+'0:0';
    }
    if(!/^[0-9a-f:]+$/i.test(host)||host.includes(':::')||host.split('::').length>2||host.startsWith(':')&&!host.startsWith('::')||host.endsWith(':')&&!host.endsWith('::'))return false;
    const parts=host.split(':').filter(Boolean);
    return parts.every(p=>p.length<=4)&&(host.includes('::')?parts.length<8:parts.length===8);
  }
  return /^[a-z0-9](?:[a-z0-9.-]*[a-z0-9.])?$/i.test(host)&&!host.includes('..');
}
export function authority(text:string,defaultPort?:number):Target {
  let host:string,portText:string|undefined;
  if(text.startsWith('[')) {
    const match=/^\[([^\]]+)\](?::([0-9]+))?$/.exec(text);if(!match)throw fault('BAD_REQUEST','IPv6 目标格式无效');
    host=match[1];portText=match[2];if(!host.includes(':'))throw fault('BAD_REQUEST','IPv6 目标格式无效');
  } else {
    const match=/^([^:]+)(?::([0-9]+))?$/.exec(text);if(!match)throw fault('BAD_REQUEST','目标地址格式无效');
    host=match[1];portText=match[2];
  }
  const port=portText===undefined?defaultPort:Number(portText);
  if(!validHost(host)||!Number.isInteger(port)||port!<1||port!>65535)throw fault('BAD_REQUEST','目标主机或端口无效');
  return {host:host.toLowerCase(),port:port!};
}
export function formatTarget(target:Target){return `${target.host.includes(':')?`[${target.host}]`:target.host}:${target.port}`;}
export function sameTarget(a:Target,b:Target){return a.host===b.host&&a.port===b.port;}
export function requestTarget(text:string,host:string|undefined):{target:Target;path:string} {
  if(!host)throw fault('BAD_REQUEST','请求缺少 Host');
  if(text.startsWith('/')) {if(/[\x00-\x20\x7f#]/.test(text))throw fault('BAD_REQUEST','请求路径无效');return {target:authority(host,80),path:text};}
  const match=/^http:\/\/([^/?#]+)([^#]*)$/i.exec(text);
  if(!match)throw fault('BAD_REQUEST','HTTP 代理需要 http 地址，HTTPS 请使用 CONNECT');
  const target=authority(match[1],80);if(!sameTarget(target,authority(host,80)))throw fault('BAD_REQUEST','Host 与请求目标不一致');
  const path=match[2].startsWith('?')?'/'+match[2]:match[2]||'/';
  if(/[\x00-\x20\x7f]/.test(path))throw fault('BAD_REQUEST','请求路径无效');
  return {target,path};
}
