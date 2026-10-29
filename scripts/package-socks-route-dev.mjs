// Isolated T-020 directory package; never writes an existing package or its data.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..'),output=resolve(process.argv[2]??join(root,'.local/dev/SoDeck-v2-T020'));
if(![join(root,'.local/dev')+sep,join(root,'.qa/t020')+sep].some(base=>output.startsWith(base)))throw Error('Use a new T020 delivery directory');
await mkdir(output); // Fail if the destination exists, including a previously used portable data root.
const files=[['target/debug/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/development-public-key.json','resources/development-public-key.json'],['docs/tasks/T-020-results.md','evidence/validation.md'],['docs/core/socks-route.md','evidence/specification.md']];
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
for(const [source,target] of files)await put(target,await readFile(resolve(root,source)));
await put('portable.json',Buffer.from(JSON.stringify({schemaVersion:1},null,2)+'\n'));
await put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:packages.map(p=>({pluginId:p.pluginId,path:p.pluginId.replace('sodeck.','')+'.sodeck-plugin'}))},null,2)+'\n'));
await put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
await put('README.md',Buffer.from('# SoDeck v2 dev.17 · Windows 开发目录包\n\n双击 Start-SoDeck.cmd 启动，数据保存在本目录 data 下。包含终端、文件与网络隧道三个独立签名插件，以及 API 5.0 / 数据库 v6 / 观测 schema 2。\n\n本包为本地开发验证产物，工程验证状态见 evidence/validation.md；不代表用户验收或正式发布。请保持在独立目录使用。本次增加 SOCKS5 网络代理访问 SSH，主机隧道和网络代理各自独立。数据库版本不一致时先完整备份，再创建空 v6 库，不迁移旧配置。\n'));
const git=args=>execFileSync('git',args,{cwd:root,windowsHide:true,maxBuffer:128*1024*1024});
const sourceHash=createHash('sha256').update(git(['diff','--binary','HEAD']));
for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){sourceHash.update(path+'\0');sourceHash.update(await readFile(join(root,path)));}
for(const pkg of packages)pkg.sha256=hashes[`resources/${pkg.pluginId.replace('sodeck.','')}.sodeck-plugin`];
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T020',version:'2.0.0-dev.17',packageRevision:1,pluginApi:5,pluginApiVersion:'5.0.0',coreSchema:6,observationSchema:2,pluginPackages:packages,builtAt:new Date().toISOString(),sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:Boolean(git(['status','--porcelain']).length),sourceFingerprint:sourceHash.digest('hex'),profile:'debug',release:false,hashes},null,2));
console.log(JSON.stringify({output,files:Object.keys(hashes).length}));
