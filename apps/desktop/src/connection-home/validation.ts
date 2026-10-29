import {generatedName,type Draft,type Catalog} from './model';
export function validate(d:Draft,c:Catalog):Record<string,string>{
  const f=d.fields,e:Record<string,string>={};
  if(!f.host.trim()||f.host.trim().length>253||/[\s/\\]/.test(f.host.trim())||f.host.includes('://'))e.host='填写域名、IPv4 或 IPv6 地址，不包含协议和路径';
  if(!/^\d+$/.test(f.port)||Number(f.port)<1||Number(f.port)>65535)e.port='端口应为 1–65535 的整数';
  if((d.kind==='ssh'||f.authType==='usernamePassword')&&!f.username.trim())e.username='请填写用户名';
  if(f.authType==='usernamePassword'&&new TextEncoder().encode(f.username).length>255)e.username='代理用户名最多 255 字节';
  const name=f.name.trim()||generatedName(d);if([...name].length>200)e.name='名称最多 200 个字符，请填写较短的名称';
  if(d.kind==='ssh'){
    if(f.groupId&&!c.groups.some(g=>g.id===f.groupId))e.groupId='此分组已删除，请重新选择';
    if(f.pathEnabled){
      const ssh=f.steps.filter(s=>s.kind==='ssh');
      if(!f.steps.length||ssh.some(s=>s.serverId===d.server?.server.id||!c.items.some(i=>i.server.id===s.serverId))||new Set(ssh.map(s=>s.serverId)).size!==ssh.length||f.steps.some(s=>s.kind==='socks5'&&!c.proxies.some(i=>i.proxy.id===s.proxyId)))e.path='请按顺序选择已保存的中转节点，SSH 节点不能重复或包含目标自身';
    }
    if(d.inlineId)e.path='请先保存或取消正在添加的代理';
  }
  return e;
}
