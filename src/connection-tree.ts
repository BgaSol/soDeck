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
  const matches=catalog.servers.filter(s=>filter.tags.every(id=>s.tagIds.includes(id)) &&
    [s.name,s.host,s.username,...s.tagIds.map(id=>catalog.tags.find(t=>t.id===id)?.name??'')].join(' ').toLocaleLowerCase().includes(q));
  for(const server of matches) {
    const path:[string,string][]=[];
    let siblings=roots,parent:string|null=null;
    const steps=[...server.route.map(step=>({kind:step.kind,id:step.kind==='ssh'?step.serverId:step.proxyId})),{kind:'ssh' as const,id:server.id}];
    steps.forEach((step,depth)=>{
      path.push([step.kind,step.id]);const key=pathKey(path);
      let node=index.get(key);
      if(!node){node={key,parent,recordId:step.id,kind:step.kind,record:step.kind==='ssh'?catalog.servers.find(s=>s.id===step.id):catalog.proxies.find(p=>p.id===step.id),target:false,children:[],depth};index.set(key,node);siblings.push(node);}
      if(depth===steps.length-1)node.target=true;
      siblings=node.children;parent=key;
    });
  }
  const sort=(nodes:ConnectionNode[])=>{nodes.sort((a,b)=>(a.record?.name??a.recordId).localeCompare(b.record?.name??b.recordId,'zh-CN',{numeric:true}));nodes.forEach(n=>sort(n.children));};
  sort(roots);
  return {roots,index,matches};
}
export function visibleNodes(roots:ConnectionNode[],collapsed:ReadonlySet<string>,filtered:boolean):ConnectionNode[] {
  const rows:ConnectionNode[]=[];
  const visit=(nodes:ConnectionNode[])=>nodes.forEach(node=>{rows.push(node);if(filtered||!collapsed.has(node.key))visit(node.children);});visit(roots);return rows;
}
export function ancestors(node:ConnectionNode|undefined,index:ReadonlyMap<string,ConnectionNode>) {
  const keys=new Set<string>();while(node){keys.add(node.key);node=node.parent?index.get(node.parent):undefined;}return keys;
}
