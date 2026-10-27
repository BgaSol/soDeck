import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
import {launch,rpc,stop} from './harness.mjs';
const output=resolve('.qa/t014/compatibility');await mkdir(output,{recursive:true});const checks=[];
const oldCore=resolve('.local/dev/SoDeck-v2-T013-Multihop/sodeck-desktop.exe'),digest=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
const baseline={core:await digest(oldCore),terminal:await digest('.qa/t013/baseline/terminal.sodeck-plugin'),files:await digest('.qa/t013/baseline/files.sodeck-plugin')};let app;
try{
 process.env.SODECK_PLUGIN_PACKAGES=resolve('resources/network-proxy.sodeck-plugin');process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.network-proxy':9962});
 app=await launch({executable:oldCore,data:join(output,'old-core-'+Date.now()),shellPort:9961,pluginPort:9962,packagePath:resolve('resources/network-proxy.sodeck-plugin'),allowStartupWarning:true});
 const info=await rpc(app.page,'app.info'),plugins=await rpc(app.page,'plugins.list');assert.ok(!plugins.some(p=>p.pluginId==='sodeck.network-proxy'&&p.activeVersion));assert.match(info.startupError??'',/API|capabilit|兼容|版本/i);await writeFile(join(output,'old-core-rejection.json'),JSON.stringify({info,plugins},null,2));await stop(app);app=undefined;
 checks.push('Actual API 4.6 core rejects the network 4.7 plugin before activation');
 const result=await promisify(execFile)(process.execPath,['tests/desktop/ssh-multihop.mjs'],{windowsHide:true,timeout:180000,env:{...process.env,MH_OLD:'1',MH_CONFIG:resolve('.qa/t014/linux/config.json'),MH_OUTPUT:join(output,'old-plugins')}});await writeFile(join(output,'old-plugins.log'),result.stdout+result.stderr);checks.push('Unrebuilt terminal 0.6.0 and files 0.3.2 perform real OpenSSH terminal, SFTP and exec through four nodes on the new core');
 assert.deepEqual({core:await digest(oldCore),terminal:await digest('.qa/t013/baseline/terminal.sodeck-plugin'),files:await digest('.qa/t013/baseline/files.sodeck-plugin')},baseline);
}finally{await stop(app);await writeFile(join(output,'results.json'),JSON.stringify({checks,baseline},null,2));console.log(checks.join('\n'));}
