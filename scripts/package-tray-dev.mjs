// Build an isolated portable folder; never overwrite an existing data directory.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..');
const output=resolve(process.argv[2]??join(root,'.local/dev/SoDeck-v2-Tray'));
if(![join(root,'.local/dev')+sep,join(root,'.qa/tray')+sep].some(base=>output.startsWith(base)))throw Error('Output must be a new isolated development folder');
await mkdir(output);
const files=[['target/release/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/development-public-key.json','resources/development-public-key.json']];
for(const name of ['terminal','files','network-proxy'])files.push([`resources/${name}.sodeck-plugin`,`resources/${name}.sodeck-plugin`]);
for(const name of ['graphite','soft-light','soft-dark','dopamine','catalog'])files.push([`themes/${name}.json`,`themes/${name}.json`]);
for(const name of await readdir(join(root,'assets/brand')))if(/^(pocket-deck.*\.(png|ico|svg)|manifest\.json)$/.test(name))files.push([`assets/brand/${name}`,`assets/brand/${name}`]);
for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)files.push([`third-party/${dir}/${name}`,`licenses/${dir}/${name}`]);
const hashes={};async function put(path,bytes){const target=join(output,path);await mkdir(dirname(target),{recursive:true});await writeFile(target,bytes);hashes[path]=createHash('sha256').update(bytes).digest('hex');}
for(const[source,target]of files)await put(target,await readFile(join(root,source)));
await put('portable.json',Buffer.from('{"schemaVersion":1}\n'));
await put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:['terminal','files','network-proxy'].map(n=>({pluginId:'sodeck.'+n,path:n+'.sodeck-plugin'}))},null,2)));
await put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
await put('README.md',Buffer.from('# SoDeck v2 · 托盘交互试用包\n\n双击 sodeck-desktop.exe 或 Start-SoDeck.cmd 启动。左键单击托盘显示主窗口，右键提供显示及退出。数据独立保存在本目录 data/，不覆盖其他开发包。\n\n基于 dev.16 / API4.10 / core schema5 / 观测schema2；本次没有升级数据格式或插件。包含诊断目录数字上限重启误判的修复。构建与验证详见 RESULTS.md；这是本地试用包，不是正式发布。\n'));
await writeFile(join(output,'build.json'),JSON.stringify({task:'TRAY-001',version:JSON.parse(await readFile(join(root,'package.json'),'utf8')).version,profile:'release',sourceDirty:true,builtAt:new Date().toISOString(),hashes},null,2));
console.log(output);
