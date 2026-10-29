// Package the validated T-025 executable and plugin archives into a NEW portable directory.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..'),output=resolve(process.argv[2]??join(root,'.local/dev/SoDeck-v2-T025'));
if(!output.startsWith(join(root,'.local/dev')+sep))throw Error('Use a new isolated directory under .local/dev');
await mkdir(output);
const runtime=join(root,'.qa/t025/runtime/sodeck-desktop.exe'),version='2.0.0-dev.19';
const files=[[runtime,'sodeck-desktop.exe'],['resources/development-public-key.json','resources/development-public-key.json'],['docs/tasks/T-025-results.md','evidence/validation.md'],['docs/core/connection-home.md','evidence/specification.md']];
const packages=[];
for(const name of ['terminal','files','network-proxy']){
 const archive=join(root,`.qa/t025/packages/${name}.sodeck-plugin`);
 const python='C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
 const manifest=JSON.parse(execFileSync(python,['-c',"import zipfile,sys; print(zipfile.ZipFile(sys.argv[1]).read('manifest.json').decode())",archive],{windowsHide:true,encoding:'utf8'}));
 packages.push({pluginId:manifest.pluginId,version:manifest.version});files.push([archive,`resources/${name}.sodeck-plugin`]);
}
for(const name of ['graphite','soft-light','soft-dark','dopamine','catalog'])files.push([`themes/${name}.json`,`themes/${name}.json`]);
for(const name of await readdir(join(root,'assets/brand')))if(/^(pocket-deck.*\.(png|ico|svg)|manifest\.json)$/.test(name))files.push([`assets/brand/${name}`,`assets/brand/${name}`]);
for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)files.push([`third-party/${dir}/${name}`,`licenses/${dir}/${name}`]);
const visual=JSON.parse(await readFile(join(root,'.qa/t025/visual-latest.json'),'utf8'));
for(const name of ['home-dopamine-100','home-soft-light-100','home-soft-dark-200','home-high-contrast','home-ime-composition'])files.push([join(visual.output,name+'.png'),'evidence/'+name+'.png']);
for(const name of ['native','socks','visual','compatibility'])files.push([`.qa/t025/${name}-latest.json`,`evidence/${name}.json`]);
const hashes={};
async function put(path,bytes){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),bytes);hashes[path]=createHash('sha256').update(bytes).digest('hex');}
for(const [source,target] of files)await put(target,await readFile(resolve(root,source)));
await put('portable.json',Buffer.from(JSON.stringify({schemaVersion:1},null,2)+'\n'));
await put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:packages.map(p=>({pluginId:p.pluginId,path:p.pluginId.replace('sodeck.','')+'.sodeck-plugin'}))},null,2)+'\n'));
await put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
await put('README.md',Buffer.from(`# SoDeck ${version} · 连接首页开发目录包\n\n双击 Start-SoDeck.cmd 启动。数据保存在本目录 data 下，与已有试用包隔离。需要 Windows 和 WebView2。\n\n启动或点服务器 Tab 的＋进入连接页：左侧目录和新建入口，右侧表单。SSH 可仅保存或保存并连接；SSH 跳板只选择已保存服务器；代理可以就地保存并复用。\n\n包含终端、文件、网络隧道三个独立签名插件。API/SDK 5.3，业务数据库 v6；监控插件是其他任务的在途交付，本包不包含。\n\n此包使用已验证的 debug profile 原生程序与生产前端资源，方便本地验收；不是正式发布或安装器。设置中心沿用 T-023 已确认行为。详细验证、截图和限制见 evidence/validation.md。\n\n建议验收：\n1. 启动与连续点＋，确认只有一个连接页。\n2. 新建 SSH 并仅保存，再从左侧打开。\n3. 为目标就地创建代理，取消目标后确认代理仍能复用。\n4. 先仅保存跳板，再从目标选择已有节点。\n5. 切换草稿、服务器页与设置，尝试退出后取消，确认修改保留。\n`));
const git=args=>execFileSync('git',['-c','core.safecrlf=false',...args],{cwd:root,windowsHide:true,maxBuffer:128*1024*1024});
const fingerprint=createHash('sha256').update(git(['diff','--binary','HEAD']));
for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){fingerprint.update(path+'\0');fingerprint.update(await readFile(join(root,path)));}
for(const p of packages)p.sha256=hashes[`resources/${p.pluginId.replace('sodeck.','')}.sodeck-plugin`];
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T025',version,packageRevision:1,pluginApiVersion:'5.3.0',coreSchema:6,profile:'debug',release:false,builtAt:new Date().toISOString(),sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:true,sourceFingerprint:fingerprint.digest('hex'),sourceFingerprintPhase:'packaging shared working tree; validated executable hash authoritative',pluginPackages:packages,hashes},null,2));
console.log(JSON.stringify({output,version,files:Object.keys(hashes).length,sha256:hashes['sodeck-desktop.exe']}));
