import {mkdir,copyFile,writeFile,readFile,access} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=fileURLToPath(new URL('../',import.meta.url));
const output=resolve(root,'.local/dev/SoDeck-v2');
await mkdir(join(output,'resources'),{recursive:true});
const files=[['target/debug/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/terminal.sodeck-plugin','resources/terminal.sodeck-plugin'],['resources/development-public-key.json','resources/development-public-key.json']];
const update='resources/sodeck.terminal-0.1.1.sodeck-plugin';
try{await access(join(root,update));files.push([update,update]);}catch(error){if(error.code!=='ENOENT')throw error;}
const hashes={};
for(const [source,target] of files){const bytes=await readFile(join(root,source));hashes[target]=createHash('sha256').update(bytes).digest('hex');await copyFile(join(root,source),join(output,target));}
await writeFile(join(output,'Start-SoDeck.cmd'),'@echo off\r\nsetlocal\r\nset "SODECK_DATA_DIR=%~dp0data"\r\nset "SODECK_PLUGIN_PACKAGE=%~dp0resources\\terminal.sodeck-plugin"\r\nset "SODECK_PLUGIN_KEYS=%~dp0resources\\development-public-key.json"\r\nset "SODECK_SHELL_DEBUG_PORT="\r\nset "SODECK_WEBVIEW_ARGS="\r\nstart "" "%~dp0sodeck-desktop.exe"\r\n');
await writeFile(join(output,'build.json'),JSON.stringify({version:'2.0.0-dev.1',builtAt:new Date().toISOString(),profile:'debug',release:false,hashes},null,2));
console.log(JSON.stringify({output,hashes}));
