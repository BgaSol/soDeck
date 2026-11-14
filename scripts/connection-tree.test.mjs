import test from 'node:test';
import assert from 'node:assert/strict';
import {connectionTree,visibleNodes,ancestors} from '../src/connection-tree.ts';
const server=(id,route=[],tags=[])=>({id,name:id,host:`${id}.internal`,username:'ops',tagIds:tags,route});
const ssh=id=>({kind:'ssh',serverId:id,id:'unstable-step'});
const proxy=id=>({kind:'socks5',proxyId:id,id:'step'});
const c={servers:[server('gateway',[proxy('proxy')],['edge']),server('db',[ssh('gateway'),proxy('proxy')],['prod','db']),server('cache',[ssh('gateway'),proxy('proxy')],['prod']),server('direct')],proxies:[{id:'proxy',name:'proxy'}],tags:[{id:'prod',name:'生产'},{id:'db',name:'数据库'},{id:'edge',name:'入口'}]};
test('explicit prefixes merge, while a jump server own route is never inherited',()=>{
 const tree=connectionTree(c,{query:'',tags:[]});
 assert.equal(tree.roots.length,3);
 const root=tree.roots.find(n=>n.recordId==='gateway');
 assert.equal(root.target,false);assert.equal(root.children[0].children.length,2);
 assert.equal(root.children[0].children[0].depth,2);
 assert.equal([...tree.index.values()].filter(n=>n.recordId==='gateway').length,2);
});
test('AND matches targets themselves, retaining complete non-connectable ancestors',()=>{
 const tree=connectionTree(c,{query:'',tags:['prod','db']});
 assert.deepEqual(tree.matches.map(s=>s.id),['db']);
 const rows=visibleNodes(tree.roots,new Set([tree.roots[0].key]),true);
 assert.deepEqual(rows.map(r=>r.recordId),['gateway','proxy','db']);
 assert.deepEqual(rows.map(r=>r.target),[false,false,true]);
 assert.equal(ancestors(rows[2],tree.index).size,3);
 assert.equal(visibleNodes(tree.roots,new Set([tree.roots[0].key]),false).length,1);
});
test('search does not match descendant targets via ancestor names or tags',()=>{
 assert.deepEqual(connectionTree(c,{query:'入口',tags:[]}).matches.map(s=>s.id),['gateway']);
 assert.equal(connectionTree(c,{query:'missing',tags:[]}).roots.length,0);
});
test('keys survive record rename and route step IDs but preserve occurrence identity',()=>{
 const before=connectionTree(c,{query:'',tags:[]});
 const copy=structuredClone(c);copy.servers[0].name='renamed';copy.servers[1].route[0].id='changed';
 assert.deepEqual([...before.index.keys()].sort(),[...connectionTree(copy,{query:'',tags:[]}).index.keys()].sort());
 const missing=structuredClone(c);missing.proxies=[];
 assert.equal([...connectionTree(missing,{query:'db',tags:[]}).index.values()].find(n=>n.kind==='socks5').record,undefined);
});
