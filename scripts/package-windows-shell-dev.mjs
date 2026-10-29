// WIN-SHELL-01: reuse verified r8 binaries; update only the terminal package.
// User configuration is copied separately with the read-only core snapshot tool.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {join,resolve,dirname,sep} from 'node:path';
import {createHash} from 'node:crypto';

const source=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T027-r8');
const output=resolve(process.argv[3]??'.local/dev/SoDeck-v2-T027-r9');
if(!output.startsWith(resolve('.local/dev')+sep)||source===output)throw Error('Use a fresh isolated development directory');
const base=JSON.parse(await readFile(join(source,'build.json'),'utf8'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const files=new Map(),hashes={};
function put(path,bytes){files.set(path,bytes);hashes[path]=hash(bytes);}
for(const [path,expected] of Object.entries(base.hashes)){
  if(path.startsWith('evidence/')||path==='README.md'||path==='resources/terminal.sodeck-plugin')continue;
  const bytes=await readFile(join(source,path));if(hash(bytes)!==expected)throw Error('Base hash mismatch: '+path);put(path,bytes);
}
const terminal=await readFile('.qa/t027/packages/terminal-windows-fix.sodeck-plugin');
put('resources/terminal.sodeck-plugin',terminal);
put('README.md',Buffer.from('# SoDeck T-027 r9 · Windows Shell 修复\n\n双击 Start-SoDeck.cmd 启动。终端插件更新为 0.13.1；核心、API6.1、core v7 和其他三个插件与 r8 相同。\n\nWindows 或未确认支持的远端使用普通 SSH Shell，底栏显示“本会话未启用”；受支持的 Linux/Bash 按全局偏好使用目录集成。已有会话不热改，需要新建或明确重连。\n\nr8 配置通过只读在线备份复制到本目录 data/core，原目录继续保留，两份数据后续不自动同步。未复制自动连接工作区、插件缓存或 WebView profiles。来源见 evidence/config-copy.json。\n\n这是 Windows 开发目录包，不是安装器或正式发布；监控仍只支持 Linux。本轮验证见 evidence/windows-shell-native.json 和 evidence/validation.md。\n'));
for(const [src,dst] of [
  ['docs/tasks/T-027-results.md','evidence/validation.md'],
  ['docs/plugins/terminal/features/directory-reporting.md','evidence/directory-reporting.md'],
  ['.qa/t027/windows-shell-native-latest.json','evidence/windows-shell-native.json'],
  ['.qa/t027/windows-shell-tests.log','evidence/terminal-tests.log'],
  ['.qa/t027/windows-shell-repository-check.log','evidence/repository-check.log'],
])put(dst,await readFile(src));
const native=JSON.parse(await readFile('.qa/t027/windows-shell-native-latest.json','utf8'));
if(!native.passed)throw Error('Native verification must pass before packaging');
for(const name of ['windows-normal-shell.png','linux-integrated-shell.png'])put('evidence/'+name,await readFile(join(native.output,name)));
put('evidence/base-r8-build.json',await readFile(join(source,'build.json')));
const terminalFingerprint=createHash('sha256');
async function fingerprint(dir){for(const entry of (await readdir(dir,{withFileTypes:true})).sort((a,b)=>a.name.localeCompare(b.name))){if(['node_modules','dist'].includes(entry.name))continue;const path=join(dir,entry.name);if(entry.isDirectory())await fingerprint(path);else{terminalFingerprint.update(path+'\0');terminalFingerprint.update(await readFile(path));}}}
await fingerprint('plugins/terminal');
const build={...base,packageRevision:'r9',basePackage:source,baseSourceFingerprint:base.sourceFingerprint,sourceFingerprint:undefined,terminalSourceFingerprint:terminalFingerprint.digest('hex'),plugins:base.plugins.map(p=>p.pluginId==='sodeck.terminal'?{...p,version:'0.13.1',sha256:hash(terminal)}:p),hashes};
await mkdir(output);
for(const [path,bytes] of files){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),bytes);}
await writeFile(join(output,'build.json'),JSON.stringify(build,null,2));
console.log(JSON.stringify({output,desktopSha256:hashes['sodeck-desktop.exe'],terminalSha256:hash(terminal)}));
