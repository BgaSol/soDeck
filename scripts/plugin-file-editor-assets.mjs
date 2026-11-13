import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
const root=fileURLToPath(new URL('../',import.meta.url));
export function fileEditorSettings(metadata) {
 const expanded=structuredClone(metadata);
 for(const page of expanded.settings?.pages??[]){
  if(!page.fileEditor)continue;
  if(!expanded.build?.fileEditor)throw new Error('Editor settings require build.fileEditor');
  const template=JSON.parse(fs.readFileSync(path.join(root,'packages/file-editor/settings.json'),'utf8'));
  const {fileEditor,...overrides}=page;Object.assign(page,template,overrides);delete page.fileEditor;
 }
 return expanded;
}
export function buildFileEditor({install=true}={}) {
 const directory=path.join(root,'packages/file-editor');
 const cli=process.env.npm_execpath??path.join(path.dirname(process.execPath),'node_modules/npm/bin/npm-cli.js');
 for(const args of [...(install?[['ci','--ignore-scripts']]:[]),['run','build']]){
  const r=spawnSync(process.execPath,[cli,...args],{cwd:directory,stdio:'inherit',windowsHide:true});
  if(r.error)throw r.error;if(r.status!==0)throw new Error('File editor SDK build failed');
 }
}
export function packageFileEditor(directory) {
 fs.cpSync(path.join(root,'packages/file-editor/dist'),path.join(directory,'ui/file-editor'),{recursive:true});
}
