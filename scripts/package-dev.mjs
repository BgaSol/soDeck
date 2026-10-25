import {mkdir,copyFile,writeFile,readFile,access} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=fileURLToPath(new URL('../',import.meta.url));
const task=process.argv[2]??'T006';
if(!['T006','T007'].includes(task))throw new Error('Use T006 or T007 as the delivery task');
const output=resolve(root,`.local/dev/SoDeck-v2-${task}`);
await mkdir(join(output,'resources'),{recursive:true});
const files=[['target/debug/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/terminal.sodeck-plugin','resources/terminal.sodeck-plugin'],['resources/development-public-key.json','resources/development-public-key.json'],['themes/graphite.json','themes/graphite.json']];
files.push([`docs/tasks/${task==='T007'?'T-007':'T-006'}-results.md`,'RESULTS.md']);
const evidence=task==='T007'?[
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
for(const file of evidence){try{await access(join(root,file[0]));files.push(file);}catch{omittedEvidence.push(file[0]);}}
for(const license of ['LICENSE-MIT','LICENSE-APACHE-2.0'])files.push([`third-party/tauri-runtime-wry/${license}`,`licenses/tauri-runtime-wry/${license}`]);
await mkdir(join(output,'licenses/tauri-runtime-wry'),{recursive:true});
await mkdir(join(output,'themes'),{recursive:true});
const hashes={};
for(const [source,target] of files){const bytes=await readFile(join(root,source));hashes[target]=createHash('sha256').update(bytes).digest('hex');await mkdir(dirname(join(output,target)),{recursive:true});await copyFile(join(root,source),join(output,target));}
await writeFile(join(output,'Start-SoDeck.cmd'),'@echo off\r\nsetlocal\r\nset "SODECK_DATA_DIR=%~dp0data"\r\nset "SODECK_PLUGIN_PACKAGE=%~dp0resources\\terminal.sodeck-plugin"\r\nset "SODECK_PLUGIN_KEYS=%~dp0resources\\development-public-key.json"\r\nset "SODECK_SHELL_DEBUG_PORT="\r\nset "SODECK_WEBVIEW_ARGS="\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n');
await copyFile(join(root,`docs/tasks/${task==='T007'?'T-007':'T-006'}-acceptance.md`),join(output,'README.md'));
for(const file of ['Start-SoDeck.cmd','README.md'])hashes[file]=createHash('sha256').update(await readFile(join(output,file))).digest('hex');
const sourceCommit=execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8',windowsHide:true}).trim();
const sourceDirty=Boolean(execFileSync('git',['status','--porcelain'],{cwd:root,encoding:'utf8',windowsHide:true}).trim());
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:task,version:'2.0.0-dev.3',pluginVersion:'0.3.0',pluginApi:3,builtAt:new Date().toISOString(),sourceCommit,sourceDirty,profile:'debug',release:false,omittedEvidence,hashes},null,2));
console.log(JSON.stringify({output,hashes}));
