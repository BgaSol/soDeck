import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {spawnSync} from 'node:child_process';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
const python=process.env.SODECK_TEST_PYTHON;
if(!python)throw Error('Set SODECK_TEST_PYTHON to the isolated Python 3.12 executable');
const output=resolve('.qa/t024/linux');await mkdir(output,{recursive:true});
const bundled=await build({stdin:{contents:"export * from './plugins/monitor/src/collectors/scripts';export * from './plugins/monitor/src/collectors/framing';export * from './plugins/monitor/src/metrics/parse';",resolveDir:process.cwd()},bundle:true,write:false,format:'esm',platform:'node'});
const api=await import('data:text/javascript;base64,'+Buffer.from(bundled.outputFiles[0].text).toString('base64'));
const records=[];
function exec(stdin,busybox=false){
  const prefix=busybox?Buffer.from('head(){ /bin/busybox head "$@"; }; uname(){ /bin/busybox uname "$@"; }; stat(){ /bin/busybox stat "$@"; }; df(){ /bin/busybox df "$@"; };\n'):Buffer.alloc(0);
  const start=performance.now(),result=spawnSync(python,['tests/linux/t024/exec.py'],{input:JSON.stringify({stdin:Buffer.concat([prefix,stdin]).toString('base64'),busybox}),encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,PYTHONPATH:resolve('.local/python')}});
  assert.equal(result.status,0,result.stderr);const value=JSON.parse(result.stdout);assert.equal(value.exitCode,0,Buffer.from(value.stderr,'base64').toString());
  records.push({busybox,elapsedMs:performance.now()-start,bytes:Buffer.from(value.stdout,'base64').length});return Buffer.from(value.stdout,'base64');
}
for(const busybox of [false,true]){
  const token=busybox?'busybox':'gnu';
  const inventory=api.parseInventory(api.parseSections(exec(api.inventoryScript(token),busybox),token,api.INVENTORY_LIMITS));
  assert.equal(inventory.os,'Linux');assert.ok(inventory.cores>0);assert.ok(inventory.mounts.length>0);assert.equal(inventory.partial,false);
  const first=api.parseFast(api.parseSections(exec(api.fastScript(token),busybox),token,api.FAST_LIMITS),inventory);
  const second=api.parseFast(api.parseSections(exec(api.fastScript(token),busybox),token,api.FAST_LIMITS),inventory);
  assert.equal(first.partial,false,JSON.stringify(first.errors));assert.ok(second.uptime>first.uptime);assert.ok(second.memory.total>0);assert.ok(second.net.length>0);assert.ok(second.disk.length>0);
  assert.notEqual(api.cpuDifference(second.cpu,first.cpu).percent,null);
  const mount=inventory.mounts.find(m=>m.path==='/');assert.ok(mount);
  const capacity=api.parseCapacity(api.parseSections(exec(api.capacityScript(token,mount),busybox),token,{before:32768,capacity:16384,after:32768}),mount);
  assert.ok(BigInt(capacity.total)>0n);
  const fallback=Buffer.concat([Buffer.from('stat(){ return 127; };\n'),api.capacityScript(token,mount)]);
  const df=api.parseCapacity(api.parseSections(exec(fallback,busybox),token,{before:32768,capacity:16384,after:32768}),mount);assert.equal(df.source,'df');
  const malformed=Buffer.concat([Buffer.from("stat(){ printf 'malformed-success'; return 0; };\n"),api.capacityScript(token,mount)]);
  assert.equal(api.parseCapacity(api.parseSections(exec(malformed,busybox),token,{before:32768,capacity:16384,after:32768}),mount).source,'df');
  const special=inventory.mounts.find(m=>m.rawPath.startsWith('/var/tmp/sodeck-monitor-special/'));assert.ok(special,'run special-mount.py setup first');
  assert.ok(special.rawPath.includes('\n')&&special.rawPath.includes("'")&&special.rawPath.includes('\xff'));
  const specialCapacity=api.parseCapacity(api.parseSections(exec(api.capacityScript(token,special),busybox),token,{before:32768,capacity:16384,after:32768}),special);assert.equal(specialCapacity.source,'stat');
  await writeFile(join(output,`${token}-evidence.json`),JSON.stringify({kernel:inventory.kernel,cores:inventory.cores,objects:{mounts:inventory.mounts.length,interfaces:first.net.length,devices:first.disk.length},memory:first.memory,capacity,df,checks:['complete framed inventory','two fast samples','CPU delta','memory','disk and network counters','stat capacity','df fallback','malformed successful stat falls back to df','lossless space, quote, newline and invalid UTF-8 mount path']},null,2));
}
await writeFile(join(output,'collection-results.json'),JSON.stringify(records,null,2));console.log('GNU/Linux and BusyBox real SSH collection passed',records.length,'executions');
