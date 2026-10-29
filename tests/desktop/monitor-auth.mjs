import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {monitorHarness,snapshot} from './monitor-harness.mjs';
import {rpc,until,delay} from './harness.mjs';
const h=await monitorHarness('auth',19061),checks=[];let failure;
// No terminal initializer: this proves monitor's own explicit authentication.
process.env.SODECK_PLUGIN_PACKAGES=resolve('.qa/t024/packages/monitor.sodeck-plugin');
try{
 const app=await h.start(),server=await rpc(app.page,'servers.create',{name:'T024 explicit auth',host:'127.0.0.1',port:22342,username:'tester',authType:'key'});
 await rpc(app.page,'workspaces.open',{serverId:server.id});const page=await h.view('sodeck.monitor',server.id);await until(()=>snapshot(page));
 await delay(1500);assert.equal(await app.page.getByRole('dialog').count(),0);
 await page.getByRole('button',{name:'连接并监控',exact:true}).click();await app.page.getByRole('dialog').waitFor();await app.page.getByRole('dialog').getByRole('button',{name:'取消',exact:true}).click();
 await delay(6000);assert.equal(await app.page.getByRole('dialog').count(),0);assert.notEqual((await snapshot(page)).connection,'connected');checks.push('explicit monitor action enters core authentication; cancel does not trigger automatic authentication');
 await page.getByRole('button',{name:'连接并监控',exact:true}).click();
 await app.page.getByRole('button',{name:'信任并连接',exact:true}).click();
 await app.page.getByLabel('私钥文件路径',{exact:true}).fill(resolve('.qa/t008/linux/test-key'));await app.page.getByRole('button',{name:'连接',exact:true}).click();
 await until(async()=>(await snapshot(page)).cpu.quality==='fresh',45000);await until(async()=>/\d+\.\d+%/.test(await page.locator('.monitor-heading').first().innerText()),5000);await page.screenshot({path:join(h.output,'authenticated-monitor.png')});checks.push('new explicit action completes key authentication and target host trust then samples real Linux');
 await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).leases.length===0,15000);
}catch(error){failure={message:error.message,stack:error.stack,body:await h.app.page.locator('body').innerText()};throw error;}
finally{await writeFile(join(h.output,'results.json'),JSON.stringify({checks,failure},null,2));await h.stop();}
