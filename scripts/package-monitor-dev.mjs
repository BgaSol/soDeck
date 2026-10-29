// Build a new isolated directory from a frozen, verified T-024 runtime.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..');
const output=resolve(process.argv[2]??join(root,'.local/dev/SoDeck-v2-T024'));
const preflight=process.argv[3]==='--preflight';
const runtime=resolve(process.env.SODECK_MONITOR_RUNTIME_DIR??join(root,'.qa/t024/runtime-monitor-r7'));
const baseline=JSON.parse(await readFile(join(runtime,'baseline.json'),'utf8'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
if(![join(root,'.local/dev')+sep,join(root,'.qa/t024')+sep].some(base=>output.startsWith(base)))throw Error('Use a new isolated T024 delivery directory');
if(!preflight){const load=JSON.parse(await readFile(join(root,'.qa/t024/load/summary.json'),'utf8'));if(!load.passed||load.elapsedMs<1800000)throw Error('Final delivery requires the completed 30-minute load result');if(load.fixtureBaseline.productionPlugin!==hash(await readFile(join(root,'.qa/t024/packages/monitor.sodeck-plugin')))||load.fixtureBaseline.runtime.hashes['sodeck-desktop.exe']!==baseline.hashes['sodeck-desktop.exe'])throw Error('Final delivery must match the tested monitor and core');}
for(const name of ['sodeck-desktop.exe','sodeck-plugin-host.exe'])if(hash(await readFile(join(runtime,name)))!==baseline.hashes[name])throw Error('Frozen runtime hash changed: '+name);
// pack-plugin uses stored ZIP entries; read the actual archive manifest rather
// than assigning a version from concurrent, possibly newer source files.
function manifest(bytes){
  for(let offset=0;offset+30<=bytes.length&&bytes.readUInt32LE(offset)===0x04034b50;){
    const size=bytes.readUInt32LE(offset+18),length=bytes.readUInt16LE(offset+26),extra=bytes.readUInt16LE(offset+28),start=offset+30+length+extra;
    if(bytes.readUInt16LE(offset+8)!==0||start+size>bytes.length)throw Error('Expected bounded stored plugin archive');
    if(bytes.toString('utf8',offset+30,offset+30+length)==='manifest.json')return JSON.parse(bytes.subarray(start,start+size));
    offset=start+size;
  }
  throw Error('Archive manifest missing');
}
const mappings=[['terminal','sodeck.terminal'],['files','sodeck.files'],['network-proxy','sodeck.network-proxy'],['monitor','sodeck.monitor']];
const packages=[];
for(const [directory,pluginId]of mappings){const bytes=await readFile(join(root,'.qa/t024/packages',directory+'.sodeck-plugin')),m=manifest(bytes);if(m.pluginId!==pluginId)throw Error('Plugin identity mismatch');packages.push({directory,pluginId,version:m.version,requires:m.requires,bytes});}
await mkdir(output); // Never replace an existing delivery or its portable data.
const hashes={};
async function put(path,bytes){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),bytes);hashes[path]=hash(bytes);}
async function copy(source,target){await put(target,await readFile(source));}
async function tree(source,target){for(const entry of await readdir(source,{withFileTypes:true})){if(entry.isSymbolicLink())throw Error('Package source cannot contain links');if(entry.isDirectory())await tree(join(source,entry.name),join(target,entry.name));else if(entry.isFile())await copy(join(source,entry.name),join(target,entry.name));}}
for(const name of ['sodeck-desktop.exe','sodeck-plugin-host.exe','portable.json'])await copy(join(runtime,name),name);
await copy(join(runtime,'baseline.json'),'evidence/runtime-baseline.json');
await tree(join(runtime,'themes'),'themes');await tree(join(runtime,'assets'),'assets');
await copy(join(root,'resources/development-public-key.json'),'resources/development-public-key.json');
for(const pkg of packages)await put('resources/'+pkg.directory+'.sodeck-plugin',pkg.bytes);
await put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:packages.map(p=>({pluginId:p.pluginId,path:p.directory+'.sodeck-plugin'}))},null,2)+'\n'));
for(const [source,target]of [['docs/tasks/T-024-results.md','validation.md'],['docs/plugins/monitor/features/server-resources.md','specification.md'],['docs/technical/server-monitor-implementation.md','implementation.md'],['.qa/t024/package-verification/monitor-dopamine.png','monitor-live.png'],['.qa/t024/package-verification/results.json','directory-package-verification.json'],['.qa/t024/auth/results.json','authentication.json'],['.qa/t024/routes/results.json','routes.json'],['.qa/t024/lifecycle/results.json','lifecycle.json'],['.qa/t024/updates/results.json','updates.json'],['.qa/t024/linux/collection-results.json','linux-collections.json'],...(!preflight?[['.qa/t024/load/results.json','load.json'],['.qa/t024/load/summary.json','load-summary.json'],['.qa/t024/load/controlled-load-guest.json','guest-load.json'],['.qa/t024/load/idle-after-load-guest.json','guest-idle.json']]:[])])await copy(join(root,source),'evidence/'+target);
for(const [dir,names]of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)await copy(join(root,'third-party',dir,name),join('licenses',dir,name));
await put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
await put('README.md',Buffer.from(`# SoDeck ${baseline.version} · 资源监控开发目录包\n\n双击 Start-SoDeck.cmd。数据独立保存在本目录 data 中，随附终端、文件、网络隧道、资源监控四个签名插件。\n\n连接一台 Linux 服务器后，当前服务器 Tab 右侧显示 CPU、内存、文件系统、磁盘 I/O、网络指标。可暂停、单次刷新、切换采样间隔和趋势范围，也可移动到中央或独立窗口。历史仅保留在内存；隐藏视图停止采集。\n\n本包为 Windows debug profile 的本地开发验证产物，API ${baseline.pluginApiVersion}，不是安装器或正式发布。远端不需要安装 Agent。详细工程证据见 evidence/validation.md，用户验收另行记录。\n`));
const git=args=>execFileSync('git',['-c','core.safecrlf=false',...args],{cwd:root,windowsHide:true,maxBuffer:128*1024*1024,stdio:['ignore','pipe','pipe']});
const fingerprint=createHash('sha256').update(git(['diff','--binary','HEAD']));
for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){fingerprint.update(path+'\0');fingerprint.update(await readFile(join(root,path)));}
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T024',preflight,version:baseline.version,pluginApiVersion:baseline.pluginApiVersion,coreSchema:6,observationSchema:2,profile:'debug',release:false,builtAt:new Date().toISOString(),runtimeBaseline:baseline,sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:Boolean(git(['status','--porcelain']).length),sourceFingerprint:fingerprint.digest('hex'),sourceFingerprintPhase:'packaging; executable baseline recorded separately',pluginPackages:packages.map(({bytes,directory,...p})=>({...p,sha256:hash(bytes)})),hashes},null,2));
console.log(JSON.stringify({output,files:Object.keys(hashes).length}));
