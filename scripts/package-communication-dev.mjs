// Curated debug package, containing no fixture configuration or application data.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..'),output=resolve(process.argv[2]??join(root,'.local/dev/SoDeck-v2-T019'));
if(![join(root,'.local/dev')+sep,join(root,'.qa/t019')+sep].some(base=>output.startsWith(base)))throw Error('Package output must remain in this workspace delivery or T019 QA directory');
const files=[['target/debug/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/development-public-key.json','resources/development-public-key.json'],['docs/tasks/T-019-acceptance.md','README.md']];
const packages=[];
for(const name of ['terminal','files','network-proxy']){
  files.push([`resources/${name}.sodeck-plugin`,`resources/${name}.sodeck-plugin`]);
  const manifest=JSON.parse(await readFile(join(root,`plugins/${name}/manifest.json`),'utf8'));
  packages.push({pluginId:manifest.pluginId,version:manifest.version});
}
for(const name of ['graphite','soft-light','soft-dark','dopamine','catalog'])files.push([`themes/${name}.json`,`themes/${name}.json`]);
for(const name of await readdir(join(root,'assets/brand')))if(/^(pocket-deck.*\.(png|ico|svg)|manifest\.json)$/.test(name))files.push([`assets/brand/${name}`,`assets/brand/${name}`]);
for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)files.push([`third-party/${dir}/${name}`,`licenses/${dir}/${name}`]);
for(const name of ['final-check','final-core-tests','final-sdk-tests','final-ui-tests','final-quality-native','final-history-native'])files.push([`.qa/t019/${name}.log`,`evidence/${name}.log`]);
for(const name of ['real-ssh-diagnostics','real-ssh-history'])files.push([`.qa/t019/${name.includes('history')?'native-history':'native-quality'}/${name}.png`,`screenshots/${name}.png`]);
files.push(['.qa/t019/package-smoke/results.json','evidence/package-smoke.json']);
files.push(['.qa/t019/package-ordinary/results.json','evidence/package-ordinary.json'],['.qa/t019/package-ordinary.log','evidence/package-ordinary.log']);
for(const mode of ['direct','launcher'])files.push([`.qa/t019/package-ordinary/${mode}-startup.png`,`screenshots/${mode}-startup.png`]);
const hashes={};
async function put(path,bytes){
  const target=join(output,path);await mkdir(dirname(target),{recursive:true});
  // Repair a delivered package without rewriting its running executable or touching data.
  const current=await readFile(target).catch(error=>{if(error.code!=='ENOENT')throw error;return null;});
  if(!current?.equals(bytes))await writeFile(target,bytes);
  hashes[path]=createHash('sha256').update(bytes).digest('hex');
}
for(const [source,target] of files)await put(target,await readFile(resolve(root,source)));
await put('portable.json',Buffer.from(JSON.stringify({schemaVersion:1},null,2)+'\n'));
await put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:packages.map(p=>({pluginId:p.pluginId,path:p.pluginId.replace('sodeck.','')+'.sodeck-plugin'}))},null,2)+'\n'));
await put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DEV_MODE=0"\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
const git=args=>execFileSync('git',args,{cwd:root,windowsHide:true,maxBuffer:128*1024*1024});
const sourceHash=createHash('sha256').update(git(['diff','--binary','HEAD']));
for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){sourceHash.update(path+'\0');sourceHash.update(await readFile(join(root,path)));}
for(const pkg of packages)pkg.sha256=hashes[`resources/${pkg.pluginId.replace('sodeck.','')}.sodeck-plugin`];
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T019',version:'2.0.0-dev.15',packageRevision:2,pluginApi:4,pluginApiVersion:'4.9.0',pluginPackages:packages,builtAt:new Date().toISOString(),sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:Boolean(git(['status','--porcelain']).length),sourceFingerprint:sourceHash.digest('hex'),profile:'debug',release:false,hashes},null,2));
console.log(JSON.stringify({output,files:Object.keys(hashes).length}));
