import test from 'node:test';
import assert from 'node:assert/strict';
import {connectionTree,filterTree,pathKey,routeTo,targetPath} from '../src/connection-tree.ts';
import {serverActions} from '../src/server-actions.ts';
import {newViaDraft,toInput} from '../src/server-api.ts';

const ssh=(id,route=[],tagIds=[])=>({id,protocol:'ssh',name:id,host:`${id}.internal`,port:22,username:'ops',tagIds,route,authentication:{type:'password',hasSavedSecret:true,hasSessionSecret:false}});
const hop=id=>({id:'old-step',kind:'ssh',serverId:id});
const proxy=id=>({id:'old-proxy-step',kind:'socks5',proxyId:id});
const catalog=()=>({servers:[ssh('A'),ssh('B',[hop('A')],['prod']),ssh('C',[proxy('P'),hop('A')])],proxies:[{id:'P',protocol:'socks5',name:'P',host:'proxy',port:1080,authentication:{type:'none'}}],tags:[{id:'prod',name:'生产'}]});
test('filtering keeps the canonical role and occurrence while inheriting only the clicked prefix',()=>{
  const c=catalog(),full=connectionTree(c),view=filterTree(full,c,{query:'',tags:['prod']}),a=targetPath(c.servers[0]);
  assert.equal(view.index.get(a).target,true);assert.equal(view.index.get(a).children[0].target,true);
  const other=pathKey([['socks5','P'],['ssh','A']]);
  assert.equal(full.index.get(other).target,false);
  assert.deepEqual(routeTo(full,a).map(s=>s.kind==='ssh'?s.serverId:s.proxyId),['A']);
  assert.deepEqual(routeTo(full,other).map(s=>s.kind==='ssh'?s.serverId:s.proxyId),['P','A']);
  c.servers[0].route=[proxy('P')];
  assert.deepEqual(routeTo(connectionTree(c),other).map(s=>s.kind==='ssh'?s.serverId:s.proxyId),['P','A'],'A own route is not recursively inserted');
  assert.throws(()=>routeTo(full,'missing'),/位置已变化/);
});
test('new via drafts inherit references and filter tags without source credentials or shared step objects',()=>{
  const c=catalog(),tree=connectionTree(c),route=routeTo(tree,targetPath(c.servers[2])),draft=newViaDraft(route,['prod']);
  assert.equal(draft.id,null);assert.equal(draft.host,'');assert.equal(draft.username,'');assert.equal(draft.password,'');assert.equal(draft.privateKeyPath,'');assert.equal(draft.copySource,undefined);
  assert.equal(draft.routed,true);assert.deepEqual(draft.tagIds,['prod']);assert.equal(new Set(draft.route.map(r=>r.id)).size,3);
  draft.route[0].proxyId='changed';assert.equal(route[0].proxyId,'P');
  assert.equal(toInput(draft,false).copyFromId,undefined);assert(draft.route.every(r=>r.id));assert(toInput(draft,false).route.every(r=>r.id===''));
});
test('actions share role, live attempt identity and directory availability rules',()=>{
  const c=catalog(),tree=connectionTree(c),node=tree.index.get(targetPath(c.servers[0]));
  const actions=(state,question=null,error='')=>serverActions(node,tree,state?{serverId:'A',attemptId:'attempt-1',state,question}:undefined,error);
  assert.equal(actions()[0].command.kind,'connect');assert.equal(actions('failed')[0].label,'重新连接');
  assert.equal(actions('connected',null,'offline')[0].command.kind,'focus');assert(!actions('connected',null,'offline')[0].disabled);
  assert.equal(actions('connecting',{id:'question-1'})[0].command.question,'question-1');
  assert.equal(actions('connecting')[0].command.attempt,'attempt-1');assert.match(actions('connecting').at(-1).disabled,/取消/);
  assert.equal(actions(undefined,null,'offline')[0].disabled,'offline');
  const middle=tree.index.get(pathKey([['socks5','P'],['ssh','A']]));
  assert.equal(serverActions(middle,tree,undefined)[0].command.kind,'locate-target');
  const p=tree.index.get(pathKey([['socks5','P']]));assert.equal(serverActions(p,tree,undefined)[0].command.kind,'new-via');
  assert(!serverActions(p,tree,undefined).some(a=>['connect','cancel-attempt','collapse'].includes(a.command.kind)));
  const missing=structuredClone(c);missing.proxies=[];const broken=connectionTree(missing);
  assert.match(serverActions(broken.index.get(middle.key),broken,undefined).find(a=>a.command.kind==='new-via').disabled,/不存在/);
});
