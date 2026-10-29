// T-029 isolated development delivery. QA release keys/installers never ship here.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,dirname,sep} from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
const output=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T029-r2');
assert.ok(output.startsWith(resolve('.local/dev')+sep),'Use a fresh .local/dev directory');
const hash=b=>createHash('sha256').update(b).digest('hex'),files=new Map(),hashes={};
function put(path,bytes){files.set(path,bytes);hashes[path]=hash(bytes);}
async function copy(source,target=source){put(target,await readFile(source));}
for(const name of ['sodeck-desktop.exe','sodeck-plugin-host.exe'])await copy('target/debug/'+name,name);
const baseline=resolve('.local/dev/SoDeck-v2-T028-r1'),previous=JSON.parse(await readFile(join(baseline,'build.json'),'utf8')),plugins=[];
for(const name of ['terminal','files','network-proxy','monitor']){
  const path=`resources/${name}.sodeck-plugin`,bytes=await readFile(join(baseline,path));assert.equal(hash(bytes),previous.hashes[path]);put(path,bytes);
  const old=previous.plugins.find(p=>p.pluginId==='sodeck.'+name);assert.ok(old);plugins.push({pluginId:old.pluginId,version:old.version,path:name+'.sodeck-plugin',sha256:hash(bytes)});
}
await copy(join(baseline,'resources/development-public-key.json'),'resources/development-public-key.json');
put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:plugins.map(({pluginId,path})=>({pluginId,path}))},null,2)+'\n'));
for(const name of ['graphite','soft-light','soft-dark','dopamine','catalog'])await copy(`themes/${name}.json`);
for(const name of await readdir('assets/brand'))if(/^(pocket-deck.*\.(png|ico|svg)|manifest\.json)$/.test(name))await copy('assets/brand/'+name);
for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)await copy(`third-party/${dir}/${name}`,`licenses/${dir}/${name}`);
const installed=JSON.parse(await readFile('.qa/t029/installer-latest.json','utf8'));assert.equal(installed.passed,true);
const original=installed.priorEvidence?JSON.parse(await readFile(join(installed.priorEvidence,'result.json'),'utf8')):installed;
assert.ok(plugins.every(p=>original.checks.some(c=>c.startsWith(p.pluginId+':'))));
await copy(join(installed.evidence,'result.json'),'evidence/installer-core.json');await copy(join(original.evidence,'result.json'),'evidence/installer-plugins-and-prior-core-failure.json');
const installFailure=JSON.parse(await readFile('.qa/t029/installer-failure.json','utf8'));assert.equal(installFailure.passed,true);await copy('.qa/t029/installer-failure.json','evidence/installer-failure.json');await copy(join(installFailure.output,'reopened-A.png'),'evidence/installer-failure-reopened-A.png');
await copy('.qa/t029/publish-simulation.json','evidence/publish-simulation.json');
await copy('.qa/t029/desktop-rfd-final.log','evidence/desktop-rfd.log');
for(const name of await readdir(installed.evidence))if(name.endsWith('.png')&&!name.includes('failure'))await copy(join(installed.evidence,name),'evidence/'+name);
for(const name of await readdir(original.evidence))if(name.startsWith('sodeck.')&&name.endsWith('.png'))await copy(join(original.evidence,name),'evidence/'+name);
for(const [source,target] of [['.qa/t029/native-latest.json','evidence/native.json'],['.qa/t029/probe-latest.json','evidence/probe.json'],['.qa/t029/release-local/input-regression.json','evidence/input-regression.json'],['.qa/t029/check.log','evidence/check.log'],['.qa/t029/contracts-final.log','evidence/contracts.log'],['.qa/t029/rust-final2.log','evidence/rust.log'],['.qa/t029/ui-final.log','evidence/ui.log'],['.qa/t029/release-rules-final.log','evidence/release-rules.log'],['docs/plugin-development.md','plugin-development.md']])await copy(source,target);
put('portable.json',Buffer.from('{"schemaVersion":1}\n'));
put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
const version=JSON.parse(await readFile('apps/desktop/package.json','utf8')).version;
put('README.md',Buffer.from(`# SoDeck ${version} · T-029 在线更新\n\n双击 Start-SoDeck.cmd，进入设置 → 更新。核心与四个官方插件支持分别检查、下载和安装；检查不下载，下载不安装。插件准备后须完整退出重开；安装版核心使用NSIS交接并重开。\n\n此目录是Windows x64 debug开发试用包：development身份，未配置正式更新源，界面明确提示无法检查官方发布。可体验更新设置、插件管理、手动导入和现有业务。GitHub Actions/公网更新尚未验证；不要将QA密钥或测试包当成正式渠道。\n\n本地真实验证已完成：独立QA安装版核心A→B、四插件逐项A→B、新UI/worker与Linux PTY/SFTP/监控/HTTP/SOCKS/端口转发、加密凭据和设置保留、启停组合。证据在evidence；其中installer-plugins-and-prior-core-failure.json保留修复前核心失败，installer-core.json是修复后通过结果。\n\nAPI/SDK6.3，core数据库v7、四观测分区v2。随附T-028四个签名插件原包：终端0.13.1、文件0.10.0、监控0.3.0、网络隧道0.6.0。测试标记插件不随附。\n\n数据首次启动后写入本目录data，与旧开发目录独立，不复制日常配置。关闭主窗口只收起到托盘，完整退出请用应用菜单“退出”。本目录没有内置服务器、凭据或私钥；插件打包见plugin-development.md。\n`));
const git=args=>execFileSync('git',['-c','core.safecrlf=false',...args],{windowsHide:true,maxBuffer:128*1024*1024});
const fingerprint=createHash('sha256').update(git(['diff','--binary','HEAD']));for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){fingerprint.update(path+'\0');fingerprint.update(await readFile(path));}
await mkdir(output);for(const [path,bytes] of files){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),bytes);}
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T029',packageRevision:'r2',version,pluginApiVersion:'6.3.0',coreSchema:7,observationSchema:2,profile:'debug',release:false,sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:true,sourceFingerprint:fingerprint.digest('hex'),plugins,hashes},null,2));
console.log(JSON.stringify({output,files:files.size,desktopSha256:hashes['sodeck-desktop.exe']}));
