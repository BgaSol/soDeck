import {mkdir,copyFile,writeFile,readFile,access} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=fileURLToPath(new URL('../',import.meta.url));
const task=process.argv[2]??'T009';
if(task==='T014-NetworkTunnel'){await import('./package-network-dev.mjs');process.exit(0);}
const multihop=task==='T013-Multihop';
if(!['T006','T007','T008','T009','T010','T011','T012','T013','T013-Multihop'].includes(task))throw new Error('Use T006, T007, T008, T009, T010, T011, T012, T013 or T013-Multihop as the delivery task');
const dual=['T008','T010','T011','T012','T013','T013-Multihop'].includes(task);
const taskDoc=multihop?'T-013-multihop':task.replace('T','T-');
const output=resolve(root,`.local/dev/SoDeck-v2-${task}`);
await mkdir(join(output,'resources'),{recursive:true});
const files=[['target/debug/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/terminal.sodeck-plugin','resources/terminal.sodeck-plugin'],['resources/development-public-key.json','resources/development-public-key.json'],['themes/graphite.json','themes/graphite.json']];
if(multihop||task==='T011'||task==='T012'||task==='T013')for(const mode of ['light','dark'])files.push([`themes/soft-${mode}.json`,`themes/soft-${mode}.json`]);
if(dual){
  if(task==='T008')files[1]=['.qa/t008/baseline/terminal-0.4.0.sodeck-plugin','resources/terminal.sodeck-plugin'];
  files.push(['resources/files.sodeck-plugin','resources/files.sodeck-plugin']);
  for(const name of ['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md'])files.push([`third-party/ssh2/${name}`,`licenses/ssh2/${name}`]);
}
if(multihop||task==='T012'){files.push(['themes/dopamine.json','themes/dopamine.json'],['themes/catalog.json','themes/catalog.json']);for(const name of ['pocket-deck.svg','pocket-deck-micro.svg','pocket-deck-monochrome.svg','manifest.json','pocket-deck.ico',...([16,20,24,32,40,48,64,128,256,512].map(n=>`pocket-deck-${n}.png`))])files.push([`assets/brand/${name}`,`assets/brand/${name}`]);}
files.push([`docs/tasks/${taskDoc}-results.md`,'RESULTS.md']);
const latest=task==='T009'?JSON.parse(await readFile(join(root,'.qa/t009/native-latest.json'),'utf8')):null;
const plugins=task==='T009'?JSON.parse(await readFile(join(root,'.qa/t009/plugins-latest.json'),'utf8')):null;
const evidence=multihop?[
 ...['native/results','native/old-results','native/old-installed','native/files-only-results','visual/results','visual/long-results','visual/ime-events','migration/results','core-performance'].map(n=>['.qa/t013-multihop/'+n+'.json','evidence/'+n.replaceAll('/','-')+'.json']),
 ...['check','contracts','rust-tests','core-all-routes','core-multihop-final','core-state','core-consumers','browser','desktop-debug','native','old-plugins','files-only','visual','migration'].map(n=>['.qa/t013-multihop/'+n+'.log','evidence/'+n+'.log']),
 ...['terminal','files','workspace-dopamine','workspace-light','workspace-dark','path-details'].map(n=>['.qa/t013-multihop/native/'+n+'.png','screenshots/'+n+'.png']),
 ...['editor-dopamine-100','editor-light-100','editor-dark-200','trust-2-of-4','delete-dependency','high-contrast','long-path'].map(n=>['.qa/t013-multihop/visual/'+n+'.png','screenshots/'+n+'.png']),
 ['.qa/t013-multihop/linux/topology.txt','evidence/topology.txt'],['.qa/t013-multihop/linux/final-remote-audit.txt','evidence/final-remote-audit.txt'],['.qa/t013-multihop/package-smoke.json','evidence/package-smoke.json'],['.qa/t013-multihop/cleanup.json','evidence/cleanup.json']
]:task==='T012'?[
 ...['native','compatibility','visual','lifecycle','brand','menu-lifecycle'].map(n=>[`.qa/t012/${n}/results.json`,`evidence/${n}.json`]),
 ...['check','contracts','rust','browser','components','components-final','native','compatibility','visual','lifecycle','menu-lifecycle','desktop'].map(n=>[`.qa/t012/${n}.log`,`evidence/${n}.log`]),
 ...['dopamine','light','dark'].flatMap(n=>[[`.qa/t012/native/workspace-${n}.png`,`screenshots/workspace-${n}.png`],[`.qa/t012/native/application-menu-${n}.png`,`screenshots/menu-${n}.png`],[`.qa/t012/native/appearance-${n}.png`,`screenshots/appearance-${n}.png`]]),
 ['.qa/t012/old-package-integrity.json','evidence/old-package-integrity.json'],['.qa/t012/ui/contrast.json','evidence/contrast.json'],['.qa/t012/brand/contact-sheet.png','screenshots/brand.png'],['.qa/t012/brand/taskbar.png','screenshots/taskbar.png'],['.qa/t012/rust-desktop-final.log','evidence/rust-desktop-final.log'],['.qa/t012/package-smoke.json','evidence/package-smoke.json'],['.qa/t012/cleanup.json','evidence/cleanup.json']
]:task==='T013'?[
 ...['native/results','native/old-results','native/files-only-results','visual/results','migration/results','load/results'].map(n=>['.qa/t013/'+n+'.json','evidence/'+n.replaceAll('/','-')+'.json']),
 ...['check','contracts','rust-tests','core-jump','browser','desktop','native','old-plugins','files-only','visual','migration','load'].map(n=>['.qa/t013/'+n+'.log','evidence/'+n+'.log']),
 ...['terminal','files','workspace-light','workspace-dark','resources'].map(n=>['.qa/t013/native/'+n+'.png','screenshots/'+n+'.png']),
 ...['editor-light-100','editor-dark-200','gateway-trust','target-trust','path-details','delete-dependency','high-contrast'].map(n=>['.qa/t013/visual/'+n+'.png','screenshots/'+n+'.png']),
 ['.qa/t013/load/qemu-capacity-diagnostic.json','evidence/qemu-capacity-diagnostic.json'],['.qa/t013/visual/ime-events.json','evidence/ime-events.json'],['.qa/t013/linux/topology.txt','evidence/topology.txt'],['.qa/t013/package-smoke.json','evidence/package-smoke.json'],['.qa/t013/cleanup.json','evidence/cleanup.json']
]:task==='T011'?[
  ...['native','compatibility','visual','regression','lifecycle'].map(name=>[`.qa/t011/${name}/results.json`,`evidence/${name}.json`]),
  ...['check','contracts','rust','browser','supplemental-browser','build','desktop-debug','native','compatibility','visual','regression','lifecycle'].map(name=>[`.qa/t011/${name}.log`,`evidence/${name}.log`]),
  ['.qa/t011/ui/contrast.json','evidence/contrast.json'],
  ...['workspace-light','workspace-dark'].map(name=>[`.qa/t011/native/${name}.png`,`screenshots/${name}.png`]),
  ...['light','dark'].flatMap(mode=>[100,150,200].map(scale=>[`.qa/t011/visual/${mode}-${scale}.png`,`screenshots/${mode}-${scale}.png`])),
  ...['appearance-light','appearance-dark','editor-light','editor-dark','ime','high-contrast'].map(name=>[`.qa/t011/visual/${name}.png`,`screenshots/${name}.png`]),
  ['.qa/t011/visual/ime-events.json','evidence/ime-events.json'],
  ['.qa/t011/package-smoke.json','evidence/package-smoke.json'],
  ['.qa/t011/cleanup.json','evidence/cleanup.json'],
]:task==='T010'?[
  ...['native','compatibility','visual','ime'].map(name=>[`.qa/t010/${name}/results.json`,`evidence/${name}.json`]),
  ...['shell-matrix','shell-gate'].map(name=>[`.qa/t010/linux/${name}.json`,`evidence/${name}.json`]),
  ...['directory-follow','native-window'].map(name=>[`.qa/t010/native/${name}.png`,`screenshots/${name}.png`]),
  ...['graphite','paper'].flatMap(theme=>[100,150,200].map(scale=>[`.qa/t010/visual/${theme}-${scale}.png`,`screenshots/${theme}-${scale}.png`])),
  ['.qa/t010/ime/composition.png','screenshots/ime-composition.png'],
  ['.qa/t010/ime/composition-events.json','evidence/ime-events.json'],
  ...['check-final','contracts-final','rust-final','browser-final','desktop-final','native-final','compatibility-final','shell-matrix-final','pressure','pty-real','remote-final'].map(name=>[`.qa/t010/${name}.log`,`evidence/${name}.log`]),
  ...['hidden-io','package-smoke','cleanup'].map(name=>[`.qa/t010/${name}.json`,`evidence/${name}.json`]),
]:task==='T008'?[
  ...['native-files','updates','degraded','visual','platform'].map(name=>[`.qa/t008/${name}/results.json`,`evidence/${name}.json`]),
  ['.qa/t008/load/metrics.json','evidence/performance.json'],
  ...['files-tree','network-mount','stacked-mounts','two-servers'].map(name=>[`.qa/t008/native-files/${name}.png`,`screenshots/${name}.png`]),
  ...['graphite','paper','large-text'].map(name=>[`.qa/t008/visual/${name}.png`,`screenshots/${name}.png`]),
  ...['check','contracts','boundaries','rust-tests','core-remote','ui-files','browser','native-files','updates','degraded','load','visual','platform','clean-start','clean-start-plugins'].map(name=>[`.qa/t008/${name}.log`,`evidence/${name}.log`]),
  ['.qa/t008/package-smoke.json','evidence/package-smoke.json'],
  ['.qa/t008/cleanup.json','evidence/cleanup.json'],
]:task==='T009'?[
  [join(latest.output,'results.json'),'evidence/native-results.json'],
  [join(plugins.output,'results.json'),'evidence/plugin-results.json'],
  ...['clean-restart','initial-auth','initial-terminal','canceled-connection'].map(name=>[join(latest.output,`${name}.png`),`screenshots/${name}.png`]),
  [join(plugins.output,'missing-plugin.png'),'screenshots/missing-plugin.png'],
  ...['check','contract','rust','browser','native','updates','desktop'].map(name=>[`.qa/t009-${name}.log`,`evidence/${name}.log`]),
]:task==='T007'?[
  ['.qa/t007-native/1792941277572/results.json','evidence/native-results.json'],
  ...['native-picker','native-editor','graphite-picker','paper-picker'].map(name=>[`.qa/t007/${name}.png`,`screenshots/${name}.png`]),
  ...['check','browser','native','package-smoke'].map(name=>[`.qa/t007-${name}.log`,`evidence/${name}.log`]),
]:[
  ['.qa/t006-themes/1792938111583/graphite-main.png','screenshots/graphite-main.png'],
  ['.qa/t006-themes/1792938111583/paper-main.png','screenshots/paper-main.png'],
  ['.qa/t006-themes/1792938111583/graphite-menu.png','screenshots/graphite-menu.png'],
  ['.qa/t006-native/1792938522647/native-menu.png','screenshots/native-menu.png'],
  ['.qa/t006-themes/1792938111583/results.json','evidence/themes.json'],
  ['.qa/t006-native/1792938522647/results.json','evidence/native.json'],
  ['.qa/ime/1792938373142/result.json','evidence/ime.json'],
  ...['check','contracts','rust','browser','manager-final','desktop-final'].map(name=>[`.qa/t006-${name}.log`,`evidence/${name}.log`]),
];
const omittedEvidence=[];
for(const file of evidence){try{await access(resolve(root,file[0]));files.push(file);}catch{omittedEvidence.push(file[0]);}}
for(const license of ['LICENSE-MIT','LICENSE-APACHE-2.0'])files.push([`third-party/tauri-runtime-wry/${license}`,`licenses/tauri-runtime-wry/${license}`]);
await mkdir(join(output,'licenses/tauri-runtime-wry'),{recursive:true});
await mkdir(join(output,'themes'),{recursive:true});
const hashes={};
for(const [source,target] of files){let bytes=await readFile(resolve(root,source));if(['T008','T011'].includes(task)&&target==='RESULTS.md')bytes=Buffer.from(bytes.toString('utf8').replaceAll(`(${taskDoc}-acceptance.md)`,'(README.md)'));hashes[target]=createHash('sha256').update(bytes).digest('hex');await mkdir(dirname(join(output,target)),{recursive:true});await writeFile(join(output,target),bytes);}
await writeFile(join(output,'Start-SoDeck.cmd'),'@echo off\r\nsetlocal\r\nset "SODECK_DATA_DIR=%~dp0data"\r\nset "SODECK_PLUGIN_PACKAGE=%~dp0resources\\terminal.sodeck-plugin"\r\nset "SODECK_PLUGIN_KEYS=%~dp0resources\\development-public-key.json"\r\nset "SODECK_SHELL_DEBUG_PORT="\r\nset "SODECK_WEBVIEW_ARGS="\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n');
if(dual)await writeFile(join(output,'Start-SoDeck.cmd'),'@echo off\r\nsetlocal\r\nset "SODECK_DATA_DIR=%~dp0data"\r\nset "SODECK_PLUGIN_PACKAGE=%~dp0resources\\terminal.sodeck-plugin"\r\nset "SODECK_PLUGIN_PACKAGES=%~dp0resources\\terminal.sodeck-plugin;%~dp0resources\\files.sodeck-plugin"\r\nset "SODECK_PLUGIN_KEYS=%~dp0resources\\development-public-key.json"\r\nset "SODECK_SHELL_DEBUG_PORT="\r\nset "SODECK_PLUGIN_DEBUG_PORTS="\r\nset "SODECK_WEBVIEW_ARGS="\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n');
await copyFile(join(root,`docs/tasks/${taskDoc}-acceptance.md`),join(output,'README.md'));
if(multihop||task==='T011'||task==='T012'||task==='T013'){const launcher=join(output,'Start-SoDeck.cmd');await writeFile(launcher,(await readFile(launcher,'utf8')).replace('set "SODECK_WEBVIEW_ARGS="','set "SODECK_WEBVIEW_ARGS="\r\nset "SODECK_THEME_FILE="\r\nset "SODECK_TEST_BRAND_FILE="\r\nset "SODECK_TEST_APPLICATION_MENU_FAILURE="'));}
for(const file of ['Start-SoDeck.cmd','README.md'])hashes[file]=createHash('sha256').update(await readFile(join(output,file))).digest('hex');
const sourceCommit=execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8',windowsHide:true}).trim();
const sourceDirty=Boolean(execFileSync('git',['status','--porcelain'],{cwd:root,encoding:'utf8',windowsHide:true}).trim());
const sourceHash=createHash('sha256').update(execFileSync('git',['diff','--binary','HEAD'],{cwd:root,windowsHide:true,maxBuffer:64*1024*1024}));
for(const path of execFileSync('git',['ls-files','--others','--exclude-standard','-z'],{cwd:root,encoding:'utf8',windowsHide:true}).split('\0').filter(Boolean).sort()){sourceHash.update(path+'\0');sourceHash.update(await readFile(join(root,path)));}
const version=JSON.parse(await readFile(join(root,'apps/desktop/package.json'),'utf8')).version;
const pluginVersion=multihop?'0.7.1':task==='T012'?'0.7.0':task==='T013'?'0.6.0':task==='T011'?'0.6.0':task==='T010'?'0.5.0':'0.4.0';
const pluginPackages=dual?[{pluginId:'sodeck.terminal',version:pluginVersion,sha256:hashes['resources/terminal.sodeck-plugin']},{pluginId:'sodeck.files',version:multihop?'0.4.1':task==='T012'?'0.4.0':task==='T013'?'0.3.3':task==='T011'?'0.3.0':task==='T010'?'0.2.0':'0.1.0',sha256:hashes['resources/files.sodeck-plugin']}]:undefined;
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:task,version,pluginVersion,pluginApi:4,...(dual?{pluginApiVersion:multihop?'4.6.0':task==='T012'?'4.5.0':task==='T013'?'4.4.0':task==='T011'?'4.3.0':task==='T010'?'4.2.0':'4.1.0',pluginPackages}:{}),builtAt:new Date().toISOString(),sourceCommit,sourceDirty,sourceFingerprint:sourceHash.digest('hex'),profile:'debug',release:false,omittedEvidence,hashes},null,2));
console.log(JSON.stringify({output,hashes}));
