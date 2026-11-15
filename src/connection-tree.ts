import type {Catalog, Server, Proxy} from './server-api';

export interface ConnectionNode {
  key:string; parent:string|null; recordId:string; kind:'ssh'|'socks5'; record:Server|Proxy|undefined;
  target:boolean; children:ConnectionNode[]; depth:number;
}
export interface TreeFilter {query:string;tags:string[]}
export const pathKey=(path:readonly (readonly [string,string])[])=>JSON.stringify(path);
export function connectionTree(catalog:Catalog,filter:TreeFilter) {
  const roots:ConnectionNode[]=[], index=new Map<string,ConnectionNode>();
  const q=filter.query.trim().toLocaleLowerCase();
  const serverById=new Map(catalog.servers.map(s=>[s.id,s])),proxyById=new Map(catalog.proxies.map(p=>[p.id,p]));
  const queryMatches=(record:Server|Proxy|undefined)=>!!record&&[record.name,record.host,
    'username' in record?record.username:record.authentication?.username??'',
    ...('tagIds' in record?record.tagIds.map(id=>catalog.tags.find(t=>t.id===id)?.name??''):[])].join(' ').toLocaleLowerCase().includes(q);
  const referencedProxies=new Set(catalog.servers.flatMap(s=>s.route.flatMap(step=>step.kind==='socks5'?[step.proxyId]:[])));
  const matches=catalog.servers.filter(s=>filter.tags.every(id=>s.tagIds.includes(id)) &&
    [s,...s.route.map(step=>step.kind==='ssh'?serverById.get(step.serverId):proxyById.get(step.proxyId))].some(queryMatches));
  for(const server of matches) {
    const path:[string,string][]=[];
    let siblings=roots,parent:string|null=null;
    const steps=[...server.route.map(step=>({kind:step.kind,id:step.kind==='ssh'?step.serverId:step.proxyId})),{kind:'ssh' as const,id:server.id}];
    steps.forEach((step,depth)=>{
      path.push([step.kind,step.id]);const key=pathKey(path);
      let node=index.get(key);
      if(!node){node={key,parent,recordId:step.id,kind:step.kind,record:step.kind==='ssh'?serverById.get(step.id):proxyById.get(step.id),target:false,children:[],depth};index.set(key,node);siblings.push(node);}
      if(depth===steps.length-1)node.target=true;
      siblings=node.children;parent=key;
    });
  }
  const sort=(nodes:ConnectionNode[])=>{nodes.sort((a,b)=>(a.record?.name??a.recordId).localeCompare(b.record?.name??b.recordId,'zh-CN',{numeric:true}));nodes.forEach(n=>sort(n.children));};
  sort(roots);
  const standalone=catalog.proxies.filter(p=>!referencedProxies.has(p.id)&&!filter.tags.length&&queryMatches(p)).sort((a,b)=>a.name.localeCompare(b.name,'zh-CN',{numeric:true}));
  for(const record of standalone){const key=pathKey([['socks5',record.id]]),node:ConnectionNode={key,parent:null,recordId:record.id,kind:'socks5',record,target:false,children:[],depth:0};roots.push(node);index.set(key,node);}
  return {roots,index,matches};
}
export function visibleNodes(roots:ConnectionNode[],collapsed:ReadonlySet<string>,filtered:boolean):ConnectionNode[] {
  const rows:ConnectionNode[]=[];
  const visit=(nodes:ConnectionNode[])=>nodes.forEach(node=>{rows.push(node);if(filtered||!collapsed.has(node.key))visit(node.children);});visit(roots);return rows;
}
export function ancestors(node:ConnectionNode|undefined,index:ReadonlyMap<string,ConnectionNode>) {
  const keys=new Set<string>();while(node){keys.add(node.key);node=node.parent?index.get(node.parent):undefined;}return keys;
}
