// Isolated T-023 directory package; never writes an existing package or its data.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..'),output=resolve(process.argv[2]??join(root,'.local/dev/SoDeck-v2-T023'));
// A validated executable may predate concurrent source/version changes in this shared workspace.
const baselinePath=process.env.SODECK_T023_RUNTIME_BASELINE;
const baseline=baselinePath?JSON.parse(await readFile(resolve(baselinePath),'utf8')):undefined;
const executable=baselinePath?join(dirname(resolve(baselinePath)),'sodeck-desktop.exe'):join(root,'target/release/sodeck-desktop.exe');
if(baseline&&createHash('sha256').update(await readFile(executable)).digest('hex')!==baseline.hashes['sodeck-desktop.exe'])throw Error('Validated executable hash changed');
const version=baseline?.version??JSON.parse(await readFile(join(root,'apps/desktop/package.json'),'utf8')).version;
const pluginApiVersion=baseline?.pluginApiVersion??JSON.parse(await readFile(join(root,'packages/sdk/package.json'),'utf8')).version;
if(![join(root,'.local/dev')+sep,join(root,'.qa/t023')+sep].some(base=>output.startsWith(base)))throw Error('Use a new T023 delivery directory');
await mkdir(output); // Fail if the destination exists, including a previously used portable data root.
const files=[[executable,'sodeck-desktop.exe'],['resources/development-public-key.json','resources/development-public-key.json'],['docs/tasks/T-023-results.md','evidence/validation.md'],['docs/core/settings.md','evidence/specification.md']];
const packages=[];
for(const name of ['terminal','files','network-proxy']){
  files.push([`resources/${name}.sodeck-plugin`,`resources/${name}.sodeck-plugin`]);
  const manifest=JSON.parse(await readFile(join(root,`plugins/${name}/manifest.json`),'utf8'));
  packages.push({pluginId:manifest.pluginId,version:manifest.version});
}
for(const name of ['graphite','soft-light','soft-dark','dopamine','catalog'])files.push([`themes/${name}.json`,`themes/${name}.json`]);
for(const name of await readdir(join(root,'assets/brand')))if(/^(pocket-deck.*\.(png|ico|svg)|manifest\.json)$/.test(name))files.push([`assets/brand/${name}`,`assets/brand/${name}`]);
for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)files.push([`third-party/${dir}/${name}`,`licenses/${dir}/${name}`]);
const hashes={};
async function put(path,bytes){const target=join(output,path);await mkdir(dirname(target),{recursive:true});await writeFile(target,bytes);hashes[path]=createHash('sha256').update(bytes).digest('hex');}
for(const name of ['settings-dopamine.png','settings-soft-dark.png','settings-soft-light.png','settings-scale-200.png','settings-terminal.png'])files.push(['.qa/t023/native/'+name,'evidence/'+name]);
for(const [source,target] of files)await put(target,await readFile(resolve(root,source)));
await put('portable.json',Buffer.from(JSON.stringify({schemaVersion:1},null,2)+'\n'));
await put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:packages.map(p=>({pluginId:p.pluginId,path:p.pluginId.replace('sodeck.','')+'.sodeck-plugin'}))},null,2)+'\n'));
await put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
await put('README.md',Buffer.from('# SoDeck '+version+' · Windows 设置中心开发目录包\n\n双击 Start-SoDeck.cmd 启动，点击右上角齿轮打开设置。数据保存在本目录 data 下。包含终端、文件与网络隧道三个独立签名插件。\n\n设置按应用和插件分组；修改先留在草稿，点击应用后保存。终端声明字号和 Shell 目录集成；字号同步到已有终端，目录集成仅影响新建或明确重连会话。\n\n这是 release profile 编译的本地开发包，不是正式发布或安装器。验证结果见 evidence/validation.md。首次保存外观会写入 v3 并保留旧文件备份；请保持本目录数据独立，不用旧核心直接打开新数据。\n'));
const git=args=>execFileSync('git',['-c','core.safecrlf=false',...args],{cwd:root,windowsHide:true,maxBuffer:128*1024*1024});
const sourceHash=createHash('sha256').update(git(['diff','--binary','HEAD']));
for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){sourceHash.update(path+'\0');sourceHash.update(await readFile(join(root,path)));}
for(const pkg of packages)pkg.sha256=hashes[`resources/${pkg.pluginId.replace('sodeck.','')}.sodeck-plugin`];
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T023',version,packageRevision:1,pluginApi:5,pluginApiVersion,coreSchema:6,observationSchema:2,pluginPackages:packages,builtAt:new Date().toISOString(),sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:Boolean(git(['status','--porcelain']).length),sourceFingerprint:sourceHash.digest('hex'),sourceFingerprintPhase:'packaging',profile:'release',release:true,hashes},null,2));
console.log(JSON.stringify({output,files:Object.keys(hashes).length}));
