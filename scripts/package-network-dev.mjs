// Curated delivery: no application data, fixture configuration or private keys.
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname} from 'node:path';
const root=resolve(import.meta.dirname,'..'),output=join(root,'.local/dev/SoDeck-v2-T014-NetworkTunnel');
const files=[['target/debug/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/development-public-key.json','resources/development-public-key.json'],['docs/tasks/T-014-results.md','RESULTS.md'],['docs/tasks/T-014-acceptance.md','README.md']];
const packages=[];
for(const name of ['terminal','files','network-proxy']){
  files.push([`resources/${name}.sodeck-plugin`,`resources/${name}.sodeck-plugin`]);
  const manifest=JSON.parse(await readFile(join(root,`plugins/${name}/manifest.json`),'utf8'));
  packages.push({pluginId:manifest.pluginId,version:manifest.version});
}
for(const name of ['graphite','soft-light','soft-dark','dopamine','catalog'])files.push([`themes/${name}.json`,`themes/${name}.json`]);
for(const name of await readdir(join(root,'assets/brand')))if(/^(pocket-deck.*\.(png|ico|svg)|manifest\.json)$/.test(name))files.push([`assets/brand/${name}`,`assets/brand/${name}`]);
for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names)files.push([`third-party/${dir}/${name}`,`licenses/${dir}/${name}`]);
for(const name of ['native','openssh','load','lifecycle','updates','visual','benchmark']){
  const latest=JSON.parse(await readFile(join(root,`.qa/t014/${name}-latest.json`),'utf8'));
  files.push([join(latest.output,'results.json'),`evidence/${name}.json`]);
  if(name==='visual')for(const image of ['ready','editor-dopamine-100','editor-light-100','editor-dark-200','high-contrast'])files.push([join(latest.output,`${image}.png`),`screenshots/${image}.png`]);
}
files.push(['.qa/t014/compatibility/results.json','evidence/compatibility.json']);
files.push(['.qa/t014/linux/final-remote-audit.json','evidence/final-remote-audit.json']);
for(const name of ['check','contracts','browser','rust-final','core-network-final','build','native','openssh','load','lifecycle','updates'])files.push([`.qa/t014/${name}.log`,`evidence/${name}.log`]);
const omittedEvidence=[];
for(const name of ['package-smoke','cleanup']){
  const path=`.qa/t014/${name}.json`;
  try{await readFile(join(root,path));files.push([path,`evidence/${name}.json`]);}catch(error){if(error.code!=='ENOENT')throw error;omittedEvidence.push(path);}
}
const hashes={};
async function put(path,bytes){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),bytes);hashes[path]=createHash('sha256').update(bytes).digest('hex');}
for(const [source,target] of files){
  let bytes=await readFile(resolve(root,source));
  if(target==='RESULTS.md')bytes=Buffer.from(bytes.toString().replaceAll('(T-014-acceptance.md)','(README.md)'));
  await put(target,bytes);
}
await put('Start-SoDeck.cmd',Buffer.from('@echo off\r\nsetlocal\r\nset "SODECK_DATA_DIR=%~dp0data"\r\nset "SODECK_PLUGIN_PACKAGE=%~dp0resources\\terminal.sodeck-plugin"\r\nset "SODECK_PLUGIN_PACKAGES=%~dp0resources\\terminal.sodeck-plugin;%~dp0resources\\files.sodeck-plugin;%~dp0resources\\network-proxy.sodeck-plugin"\r\nset "SODECK_PLUGIN_KEYS=%~dp0resources\\development-public-key.json"\r\nset "SODECK_SHELL_DEBUG_PORT="\r\nset "SODECK_PLUGIN_DEBUG_PORTS="\r\nset "SODECK_WEBVIEW_ARGS="\r\nset "SODECK_THEME_FILE="\r\nset "SODECK_TEST_BRAND_FILE="\r\nset "SODECK_TEST_APPLICATION_MENU_FAILURE="\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n'));
const git=args=>execFileSync('git',args,{cwd:root,windowsHide:true,maxBuffer:64*1024*1024});
const sourceHash=createHash('sha256').update(git(['diff','--binary','HEAD']));
for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){sourceHash.update(path+'\0');sourceHash.update(await readFile(join(root,path)));}
const version=JSON.parse(await readFile(join(root,'apps/desktop/package.json'),'utf8')).version;
for(const pkg of packages)pkg.sha256=hashes[`resources/${pkg.pluginId.replace('sodeck.','')}.sodeck-plugin`];
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T014-NetworkTunnel',version,pluginVersion:packages[0].version,pluginApi:4,pluginApiVersion:'4.7.0',pluginPackages:packages,builtAt:new Date().toISOString(),sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:Boolean(git(['status','--porcelain']).length),sourceFingerprint:sourceHash.digest('hex'),profile:'debug',release:false,omittedEvidence,hashes},null,2));
console.log(JSON.stringify({output,files:Object.keys(hashes).length}));
