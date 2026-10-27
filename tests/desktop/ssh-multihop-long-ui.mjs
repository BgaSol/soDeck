import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {launch,rpc,until,stop} from './harness.mjs';
const output=resolve('.qa/t013-multihop/visual');await mkdir(output,{recursive:true});let app;
try{
 app=await launch({data:join(output,'long-data-'+Date.now()),shellPort:9911,pluginPort:9912});let last;
 for(let i=0;i<70;i++)last=await rpc(app.page,'servers.routes.v2.create',{name:`路径节点 ${i+1} · 中文长名称`,host:'2001:db8::1',username:'fixture',connectionRoute:last?{kind:'sshJump',jumpServerId:last.id}:{kind:'direct'}});
 const state=await rpc(app.page,'connections.route.v2.get',{serverId:last.id,workspaceId:last.id});
 const first=await rpc(app.page,'connections.route.v2.nodes',{reference:state.saved.reference});assert.equal(first.nodes.length,64);
 const second=await rpc(app.page,'connections.route.v2.nodes',{reference:state.saved.reference,cursor:first.nextCursor});assert.equal(second.offset,64);assert.equal(second.nodes.length,6);
 await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
 await app.page.getByRole('button',{name:`编辑 ${last.name}`,exact:true}).click();
 await app.page.getByText('查看完整路径',{exact:true}).click();
 await until(async()=>await app.page.locator('.route-node-list li').count()===69);
 await app.page.locator('.route-node-list li').last().scrollIntoViewIfNeeded();
 await app.page.screenshot({path:join(output,'long-path.png')});
 assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,0);
 assert.deepEqual(app.errors,[]);
 await writeFile(join(output,'long-results.json'),JSON.stringify({passed:true,nodes:70,pages:[64,6],previewJumps:69,ipv6:true,networkConnections:0},null,2));
}finally{await stop(app);}
