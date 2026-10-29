// T-028 fresh isolated Windows directory; never imports ordinary user data.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,dirname,sep} from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
const root=resolve('.'),output=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T028-r1');
if(!output.startsWith(resolve('.local/dev')+sep))throw Error('Use a fresh directory under .local/dev');
const hash=bytes=>createHash('sha256').update(bytes).digest('hex'),files=new Map(),hashes={};
function put(path,bytes){files.set(path,bytes);hashes[path]=hash(bytes);}
async function copy(source,target=source){put(target,await readFile(source));}
for(const name of ['sodeck-desktop.exe','sodeck-plugin-host.exe'])await copy('target/debug/'+name,name);
const baseline=resolve('.local/dev/SoDeck-v2-T027-r9');const previous=JSON.parse(await readFile(join(baseline,'build.json'),'utf8'));
const plugins=[];
for(const name of ['terminal','files','network-proxy','monitor']){
  const path=`resources/${name}.sodeck-plugin`,bytes=await readFile(join(baseline,path));assertHash(path,bytes);put(path,bytes);
  const old=previous.plugins.find(p=>p.pluginId==='sodeck.'+name);if(!old)throw Error('Missing baseline plugin');plugins.push({pluginId:old.pluginId,version:old.version,path:name+'.sodeck-plugin',sha256:hash(bytes)});
}
function assertHash(path,bytes){if(previous.hashes[path]!==hash(bytes))throw Error('Baseline package changed: '+path);}
await copy(join(baseline,'resources/development-public-key.json'),'resources/development-public-key.json');
put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:plugins.map(({pluginId,path})=>({pluginId,path}))},null,2)+'\n'));
for(const name of ['graphite','soft-light','soft-dark','dopamine','catalog'])await copy(`themes/${name}.json`);
for(const name of await readdir('assets/brand'))if(/^(pocket-deck.*\.(png|ico|svg)|manifest\.json)$/.test(name))await copy('assets/brand/'+name);
for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)await copy(`third-party/${dir}/${name}`,`licenses/${dir}/${name}`);
for(const name of ['native','business','import']){
  const result=JSON.parse(await readFile(`.qa/t028/${name}-latest.json`,'utf8'));if(!result.passed)throw Error('Required validation did not pass: '+name);
  await copy(`.qa/t028/${name}-latest.json`,`evidence/${name}.json`);
  for(const image of await readdir(result.output))if(image.endsWith('.png')&&!image.includes('failure'))await copy(join(result.output,image),`evidence/${name}-${image}`);
}
for(const [source,target] of [['docs/tasks/T-028-results.md','evidence/validation.md'],['docs/core/plugin-management.md','evidence/specification.md'],['docs/plugin-development.md','plugin-development.md'],['.qa/t028/check.log','evidence/check.log'],['.qa/t028/contracts.log','evidence/contracts.log'],['.qa/t028/rust.log','evidence/rust.log'],['.qa/t028/manager.log','evidence/manager.log'],['.qa/t028/ui-final.log','evidence/ui.log']])await copy(source,target);
put('portable.json',Buffer.from('{"schemaVersion":1}\n'));
put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
const version=JSON.parse(await readFile('apps/desktop/package.json','utf8')).version;
put('README.md',Buffer.from(`# SoDeck ${version} · T-028 插件管理\n\n双击 Start-SoDeck.cmd。设置 → 插件管理可安装/更新插件并勾选启用。点击应用后，使用应用菜单“退出”并重新启动才生效；关闭主窗口只收起到托盘。\n\n完整性必须通过；未签名或未知来源显示提示但允许加载。已知错签/无效签名拒绝。停用保留包和配置且不启动插件代码，全部停用时核心仍可管理。\n\nAPI/SDK6.2，core数据库v7。随附四个已有签名插件原包，终端0.13.1、文件0.10.0、监控0.3.0、网络隧道0.6.0；测试专用无签名包不随附。\n\n数据在此目录data，与旧目录独立，不自动复制日常配置。旧核心会忽略新的启停文件；回退应使用旧独立目录或备份。\n\nWindows debug profile开发目录包，不是安装器或正式发布。实测见evidence/validation.md；第三方打包见plugin-development.md。\n`));
const git=args=>execFileSync('git',['-c','core.safecrlf=false',...args],{windowsHide:true,maxBuffer:128*1024*1024});
const fingerprint=createHash('sha256').update(git(['diff','--binary','HEAD']));for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){fingerprint.update(path+'\0');fingerprint.update(await readFile(path));}
await mkdir(output);for(const [path,bytes] of files){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),bytes);}
const result={deliveryTask:'T028',packageRevision:'r1',version,pluginApiVersion:'6.2.0',coreSchema:7,profile:'debug',release:false,sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:true,sourceFingerprint:fingerprint.digest('hex'),plugins,hashes};await writeFile(join(output,'build.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({output,files:files.size,desktopSha256:hashes['sodeck-desktop.exe']}));
